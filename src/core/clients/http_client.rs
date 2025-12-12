use futures::future::join_all;
use rand::rngs::SmallRng;
use rand::seq::IndexedRandom;
use rand::SeedableRng;
use reqwest::{Client, ClientBuilder, StatusCode};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, warn};

use super::path::Path;
use crate::core::metrics::exporter_metrics::EXPORTER_HTTP_REQUESTS;

#[derive(Debug, Clone)]
struct Node {
    name: String,
    url: String,
    health_url: Path,
    healthy: bool,
    consecutive_failures: usize,
    network: String,
    // Track successful endpoint patterns per node (e.g., "tx_search" -> true if node successfully returned data)
    // This allows us to prefer nodes that have successfully handled specific endpoints before
    successful_endpoints: std::collections::HashSet<String>,
}

impl Node {
    fn new(name: String, url: String, health_url: String, network: String) -> Self {
        let url = if url.ends_with('/') {
            url.trim_end_matches('/').to_string()
        } else {
            url
        };
        Node {
            name,
            url,
            health_url: Path::from(health_url),
            healthy: true,
            consecutive_failures: 0,
            network,
            successful_endpoints: std::collections::HashSet::new(),
        }
    }

    async fn check_health(self, client: &Client) -> bool {
        let health_url = format!("{}{}", self.url, self.health_url);

        match client.get(&health_url).send().await {
            Ok(response) => {
                let status = response.status();
                EXPORTER_HTTP_REQUESTS
                    .with_label_values(&[&self.url, &status.as_u16().to_string(), &self.network])
                    .inc();

                if status == StatusCode::OK {
                    true
                } else {
                    warn!(
                        "(NodePool) Health check failed for {} with status {}",
                        health_url, status
                    );
                    false
                }
            }
            Err(_) => {
                warn!("(NodePool) Health check failed for {}", health_url);
                false
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum NodePoolErrors {
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("No healthy nodes to call for: {0}")]
    NoHealthyNodes(String),
}

pub struct NodePool {
    nodes: Arc<RwLock<Vec<Node>>>,
    client: Client,
    health_check_interval: Duration,
    http_timeout: Duration, // Store HTTP client timeout for adaptive retry logic
}

///
/// NodePool tracks the health of nodes (endpoints)
///
impl NodePool {
    ///
    /// Accepts pairs of <http endpoint, health_url> and health check interval
    ///
    pub fn new(
        urls: Vec<(String, String, String)>,
        health_check_interval: Option<Duration>,
        network: String,
        timeout: Option<Duration>,
    ) -> Option<Self> {
        if urls.is_empty() {
            return None;
        }

        let http_timeout = timeout.unwrap_or(Duration::from_secs(30));
        let client = ClientBuilder::new()
            .timeout(http_timeout)
            .build()
            .ok()?;

        let nodes: Vec<Node> = urls
            .into_iter()
            .map(|(name, url, health_url)| Node::new(name, url, health_url, network.clone()))
            .collect();

        Some(Self {
            nodes: Arc::new(RwLock::new(nodes)),
            client,
            health_check_interval: health_check_interval.unwrap_or(Duration::from_secs(10)),
            http_timeout,
        })
    }

    pub fn start_health_checks(&self) {
        let nodes = Arc::clone(&self.nodes);
        let client = self.client.clone();
        let interval = self.health_check_interval;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            let nodes = nodes.clone();
            loop {
                ticker.tick().await;

                let nodes_read = nodes.read().await;
                let mut tasks = vec![];
                for node in nodes_read.iter() {
                    let client = client.clone();
                    let node = node.clone();

                    tasks.push(tokio::spawn(
                        async move { node.check_health(&client).await },
                    ));
                }
                let results: Vec<_> = join_all(tasks).await.into_iter().collect();

                drop(nodes_read);
                let mut nodes_write = nodes.write().await;

                for (node, &ref is_healthy) in nodes_write.iter_mut().zip(results.iter()) {
                    match is_healthy {
                        Ok(is_healthy) => {
                            let was_healthy = node.healthy;
                            if *is_healthy {
                                // If healthy, reset consecutive failures
                                if !was_healthy {
                                    // Node recovered - log it
                                    warn!(
                                        "(NodePool) Node {} ({}) recovered and is now healthy (was unhealthy for {} consecutive failures)",
                                        node.name,
                                        node.url,
                                        node.consecutive_failures
                                    );
                                }
                                node.healthy = true;
                                node.consecutive_failures = 0;
                            } else {
                                // If unhealthy, increment the consecutive failures
                                if was_healthy {
                                    // Node just became unhealthy - log it
                                    warn!(
                                        "(NodePool) Node {} ({}) marked as UNHEALTHY (health check failed)",
                                        node.name,
                                        node.url
                                    );
                                }
                                node.healthy = false;
                                node.consecutive_failures += 1;
                            }
                        }
                        Err(e) => {
                            error!("(NodePool) Health check task failed: {}", e);
                        }
                    }
                }
            }
        });
    }

    /// Get with endpoint preference - prefers nodes that have successfully handled this endpoint pattern before
    /// This allows automatic learning: nodes that successfully return data for an endpoint are preferred for future calls
    /// When `tx.enabled = true` and calling tx_search, pass `Some("tx_search")` to prefer nodes that successfully returned tx data
    pub async fn get_with_endpoint_preference(
        &self,
        path: Path,
        endpoint_pattern: Option<&str>, // e.g., "tx_search" - if provided, prefer nodes that succeeded on this pattern
    ) -> Result<String, NodePoolErrors> {
        let endpoint_key = endpoint_pattern.map(|s| s.to_string());
        debug!("Making call to {} (endpoint preference: {:?})", path, endpoint_key);

        let mut rng = SmallRng::from_os_rng();
        // Adaptive retry count based on HTTP timeout:
        // - Default (30s): 5 retries = ~10s total (2s delay between retries)
        // - High timeout (90s): 3 retries = ~6s total (faster fallback to avoid long waits)
        // - Very high timeout (120s+): 2 retries = ~4s total (very fast fallback)
        let max_retries = if self.http_timeout.as_secs() >= 90 {
            3 // High timeout: fewer retries for faster fallback
        } else if self.http_timeout.as_secs() >= 60 {
            4 // Medium-high timeout: moderate retries
        } else {
            5 // Default timeout (30s): standard retries
        };

        for attempt in 0..max_retries {
            // Get node list and select one (with endpoint preference if applicable)
            let (node_url, node_name, node_network) = {
                let nodes = self.nodes.read().await;
                let healthy_nodes: Vec<&Node> = nodes.iter().filter(|e| e.healthy).collect();

                if healthy_nodes.is_empty() {
                    drop(nodes);
                    // Shorter delay between retries for faster fallback (1s instead of 2s)
            sleep(Duration::from_secs(1)).await;
                    continue;
                }

                // If we have an endpoint pattern, prefer nodes that have successfully handled it
                // But if preferred nodes fail after a few attempts, fall back to all healthy nodes
                let nodes_to_try: Vec<&Node> = if let Some(ref pattern) = endpoint_key {
                    let preferred: Vec<&Node> = healthy_nodes.iter()
                        .filter(|n| n.successful_endpoints.contains(pattern))
                        .copied()
                        .collect();

                    // Try preferred nodes first, then fall back to all nodes if they fail
                    // Fallback threshold: try preferred for first 40% of retries, then fall back to all nodes
                    // This ensures we prefer nodes with tx_search support, but don't get stuck if they're down
                    let preferred_attempts = (max_retries as f64 * 0.4).ceil() as u32;
                    if !preferred.is_empty() && attempt < preferred_attempts {
                        preferred
                    } else {
                        // Fall back to all healthy nodes if preferred failed or don't exist
                        healthy_nodes.clone()
                    }
                } else {
                    healthy_nodes.clone()
                };

                // Log unhealthy nodes on first attempt
                if attempt == 0 {
                    let unhealthy_nodes: Vec<_> = nodes.iter().filter(|e| !e.healthy).collect();
                    if !unhealthy_nodes.is_empty() {
                        let unhealthy_list: Vec<String> = unhealthy_nodes
                            .iter()
                            .map(|n| format!("{} ({}) - {} consecutive failures", n.name, n.url, n.consecutive_failures))
                            .collect();
                        debug!(
                            "(NodePool) {} unhealthy node(s) available: {}",
                            unhealthy_list.len(),
                            unhealthy_list.join(", ")
                        );
                    }
                }

                // Select a node and clone its info before dropping the lock
                if let Some(node) = nodes_to_try.choose(&mut rng) {
                    (node.url.clone(), node.name.clone(), node.network.clone())
                } else {
                    drop(nodes);
                    // Shorter delay between retries for faster fallback (1s instead of 2s)
            sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            debug_assert!(
                !node_url.ends_with('/'),
                "Node URL should not end with a slash"
            );
            debug_assert!(
                path.as_str().starts_with('/'),
                "Path should start with a slash"
            );
            let url = format!("{}{}", node_url, path.as_str());

            let response = self.client.get(&url).send().await;

            match response {
                Ok(res) => {
                    let status = res.status();
                    let status_str = status.as_u16().to_string();
                    EXPORTER_HTTP_REQUESTS
                        .with_label_values(&[&node_url, &status_str, &node_network])
                        .inc();

                    let text = res.text().await?;

                    // If we have an endpoint pattern and the call succeeded, mark this node as successful for this endpoint
                    if let Some(ref pattern) = endpoint_key {
                        if status == StatusCode::OK {
                            // Success - mark this node as successfully handling this endpoint pattern
                            let mut nodes_write = self.nodes.write().await;
                            if let Some(node_mut) = nodes_write.iter_mut().find(|n| n.url == node_url) {
                                if node_mut.successful_endpoints.insert(pattern.clone()) {
                                    debug!(
                                        "(NodePool) Node {} ({}) successfully handled endpoint pattern '{}'",
                                        node_name,
                                        node_url,
                                        pattern
                                    );
                                }
                            }
                            drop(nodes_write);
                        }
                    }

                    if status == StatusCode::OK {
                        return Ok(text);
                    } else {
                        warn!(
                            "(NodePool) {} Attempt {} failed: {} - No healthy response",
                            node_name,
                            attempt + 1,
                            url
                        );
                    }
                }
                Err(_) => {
                    EXPORTER_HTTP_REQUESTS
                        .with_label_values(&[&node_url, "error", &node_network])
                        .inc();
                    warn!(
                        "(NodePool) {} Attempt {} failed: {} - No HTTP response",
                        node_name,
                        attempt + 1,
                        url
                    );
                }
            }

            // Shorter delay between retries for faster fallback (1s instead of 2s)
            sleep(Duration::from_secs(1)).await;
        }

        // Final error logging
        let nodes = self.nodes.read().await;
        let unhealthy_list: Vec<String> = nodes
            .iter()
            .filter(|e| !e.healthy)
            .map(|n| format!("{} ({}) - {} consecutive failures", n.name, n.url, n.consecutive_failures))
            .collect();

        if !unhealthy_list.is_empty() {
            // We have unhealthy nodes - list them
            warn!(
                "(NodePool) No healthy nodes when calling {}. Unhealthy nodes: {}",
                path,
                unhealthy_list.join(", ")
            );
        } else {
            // All nodes are marked healthy but all retry attempts failed
            warn!(
                "(NodePool) No healthy nodes when calling {} (all {} node(s) marked healthy but all retry attempts failed)",
                path,
                nodes.len()
            );
        }
        Err(NodePoolErrors::NoHealthyNodes(path.to_string()))
    }

    pub async fn get(&self, path: Path) -> Result<String, NodePoolErrors> {
        self.get_with_endpoint_preference(path, None).await
    }

    pub async fn post<T: serde::Serialize>(
        &self,
        path: Path,
        body: T,
    ) -> Result<String, NodePoolErrors> {
        debug!("Making POST call to {}", path);

        let nodes = self.nodes.read().await;
        let healthy_nodes: Vec<_> = nodes.iter().filter(|e| e.healthy).collect();
        let unhealthy_nodes: Vec<_> = nodes.iter().filter(|e| !e.healthy).collect();

        // Log unhealthy nodes if there are any and this is the first attempt
        if !unhealthy_nodes.is_empty() {
            let unhealthy_list: Vec<String> = unhealthy_nodes
                .iter()
                .map(|n| format!("{} ({}) - {} consecutive failures", n.name, n.url, n.consecutive_failures))
                .collect();
            debug!(
                "(NodePool) {} unhealthy node(s) available: {}",
                unhealthy_list.len(),
                unhealthy_list.join(", ")
            );
        }

        let mut rng = SmallRng::from_os_rng();

        for attempt in 0..5 {
            if let Some(node) = healthy_nodes.choose(&mut rng) {
                // Defensive: ensure no double slash in URL construction
                debug_assert!(
                    !node.url.ends_with('/'),
                    "Node URL should not end with a slash"
                );
                debug_assert!(
                    path.as_str().starts_with('/'),
                    "Path should start with a slash"
                );
                let url = format!("{}{}", node.url, path.as_str());

                let response = self
                    .client
                    .post(&url)
                    .json(&body)
                    .send()
                    .await;

                match response {
                    Ok(res) => {
                        let status_str = res.status().as_u16().to_string();
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[&node.url, &status_str, &node.network])
                            .inc();

                        if res.status() == StatusCode::OK {
                            return Ok(res.text().await?);
                        } else {
                            warn!(
                                "(NodePool) {} Attempt {} failed: {} - No healthy response",
                                node.name,
                                attempt + 1,
                                url
                            );
                        }
                    }
                    Err(_) => {
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[&node.url, "error", &node.network])
                            .inc();
                        warn!(
                            "(NodePool) {} Attempt {} failed: {} - No HTTP response",
                            node.name,
                            attempt + 1,
                            url
                        );
                    }
                }
            }
            // Shorter delay between retries for faster fallback (1s instead of 2s)
            sleep(Duration::from_secs(1)).await;
        }

        // Log detailed information about unhealthy nodes
        let nodes = self.nodes.read().await;
        let unhealthy_list: Vec<String> = nodes
            .iter()
            .filter(|e| !e.healthy)
            .map(|n| format!("{} ({}) - {} consecutive failures", n.name, n.url, n.consecutive_failures))
            .collect();

        if !unhealthy_list.is_empty() {
            // We have unhealthy nodes - list them
            warn!(
                "(NodePool) No healthy nodes when calling {}. Unhealthy nodes: {}",
                path,
                unhealthy_list.join(", ")
            );
        } else {
            // All nodes are marked healthy but all retry attempts failed
            warn!(
                "(NodePool) No healthy nodes when calling {} (all {} node(s) marked healthy but all retry attempts failed)",
                path,
                nodes.len()
            );
        }
        Err(NodePoolErrors::NoHealthyNodes(path.to_string()))
    }
}
