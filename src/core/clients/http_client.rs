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
struct Endpoint {
    url: String,
    health_url: Path,
    healthy: bool,
    consecutive_failures: usize,
    network: String,
}

impl Endpoint {
    fn new(url: String, health_url: String, network: String) -> Self {
        let url = if url.ends_with('/') {
            url.trim_end_matches('/').to_string()
        } else {
            url
        };
        Endpoint {
            url,
            health_url: Path::ensure_leading_slash(health_url),
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
                        "(HTTP Client) Health check failed for {} with status {}",
                        health_url, status
                    );
                    false
                }
            }
            Err(_) => {
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
        network: String,
    ) -> Option<Self> {
        let endpoints = urls
            .into_iter()
            .map(|(url, health_url)| Endpoint::new(url, health_url, network.clone()))
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

    pub async fn get(&self, path: impl Into<Path>) -> Result<String, HTTPClientErrors> {
        let path = Path::ensure_leading_slash(&path.into());
        debug!("Making call to {}", path);

        let endpoints = self.endpoints.read().await;
        let healthy_endpoints: Vec<_> = endpoints.iter().filter(|e| e.healthy).collect();

        let mut rng = SmallRng::from_os_rng();

        for attempt in 0..5 {
            if let Some(endpoint) = healthy_endpoints.choose(&mut rng) {
                debug_assert!(
                    !endpoint.url.ends_with('/'),
                    "Endpoint URL should not end with a slash"
                );
                debug_assert!(
                    path.as_str().starts_with('/'),
                    "Path should start with a slash"
                );
                let url = format!("{}{}", endpoint.url, path.as_str());

                let response = self.client.get(&url).send().await;

                match response {
                    Ok(res) => {
                        let status_str = res.status().as_u16().to_string();
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[&endpoint.url, &status_str, &endpoint.network])
                            .inc();

                        if res.status() == StatusCode::OK {
                            return Ok(res.text().await?);
                        } else {
                            warn!(
                                "(HTTP Client) Attempt {} failed for {}, using {}: No healthy response",
                                attempt + 1,
                                path,
                                url
                            );
                        }
                    }
                    Err(_) => {
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[&endpoint.url, "error", &endpoint.network])
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

    pub async fn post<T: serde::Serialize>(
        &self,
        path: Path,
        body: T,
    ) -> Result<String, HTTPClientErrors> {
        let path = Path::ensure_leading_slash(path);
        debug!("Making POST call to {}", path);

        let body_string = match serde_json::to_string(&body) {
            Ok(json) => json,
            Err(e) => {
                return Err(HTTPClientErrors::NoHealthyEndpoints(format!(
                    "JSON serialization error: {}",
                    e
                )))
            }
        };

        let endpoints = self.endpoints.read().await;
        let healthy_endpoints: Vec<_> = endpoints.iter().filter(|e| e.healthy).collect();

        let mut rng = SmallRng::from_os_rng();

        for attempt in 0..5 {
            if let Some(endpoint) = healthy_endpoints.choose(&mut rng) {
                // Defensive: ensure no double slash in URL construction
                debug_assert!(
                    !endpoint.url.ends_with('/'),
                    "Endpoint URL should not end with a slash"
                );
                debug_assert!(
                    path.as_str().starts_with('/'),
                    "Path should start with a slash"
                );

                let url = format!("{}{}", endpoint.url, path.as_str());

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
                            .with_label_values(&[&endpoint.url, &status_str, &endpoint.network])
                            .inc();

                        if res.status() == StatusCode::OK {
                            return Ok(res.text().await?);
                        } else {
                            warn!(
                                "(HTTP Client) Attempt {} failed for {}, using {}: No healthy response",
                                attempt + 1,
                                path,
                                url
                            );
                        }
                    }
                    Err(_) => {
                        EXPORTER_HTTP_REQUESTS
                            .with_label_values(&[&endpoint.url, "error", &endpoint.network])
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
}
