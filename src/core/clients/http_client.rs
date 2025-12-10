use futures::future::join_all;
use rand::rngs::SmallRng;
use rand::seq::IndexedRandom;
use rand::SeedableRng;
use reqwest::{Client, ClientBuilder, StatusCode};
use serde_json;
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
        let nodes = urls
            .into_iter()
            .map(|(name, url, health_url)| Node::new(name, url, health_url, network.clone()))
            .collect();

        // Timeout is now configurable via general.rpc_timeout_seconds in config.yaml
        // Default: 30 seconds (good for most chains)
        // Celestia: 90 seconds (for 60-70 MB blocks that take 35-40 seconds to download)
        let request_timeout = timeout.unwrap_or(Duration::from_secs(30));
        let client = ClientBuilder::new()
            .timeout(request_timeout)
            .connect_timeout(Duration::from_secs(10)) // Increased connection timeout
            .build()
            .unwrap();

        Some(NodePool {
            nodes: Arc::new(RwLock::new(nodes)),
            client,
            health_check_interval: health_check_interval.unwrap_or(Duration::from_secs(10)),
        })
    }

    pub fn start_health_checks(&self) {
        let nodes = self.nodes.clone();
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
                            if *is_healthy {
                                // If healthy, reset consecutive failures
                                node.healthy = true;
                                node.consecutive_failures = 0;
                            } else {
                                // If unhealthy, increment the consecutive failures
                                node.healthy = false;
                                node.consecutive_failures += 1;
                            }
                        }
                        Err(e) => {
                            error!("(NodePool) Health check task couldn't join: {:?}", e)
                        }
                    }
                }
            }
        });
    }

    pub async fn get(&self, path: Path) -> Result<String, NodePoolErrors> {
        debug!("Making call to {}", path);

        let nodes = self.nodes.read().await;
        let healthy_nodes: Vec<_> = nodes.iter().filter(|e| e.healthy).collect();

        let mut rng = SmallRng::from_os_rng();

        for attempt in 0..5 {
            if let Some(node) = healthy_nodes.choose(&mut rng) {
                debug_assert!(
                    !node.url.ends_with('/'),
                    "Node URL should not end with a slash"
                );
                debug_assert!(
                    path.as_str().starts_with('/'),
                    "Path should start with a slash"
                );
                let url = format!("{}{}", node.url, path.as_str());

                let response = self.client.get(&url).send().await;

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
            sleep(Duration::from_secs(2)).await;
        }

        warn!("(NodePool) No healthy nodes when calling {}", path);
        Err(NodePoolErrors::NoHealthyNodes(path.to_string()))
    }

    pub async fn post<T: serde::Serialize>(
        &self,
        path: Path,
        body: T,
    ) -> Result<String, NodePoolErrors> {
        let path = Path::from(path);
        debug!("Making POST call to {}", path);

        let body_string = match serde_json::to_string(&body) {
            Ok(json) => json,
            Err(e) => {
                return Err(NodePoolErrors::NoHealthyNodes(format!(
                    "JSON serialization error: {}",
                    e
                )))
            }
        };

        let nodes = self.nodes.read().await;
        let healthy_nodes: Vec<_> = nodes.iter().filter(|e| e.healthy).collect();

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

                debug!("Attempting POST request to: {}", url);

                let response = self
                    .client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .body(body_string.clone())
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
            sleep(Duration::from_secs(2)).await;
        }

        warn!("(NodePool) No healthy nodes when calling {}", path);
        Err(NodePoolErrors::NoHealthyNodes(path.to_string()))
    }
}
