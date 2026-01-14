use futures::future::join_all;
use rand::rngs::SmallRng;
use rand::seq::IndexedRandom;
use rand::SeedableRng;
use reqwest::{Client, ClientBuilder, StatusCode};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};


use super::path::Path;
use crate::core::metrics::exporter_metrics::EXPORTER_HTTP_REQUESTS;
use crate::core::utils::{extract_tx_index, ResponseStructure, detect_response_structure};

/// Construct a full URL from a node URL and path, ensuring proper formatting
/// Gracefully handles trailing slashes in node_url and ensures path starts with /
/// This is defensive programming - even though Node::new normalizes URLs, we handle edge cases here too
fn construct_url(node_url: &str, path: &Path) -> String {
    // Remove trailing slash from node_url if present (defensive - Node::new should have normalized this)
    let node_url_clean = node_url.trim_end_matches('/');
    // Path already ensures leading slash via Path::from(), but be defensive
    let path_str = path.as_str();
    let path_clean = if path_str.starts_with('/') {
        path_str.to_string()
    } else {
        // This shouldn't happen due to Path::from() normalization, but handle it gracefully
        format!("/{}", path_str)
    };
    format!("{}{}", node_url_clean, path_clean)
}

/// Format unhealthy nodes list for logging
fn format_unhealthy_nodes_list(nodes: &[&Node]) -> Vec<String> {
    nodes
        .iter()
        .map(|n| format!("{} ({}) - {} consecutive failures", n.name, n.url, n.consecutive_failures))
        .collect()
}

/// Log unhealthy nodes if any exist
fn log_unhealthy_nodes(nodes: &[&Node]) {
    if !nodes.is_empty() {
        let unhealthy_list = format_unhealthy_nodes_list(nodes);
        debug!(
            "(NodePool) {} unhealthy node(s) available: {}",
            unhealthy_list.len(),
            unhealthy_list.join(", ")
        );
    }
}

/// Check if an HTTP status code represents a transient (retryable) error
fn is_transient_error(status: StatusCode) -> bool {
    // 429 (Too Many Requests) - rate limiting, should retry
    // 500-599 (Server Errors) - transient server issues
    // 503 (Service Unavailable) - explicitly transient
    status == StatusCode::TOO_MANY_REQUESTS
        || (status.as_u16() >= 500 && status.as_u16() < 600)
}

/// Check if an HTTP status code represents a permanent (non-retryable) error
fn is_permanent_error(status: StatusCode) -> bool {
    // 400-499 (except 429) are typically client errors that won't be fixed by retrying
    status.as_u16() >= 400 && status.as_u16() < 500 && status != StatusCode::TOO_MANY_REQUESTS
}

