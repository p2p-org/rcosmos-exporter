use std::sync::Arc;
use std::time::Duration;
use reqwest::{ClientBuilder, StatusCode};
use tokio::time::interval;
use tokio::sync::RwLock;

use crate::{
    config,
    MessageLog,
    internal::logger::JsonLog,
    tendermint::metrics::{
        TENDERMINT_EXPORTER_HEALTH_CHECK_REQUESTS,
        TENDERMINT_EXPORTER_HEALTH_CHECK_FAILURES
    }
};

#[derive(Debug)]
pub struct EndpointManager {
    config: Arc<config::Settings>,
    healthy_endpoints: Arc<RwLock<Vec<String>>>,
}

impl EndpointManager {
    pub fn new(config: Arc<config::Settings>) -> Self {
        MessageLog!("EndpointManager has been created");
        EndpointManager {
            config,
            healthy_endpoints: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn run_health_checks(&self, interval_duration: Duration) {
        MessageLog!("Starting health checks...");

        let mut interval = interval(interval_duration);

        loop {
            interval.tick().await;
            TENDERMINT_EXPORTER_HEALTH_CHECK_REQUESTS.inc();

            let mut new_endpoints = Vec::new();

            let endpoints = self.get_endpoints().await;
            for endpoint in endpoints.iter() {
                MessageLog!("Checking health for endpoint: {}", endpoint);

                if self.check_health(endpoint).await {
                    new_endpoints.push(endpoint.clone());
                } else {
                    TENDERMINT_EXPORTER_HEALTH_CHECK_FAILURES.inc();
                }
            }

            *self.healthy_endpoints.write().await = new_endpoints;

            MessageLog!("Updated list of healthy endpoints");
        }
    }

    async fn check_health(&self, endpoint: &str) -> bool {
        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(3))
            .build()
            .expect("Failed to build HTTP client");

        let health_url = format!("{}/health", endpoint);
        MessageLog!("Checking health for endpoint: {}", endpoint);

        match client.get(&health_url).send().await {
            Ok(response) => {
                MessageLog!("Get health response from {}", endpoint);
                response.status() == StatusCode::OK
            }
            Err(_) => {
                MessageLog!("Failed to get health response from {}", endpoint);
                false
            }
        }
    }

    pub async fn get_endpoints(&self) -> Vec<String> {
        let config = self.config.rpc_endpoints.clone();
        let endpoints: Vec<String> = config.split(',').map(|s| s.trim().to_string()).collect();
        endpoints
    }

    pub fn get_config(&self) -> Arc<config::Settings> {
        MessageLog!("Get config");
        Arc::clone(&self.config)
    }
}
