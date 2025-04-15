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
use tracing::{debug, error, warn, info};

use crate::core::metrics::exporter_metrics::EXPORTER_HTTP_REQUESTS;

#[derive(Debug, Clone)]
struct Endpoint {
    url: String,
    health_url: String,
    healthy: bool,
    consecutive_failures: usize,
    last_checked: Option<std::time::SystemTime>,
}

impl Endpoint {
    fn new(url: String, health_url: String) -> Self {
        Endpoint {
            url: url.to_string(),
            health_url: health_url.to_string(),
            healthy: true,
            consecutive_failures: 0,
            last_checked: None,
        }
    }

    async fn check_health(self, client: &Client) -> bool {
        let health_url = format!("{}{}", self.url, self.health_url);

        match client.get(&health_url).send().await {
            Ok(response) if response.status() == StatusCode::OK => {
                EXPORTER_HTTP_REQUESTS
                    .with_label_values(&[
                        &self.url,
                        &self.health_url,
                        &response.status().as_u16().to_string(),
                    ])
                    .inc();

                true
            }
            Ok(response) => {
                warn!("(HTTP Client) Health check failed for {}", health_url);
                EXPORTER_HTTP_REQUESTS
                    .with_label_values(&[
                        &self.url,
                        &self.health_url,
                        &response.status().as_u16().to_string(),
                    ])
                    .inc();
                false
            }
            _ => {
                warn!("(HTTP Client) Health check failed for {}", health_url);
                false
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum HTTPClientErrors {
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("No healthy endpoints to call for: {0}")]
    NoHealthyEndpoints(String),
}

pub struct HttpClient {
    endpoints: Arc<RwLock<Vec<Endpoint>>>,
    client: Client,
    health_check_interval: Duration,
}

///
/// HttpClient does track the endpoints health
///
impl HttpClient {
    ///
    /// Accepts pairs of <http endpoint, health_url> and health check interval
    ///
    pub fn new(
        urls: Vec<(String, String)>,
        health_check_interval: Option<Duration>,
    ) -> Option<Self> {
        let endpoints = urls
            .into_iter()
            .map(|(url, health_url)| Endpoint::new(url, health_url))
            .collect();

        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(10)) // Set the default timeout
            .connect_timeout(Duration::from_secs(5)) // Set the connection timeout
            .build()
            .unwrap();

        Some(HttpClient {
            endpoints: Arc::new(RwLock::new(endpoints)),
            client,
            health_check_interval: health_check_interval.unwrap_or(Duration::from_secs(10)),
        })
    }

    pub fn start_health_checks(&self) {
        let endpoints = self.endpoints.clone();
        let client = self.client.clone();
        let interval = self.health_check_interval;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            let endpoints = endpoints.clone();
            loop {
                ticker.tick().await;

                let endpoints_read = endpoints.read().await;
                let mut tasks = vec![];
                for endpoint in endpoints_read.iter() {
                    let client = client.clone();
                    let endpoint = endpoint.clone();

                    tasks.push(tokio::spawn(
                        async move { endpoint.check_health(&client).await },
                    ));
                }
                let results: Vec<_> = join_all(tasks).await.into_iter().collect();

                drop(endpoints_read);
                let mut endpoints_write = endpoints.write().await;

                for (endpoint, &ref is_healthy) in endpoints_write.iter_mut().zip(results.iter()) {
                    match is_healthy {
                        Ok(is_healthy) => {
                            if *is_healthy {
                                // If healthy, reset consecutive failures
                                endpoint.healthy = true;
                                endpoint.consecutive_failures = 0;
                            } else {
                                // If unhealthy, increment the consecutive failures
                                endpoint.healthy = false;
                                endpoint.consecutive_failures += 1;
                            }
                        }
                        Err(e) => {
                            error!("(HTTP Client) Health check task couldn't join: {:?}", e)
                        }
                    }
                }
            }
        });
    }

