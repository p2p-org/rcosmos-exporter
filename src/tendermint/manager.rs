use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use reqwest::{ClientBuilder, StatusCode};
use serde::ser::StdError;
use std::sync::Mutex;
use tokio::{
    spawn,
    time::interval,
    sync::{
        RwLock
    }
};

use crate::{
    config,
    config::Settings,
    MessageLog,
    tendermint::metrics::TENDERMINT_EXPORTER_RPC_FAILURES,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EndpointType {
    Rpc,
    Rest,
}


#[derive(Debug)]
pub struct EndpointManager {
    config: Arc<config::Settings>,
    endpoints: Arc<RwLock<HashMap<String, (EndpointType, bool)>>>,
}

pub static ENDPOINT_MANAGER: Mutex<Option<Arc<EndpointManager>>> = Mutex::new(None);

pub async fn initialize_endpoint_manager() -> Result<Arc<EndpointManager>, Box<dyn StdError>> {
    let config = config::Settings::new()?;
    let endpoint_manager = Arc::new(EndpointManager::new(config.into()));
    
    let endpoint_manager_clone = endpoint_manager.clone();
    spawn(async move {
        endpoint_manager_clone.run_health_checks(Duration::from_secs(10)).await;
    });

    Ok(endpoint_manager)
}

pub async fn get_endpoint_manager() -> Result<Arc<EndpointManager>, Box<dyn StdError>> {
    let mut manager_lock = ENDPOINT_MANAGER.lock().unwrap();
    if let Some(manager) = manager_lock.clone() {
        Ok(manager)
    } else {
        match initialize_endpoint_manager().await {
            Ok(manager) => {
                *manager_lock = Some(manager.clone());
                Ok(manager)
            }
            Err(e) => Err(e),
        }
    }
}

impl EndpointManager {
    pub fn new(config: Arc<config::Settings>) -> Self {
        MessageLog!("DEBUG", "EndpointManager has been created");
        let mut initial_endpoints = HashMap::new();
        for endpoint in config.rpc_endpoints.split(',') {
            let url = endpoint.trim().to_string();
            if !url.is_empty() {
                initial_endpoints.insert(url, (EndpointType::Rpc, true));
            }
        }
        for endpoint in config.rest_endpoints.split(',') {
            let url = endpoint.trim().to_string();
            if !url.is_empty() {
                initial_endpoints.insert(url, (EndpointType::Rest, true));
            }
        }

        EndpointManager {
            config,
            endpoints: Arc::new(RwLock::new(initial_endpoints)),
        }
    }

    pub async fn is_endpoint_healthy(&self, url: &str) -> bool {
        let endpoints = self.endpoints.read().await;
        if let Some((_, is_healthy)) = endpoints.get(url) {
            *is_healthy
        } else {
            false
        }
    }

    pub async fn run_health_checks(&self, interval_duration: Duration) {
        let mut interval = interval(interval_duration);
        loop {
            interval.tick().await;
            let endpoints = self.get_endpoints(None, false).await;
            for (endpoint, endpoint_type) in endpoints.iter() {
                let is_healthy = self.check_health(endpoint, endpoint_type).await;
                if !is_healthy {
                    TENDERMINT_EXPORTER_RPC_FAILURES
                    .with_label_values(&[endpoint])
                    .inc();
                } else {
                    MessageLog!("DEBUG", "Updated health status for unhealthy endpoint, {:?}", endpoint);
                    self.update_endpoint_health(endpoint, endpoint_type.clone(), is_healthy).await;
                }
            }
        }
    }

    async fn check_health(&self, endpoint: &str, endpoint_type: &EndpointType) -> bool {
        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(3))
            .build()
            .expect("Failed to build HTTP client");

        let health_url = match endpoint_type {
            EndpointType::Rpc => format!("{}/health", endpoint),
            EndpointType::Rest => format!("{}/node_info", endpoint),
        };

        MessageLog!("INFO", "Checking health for endpoint: {}", endpoint);

        match client.get(&health_url).send().await {
            Ok(response) => {
                MessageLog!("DEBUG", "Get health response from {}", endpoint);
                response.status() == StatusCode::OK
            }
            Err(_) => {
                MessageLog!("ERROR", "Failed to get health response from {}", endpoint);
                false
            }
        }
    }

    pub async fn get_endpoints(
        &self,
        filter: Option<EndpointType>,
        healthy: bool
    ) -> Vec<(String, EndpointType)> {
        let endpoints = self.endpoints.read().await;
        endpoints
            .iter()
            .filter(|(_, (etype, health_status))| {
                filter.as_ref().map_or(true, |f| *etype == *f) && *health_status == healthy
            })
            .map(|(url, (etype, _))| (url.clone(), etype.clone()))
            .collect()
    }

    pub async fn update_endpoint_health(&self, endpoint: &str, endpoint_type: EndpointType, is_healthy: bool) {
        let mut endpoints = self.endpoints.write().await;
        endpoints.insert(endpoint.to_string(), (endpoint_type, is_healthy));
        MessageLog!(
            "DEBUG",
            "Updated endpoint: {:?} to health status: {}",
            endpoint,
            if is_healthy { "healthy" } else { "unhealthy" }
        );
    }

    pub fn get_config(&self) -> Arc<config::Settings> {
        MessageLog!("DEBUG", "Get config");
        Arc::clone(&self.config)
    }
}