/// Extract retry-after delay from response headers (for 429 rate limiting)
fn extract_retry_after(res: &reqwest::Response) -> Option<Duration> {
    res.headers()
        .get("retry-after")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Circuit breaker threshold: after this many consecutive failures, temporarily exclude node
const CIRCUIT_BREAKER_THRESHOLD: usize = 5;
/// Circuit breaker duration: how long to exclude a node after hitting threshold
const CIRCUIT_BREAKER_DURATION: Duration = Duration::from_secs(60);
/// When all nodes are unhealthy, wait this long before retrying (self-healing)
const ALL_NODES_UNHEALTHY_RETRY_DELAY: Duration = Duration::from_secs(5);

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
    // Circuit breaker: temporarily exclude node after too many failures (even if health check passes)
    circuit_breaker_until: Option<std::time::Instant>,
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
            circuit_breaker_until: None,
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
        })
    }

    pub fn start_health_checks(&self) {
        let nodes = Arc::clone(&self.nodes);
        let client = self.client.clone();
        let interval = self.health_check_interval;

        // Start periodic tx_index checks (every 5 health check cycles = ~50s with default 10s interval)
        // This proactively identifies nodes with tx_search support
        let nodes_tx_check = Arc::clone(&nodes);
        let client_tx_check = self.client.clone();
        tokio::spawn(async move {
            // Run initial check immediately on startup
            info!("(NodePool) Running initial tx_index detection check...");
            Self::check_tx_index_support(&nodes_tx_check, &client_tx_check).await;

            // Then run periodically
            let mut ticker = tokio::time::interval(interval * 5);
            loop {
                ticker.tick().await;
                Self::check_tx_index_support(&nodes_tx_check, &client_tx_check).await;
            }
        });

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

                let now = Instant::now();
                for (node, &ref is_healthy) in nodes_write.iter_mut().zip(results.iter()) {
                    match is_healthy {
                        Ok(is_healthy) => {
                            let was_healthy = node.healthy;
                            if *is_healthy {
                                // If healthy, reset consecutive failures and clear circuit breaker
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
                                // Clear circuit breaker if it's expired or node is healthy
                                if node.circuit_breaker_until.map(|until| now > until).unwrap_or(false) {
                                    node.circuit_breaker_until = None;
                                }
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
                                // Activate circuit breaker if threshold reached
                                if node.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
                                    node.circuit_breaker_until = Some(now + CIRCUIT_BREAKER_DURATION);
                                    warn!(
                                        "(NodePool) Node {} ({}) circuit breaker activated for {}s ({} consecutive health check failures)",
                                        node.name,
                                        node.url,
                                        CIRCUIT_BREAKER_DURATION.as_secs(),
                                        node.consecutive_failures
                                    );
                                }
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


    /// Check /status endpoint for tx_index support and update successful_endpoints accordingly
    /// This proactively identifies nodes with tx_search support without trial and error
    async fn check_tx_index_support(nodes: &Arc<RwLock<Vec<Node>>>, client: &Client) {
        let nodes_read = nodes.read().await;
        let mut tasks = vec![];

        for node in nodes_read.iter() {
            let client = client.clone();
            let node_url = node.url.clone();
            let node_name = node.name.clone();

            tasks.push(tokio::spawn(async move {
                let status_url = format!("{}/status", node_url);
                match client.get(&status_url).send().await {
                    Ok(res) if res.status() == StatusCode::OK => {
                        match res.json::<serde_json::Value>().await {
                            Ok(json) => {
                                // Use definitive detection: identify structure first, then extract tx_index
                                // This is more reliable than fallback-based approach
                                if extract_tx_index(&json).is_some() {
                                    let structure = detect_response_structure(&json);
                                    info!(
                                        "(NodePool) Detected {} response structure for node {} (tx_index enabled)",
                                        match structure {
                                            ResponseStructure::CometBft => "CometBFT",
                                            ResponseStructure::Sei => "Sei",
                                        },
                                        node_url
                                    );
                                    return Some((node_url, node_name));
                                }
                            }
                            Err(e) => {
                                debug!("(NodePool) Could not parse /status response for {}: {}", node_url, e);
                            }
                        }
                    }
                    Ok(_) => {
                        // Non-200 status, skip
                    }
                    Err(_) => {
                        // Request failed, skip
                    }
                }
                None
            }));
        }

        drop(nodes_read);
        let results: Vec<_> = join_all(tasks).await.into_iter().filter_map(|r| r.ok().flatten()).collect();

        if !results.is_empty() {
            let mut nodes_write = nodes.write().await;
            for (node_url, node_name) in results {
                if let Some(node) = nodes_write.iter_mut().find(|n| n.url == node_url) {
                    if node.successful_endpoints.insert("tx_search".to_string()) {
                        info!(
                            "(NodePool) Node {} ({}) has tx_index enabled (detected via /status endpoint)",
                            node_name,
                            node_url
                        );
                    }
                }
            }
        }
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
        // Minimal retries: try 1-2 different nodes, fail fast
        // This prevents wasting time on slow/failing nodes
        // If all nodes fail, the caller can retry the entire request
        const MAX_RETRIES: u32 = 2; // Try 2 different nodes max

        for attempt in 0..MAX_RETRIES {
            // Get node list and select one (with endpoint preference if applicable)
            let (node_url, node_name, node_network) = {
                let nodes = self.nodes.read().await;
                // Filter nodes: must be healthy AND not in circuit breaker
                let now = Instant::now();
                let healthy_nodes: Vec<&Node> = nodes
                    .iter()
                    .filter(|e| {
                        e.healthy
                            && e.circuit_breaker_until
                                .map(|until| now > until)
                                .unwrap_or(true)
                    })
                    .collect();

                if healthy_nodes.is_empty() {
                    drop(nodes);
                    // No healthy nodes - wait briefly and retry once (self-healing)
                    // This handles temporary network issues or brief node outages
                    if attempt == 0 {
                        debug!(
                            "(NodePool) No healthy nodes available for {}. Waiting {}s before retry (self-healing attempt)...",
                            path,
                            ALL_NODES_UNHEALTHY_RETRY_DELAY.as_secs()
                        );
                        tokio::time::sleep(ALL_NODES_UNHEALTHY_RETRY_DELAY).await;
                        continue; // Retry once after delay
                    } else {
                        // Already retried once - return error
                        return Err(NodePoolErrors::NoHealthyNodes(format!(
                            "No healthy nodes when calling {} (waited {}s)",
                            path,
                            ALL_NODES_UNHEALTHY_RETRY_DELAY.as_secs()
                        )));
                    }
                }

                // If we have an endpoint pattern, prefer nodes that have successfully handled it
                // On first attempt, try preferred nodes; on retry, try all healthy nodes
                let nodes_to_try: Vec<&Node> = if let Some(ref pattern) = endpoint_key {
                    let preferred: Vec<&Node> = healthy_nodes.iter()
                        .filter(|n| n.successful_endpoints.contains(pattern))
                        .copied()
                        .collect();

                    // Try preferred nodes on first attempt, fall back to all nodes on retry
                    if !preferred.is_empty() && attempt == 0 {
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
                    log_unhealthy_nodes(&unhealthy_nodes);
                }

                // Select a node and clone its info before dropping the lock
                if let Some(node) = nodes_to_try.choose(&mut rng) {
                    (node.url.clone(), node.name.clone(), node.network.clone())
                } else {
                    drop(nodes);
                    // No nodes to try - return error immediately
                    return Err(NodePoolErrors::NoHealthyNodes(format!(
                        "No healthy nodes when calling {}",
                        path
                    )));
                }
            };

            let url = construct_url(&node_url, &path);

                let response = self.client.get(&url).send().await;

                match response {
                    Ok(res) => {
                    let status = res.status();
                    let status_str = status.as_u16().to_string();
                        EXPORTER_HTTP_REQUESTS
                        .with_label_values(&[&node_url, &status_str, &node_network])
                            .inc();

                    // Handle rate limiting (429) with retry-after support
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let retry_after = extract_retry_after(&res).unwrap_or(Duration::from_secs(1));
                        warn!(
                            "(NodePool) {} Rate limited (429) for {}. Retry after {}s",
                            node_name,
                            url,
                            retry_after.as_secs()
                        );
                        // Don't mark as failure for rate limiting - it's temporary
                        // Wait for retry-after period, then try next node
                        tokio::time::sleep(retry_after).await;
                        continue; // Try next node after rate limit delay
                    }

                    // Handle transient server errors (5xx) - retry on next node
                    if is_transient_error(status) {
                        warn!(
                            "(NodePool) {} Transient error ({}): {}. Will retry on next node",
                            node_name,
                            status.as_u16(),
                            url
                        );
                        // Update node failure count for circuit breaker
                        let mut nodes_write = self.nodes.write().await;
                        if let Some(node_mut) = nodes_write.iter_mut().find(|n| n.url == node_url) {
                            node_mut.consecutive_failures += 1;
                            // Activate circuit breaker if threshold reached
                            if node_mut.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
                                node_mut.circuit_breaker_until = Some(Instant::now() + CIRCUIT_BREAKER_DURATION);
                                warn!(
                                    "(NodePool) Node {} ({}) circuit breaker activated for {}s ({} consecutive failures)",
                                    node_name,
                                    node_url,
                                    CIRCUIT_BREAKER_DURATION.as_secs(),
                                    node_mut.consecutive_failures
                                );
                            }
                        }
                        drop(nodes_write);
                        continue; // Try next node
                    }

                    // Handle permanent errors (4xx except 429) - don't retry this node
                    if is_permanent_error(status) {
                        warn!(
                            "(NodePool) {} Permanent error ({}): {}. Skipping this node",
                            node_name,
                            status.as_u16(),
                            url
                        );
                        continue; // Try next node, but don't increment failure count (it's a client error)
                    }

                    let text = res.text().await?;

                    // Success - reset failure count and mark endpoint as successful
                    {
                        let mut nodes_write = self.nodes.write().await;
                        if let Some(node_mut) = nodes_write.iter_mut().find(|n| n.url == node_url) {
                            // Reset failure count on success
                            if node_mut.consecutive_failures > 0 {
                                debug!(
                                    "(NodePool) Node {} ({}) recovered from {} consecutive failures",
                                    node_name,
                                    node_url,
                                    node_mut.consecutive_failures
                                );
                                node_mut.consecutive_failures = 0;
                                node_mut.circuit_breaker_until = None; // Clear circuit breaker
                            }

                            // If we have an endpoint pattern, mark this node as successful for this endpoint
                            if let Some(ref pattern) = endpoint_key {
                                if node_mut.successful_endpoints.insert(pattern.clone()) {
                                    debug!(
                                        "(NodePool) Node {} ({}) successfully handled endpoint pattern '{}'",
                                        node_name,
                                        node_url,
                                        pattern
                                    );
                                }
                            }
                        }
                    }

                    if status == StatusCode::OK {
                        return Ok(text);
                    } else {
                        // Unexpected status (shouldn't reach here, but handle gracefully)
                        warn!(
                            "(NodePool) {} Unexpected status {}: {}",
                            node_name,
                            status.as_u16(),
                            url
                        );
                    }
                    }
                    Err(e) => {
                        // Network/connection errors are transient - retry on next node
                        EXPORTER_HTTP_REQUESTS
                        .with_label_values(&[&node_url, "error", &node_network])
                            .inc();
                        warn!(
                            "(NodePool) {} Network error for {}: {}. Will retry on next node",
                        node_name,
                            url,
                            e
                        );
                        // Update failure count for circuit breaker
                        let mut nodes_write = self.nodes.write().await;
                        if let Some(node_mut) = nodes_write.iter_mut().find(|n| n.url == node_url) {
                            node_mut.consecutive_failures += 1;
                            if node_mut.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
                                node_mut.circuit_breaker_until = Some(Instant::now() + CIRCUIT_BREAKER_DURATION);
                                warn!(
                                    "(NodePool) Node {} ({}) circuit breaker activated for {}s ({} consecutive failures)",
                                    node_name,
                                    node_url,
                                    CIRCUIT_BREAKER_DURATION.as_secs(),
                                    node_mut.consecutive_failures
                                );
                            }
                        }
                        drop(nodes_write);
                        continue; // Try next node
                }
            }

            // No delay between retries - fail fast to try next node immediately
            // Only continue loop if we have more retries left
        }

        // Final error logging
        let nodes = self.nodes.read().await;
        let unhealthy_nodes: Vec<_> = nodes.iter().filter(|e| !e.healthy).collect();
        let unhealthy_list = format_unhealthy_nodes_list(&unhealthy_nodes);

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
        let now = Instant::now();
        // Filter nodes: must be healthy AND not in circuit breaker
        let healthy_nodes: Vec<_> = nodes
            .iter()
            .filter(|e| {
                e.healthy
                    && e.circuit_breaker_until
                        .map(|until| now > until)
                        .unwrap_or(true)
            })
            .collect();
        let unhealthy_nodes: Vec<_> = nodes.iter().filter(|e| !e.healthy).collect();

        // Log unhealthy nodes if there are any
        log_unhealthy_nodes(&unhealthy_nodes);

        // If no healthy nodes, wait briefly and retry (self-healing)
        if healthy_nodes.is_empty() {
            drop(nodes);
                        debug!(
                "(NodePool) No healthy nodes available for POST {}. Waiting {}s before retry (self-healing attempt)...",
                path,
                ALL_NODES_UNHEALTHY_RETRY_DELAY.as_secs()
            );
            tokio::time::sleep(ALL_NODES_UNHEALTHY_RETRY_DELAY).await;
            // Re-read nodes after delay
            let nodes = self.nodes.read().await;
            let now = Instant::now();
            let healthy_nodes: Vec<_> = nodes
                .iter()
                .filter(|e| {
                    e.healthy
                        && e.circuit_breaker_until
                            .map(|until| now > until)
                            .unwrap_or(true)
                })
                .collect();
            if healthy_nodes.is_empty() {
                return Err(NodePoolErrors::NoHealthyNodes(format!(
                    "No healthy nodes when calling {} (waited {}s)",
                    path,
                    ALL_NODES_UNHEALTHY_RETRY_DELAY.as_secs()
                )));
            }
        }

        let mut rng = SmallRng::from_os_rng();
        // Use same retry count as get_with_endpoint_preference for consistency
        const MAX_RETRIES: u32 = 2;

        for attempt in 0..MAX_RETRIES {
            // Re-read healthy nodes each attempt (they may have changed)
            let nodes = self.nodes.read().await;
            let now = Instant::now();
            let healthy_nodes: Vec<_> = nodes
                .iter()
                .filter(|e| {
                    e.healthy
                        && e.circuit_breaker_until
                            .map(|until| now > until)
                            .unwrap_or(true)
                })
                .collect();

            if let Some(node) = healthy_nodes.choose(&mut rng) {
                let url = construct_url(&node.url, &path);

                let response = self
                    .client
                    .post(&url)
                    .json(&body)
                    .send()
                    .await;

                match response {
                    Ok(res) => {
                        let status = res.status();
                        let status_str = status.as_u16().to_string();
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[&node.url, &status_str, &node.network])
                            .inc();

                        // Handle rate limiting (429)
                        if status == StatusCode::TOO_MANY_REQUESTS {
                            let retry_after = extract_retry_after(&res).unwrap_or(Duration::from_secs(1));
                        warn!(
                                "(NodePool) {} Rate limited (429) for POST {}. Retry after {}s",
                                node.name,
                                url,
                                retry_after.as_secs()
                            );
                            tokio::time::sleep(retry_after).await;
                            continue;
                        }

                        // Handle transient errors
                        if is_transient_error(status) {
                            warn!(
                                "(NodePool) {} Transient error ({}) for POST {}: {}",
                                node.name,
                                status.as_u16(),
                                url,
                                path
                            );
                            // Update failure count
                            let mut nodes_write = self.nodes.write().await;
                            if let Some(node_mut) = nodes_write.iter_mut().find(|n| n.url == node.url) {
                                node_mut.consecutive_failures += 1;
                                if node_mut.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
                                    node_mut.circuit_breaker_until = Some(Instant::now() + CIRCUIT_BREAKER_DURATION);
                                }
                            }
                            continue;
                        }

                        if status == StatusCode::OK {
                            // Success - reset failure count
                            let mut nodes_write = self.nodes.write().await;
                            if let Some(node_mut) = nodes_write.iter_mut().find(|n| n.url == node.url) {
                                if node_mut.consecutive_failures > 0 {
                                    node_mut.consecutive_failures = 0;
                                    node_mut.circuit_breaker_until = None;
                                }
                            }
                            return Ok(res.text().await?);
                        } else {
                        warn!(
                                "(NodePool) {} Attempt {} failed: {} - Status {}",
                                node.name,
                                attempt + 1,
                                url,
                                status.as_u16()
                            );
                        }
                    }
                    Err(e) => {
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[&node.url, "error", &node.network])
                            .inc();
                        warn!(
                            "(NodePool) {} Network error for POST {}: {}",
                            node.name,
                            url,
                            e
                        );
                        // Update failure count
                        let mut nodes_write = self.nodes.write().await;
                        if let Some(node_mut) = nodes_write.iter_mut().find(|n| n.url == node.url) {
                            node_mut.consecutive_failures += 1;
                            if node_mut.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
                                node_mut.circuit_breaker_until = Some(Instant::now() + CIRCUIT_BREAKER_DURATION);
                            }
                        }
                        continue;
                    }
                }
            }
            // No delay between retries - fail fast to try next node immediately
        }

        // Log detailed information about unhealthy nodes
        let nodes = self.nodes.read().await;
        let unhealthy_nodes: Vec<_> = nodes.iter().filter(|e| !e.healthy).collect();
        let unhealthy_list = format_unhealthy_nodes_list(&unhealthy_nodes);

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