    pub async fn get(&self, path: &str) -> Result<String, HTTPClientErrors> {
        debug!("Making call to {}", path);

        let endpoints = self.endpoints.read().await;
        let healthy_endpoints: Vec<_> = endpoints.iter().filter(|e| e.healthy).collect();

        let mut rng = SmallRng::from_os_rng();

        // Retry up to 5 times
        for attempt in 0..5 {
            if let Some(endpoint) = healthy_endpoints.choose(&mut rng) {
                let url = format!("{}/{}", endpoint.url, path);
                let metric_path: Vec<&str> = path.split("?").collect();

                let metric_path = if !metric_path.is_empty() {
                    metric_path[0]
                } else {
                    ""
                };

                let response = self.client.get(&url).send().await;

                match response {
                    Ok(res) if res.status() == StatusCode::OK => {
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[
                                &endpoint.url,
                                metric_path,
                                &res.status().as_u16().to_string(),
                            ])
                            .inc();
                        return Ok(res.text().await?);
                    }
                    Ok(res) => {
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[
                                &endpoint.url,
                                metric_path,
                                &res.status().as_u16().to_string(),
                            ])
                            .inc();
                        warn!(
                            "(HTTP Client) Attempt {} failed for {}, using {}: No healthy response",
                            attempt + 1,
                            path,
                            url
                        );
                    }
                    _ => {
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[&endpoint.url, path, "error"])
                            .inc();
                        warn!(
                            "(HTTP Client) Attempt {} failed for {}, using {}: No HTTP response",
                            attempt + 1,
                            path,
                            url
                        );
                    }
                }
            }
            sleep(Duration::from_secs(2)).await;
        }

        warn!("(HTTP Client) No healthy endpoints when calling {}", path);
        Err(HTTPClientErrors::NoHealthyEndpoints(path.to_string()))
    }

    pub async fn post(&self, path: &str, body: String) -> Result<String, HTTPClientErrors> {
        debug!("Making POST call to {}", path);

        let endpoints = self.endpoints.read().await;
        let healthy_endpoints: Vec<_> = endpoints.iter().filter(|e| e.healthy).collect();

        let mut rng = SmallRng::from_os_rng();

        // Retry up to 5 times
        for attempt in 0..5 {
            if let Some(endpoint) = healthy_endpoints.choose(&mut rng) {
                // Fix URL formatting to avoid double slashes
                let url = if path.is_empty() {
                    endpoint.url.clone()
                } else if path.starts_with("/") {
                    format!("{}{}", endpoint.url, path)
                } else {
                    format!("{}/{}", endpoint.url, path)
                };
                
                let metric_path = if path.contains("?") {
                    path.split("?").next().unwrap_or("")
                } else {
                    path
                };

                info!("Attempting POST request to: {}", url);
                
                let response = self.client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .body(body.clone())
                    .send()
                    .await;

                match response {
                    Ok(res) if res.status() == StatusCode::OK => {
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[
                                &endpoint.url,
                                metric_path,
                                &res.status().as_u16().to_string(),
                            ])
                            .inc();
                        return Ok(res.text().await?);
                    }
                    Ok(res) => {
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[
                                &endpoint.url,
                                metric_path,
                                &res.status().as_u16().to_string(),
                            ])
                            .inc();
                        warn!(
                            "(HTTP Client) Attempt {} failed for {}, using {}: No healthy response, status: {}",
                            attempt + 1,
                            path,
                            url,
                            res.status()
                        );
                    }
                    Err(e) => {
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[&endpoint.url, path, "error"])
                            .inc();
                        warn!(
                            "(HTTP Client) Attempt {} failed for {}, using {}: Error: {}",
                            attempt + 1,
                            path,
                            url,
                            e
                        );
                    }
                }
            }
            sleep(Duration::from_secs(2)).await;
        }

        warn!("(HTTP Client) No healthy endpoints when calling {}", path);
        Err(HTTPClientErrors::NoHealthyEndpoints(path.to_string()))
    }

    pub async fn print_endpoints(&self) {
        let endpoints = self.endpoints.read().await;
        if endpoints.is_empty() {
            info!("HTTP Client has no endpoints configured");
        } else {
            for (i, endpoint) in endpoints.iter().enumerate() {
                info!("HTTP Client endpoint {}: {}", i, endpoint.url);
                info!("  Healthy: {}", endpoint.healthy);
                info!("  Last checked: {:?}", endpoint.last_checked);
            }
        }
    }
}
