use reqwest::{Client, StatusCode};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::{task, time};
use tracing::{debug, warn};

#[derive(Debug, Clone)]
struct Endpoint {
    url: String,
    health_url: String,
    healthy: bool,
    consecutive_failures: usize,
    last_checked: Option<Instant>,
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

    async fn check_health(&mut self, client: &Client) {
        let health_url = format!("{}/{}", self.url, self.health_url);
        self.last_checked = Some(Instant::now());

        match client.get(&health_url).send().await {
            Ok(response) if response.status() == StatusCode::OK => {
                self.healthy = true;
                self.consecutive_failures = 0;
                debug!("Healt check success for {}", health_url);
            }
            _ => {
                self.healthy = false;
                self.consecutive_failures += 1;
                warn!("Health check failed for {}", health_url);
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
    endpoints: Arc<Mutex<Vec<Endpoint>>>,
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

        Some(HttpClient {
            endpoints: Arc::new(Mutex::new(endpoints)),
            client: Client::new(),
            health_check_interval: health_check_interval.unwrap_or(Duration::from_secs(3)),
        })
    }

    pub fn start_health_checks(&self) {
        let endpoints = self.endpoints.clone();
        let client = self.client.clone();
        let interval = self.health_check_interval;

        task::spawn(async move {
            let mut ticker = time::interval(interval);
            loop {
                ticker.tick().await;
                let mut endpoints = endpoints.lock().await;

                for endpoint in endpoints.iter_mut() {
                    endpoint.check_health(&client).await;
                }
            }
        });
    }

    pub async fn get(&self, path: &str) -> Result<String, HTTPClientErrors> {
        debug!("Making call to {}", path);

        let endpoints = self.endpoints.lock().await;
        let healthy_endpoints: Vec<_> = endpoints.iter().filter(|e| e.healthy).collect();

        if let Some(endpoint) = healthy_endpoints.first() {
            let url = format!("{}/{}", endpoint.url, path);
            let response = self.client.get(&url).send().await?;
            if response.status() == StatusCode::OK {
                return Ok(response.text().await?);
            }
        }

        warn!("No healthy endpoints when calling {}", path);
        Err(HTTPClientErrors::NoHealthyEndpoints(path.to_string()))
    }
}
