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
    tendermint::metrics::{
        TENDERMINT_EXPORTER_RPC_HEALTH_CHECK_REQUESTS,
        TENDERMINT_EXPORTER_RPC_HEALTH_CHECK_FAILURES
    }
};

#[derive(Debug, Clone, PartialEq)]
pub enum EndpointType {
    Rpc,
    Rest,
}


#[derive(Debug)]
pub struct EndpointManager {
    config: Arc<config::Settings>,
    healthy_rpc_endpoints: Arc<RwLock<Vec<String>>>,
    healthy_rest_endpoints: Arc<RwLock<Vec<String>>>,
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
        MessageLog!("DEBUG","EndpointManager has been created");
        EndpointManager {
            config,
            healthy_rpc_endpoints: Arc::new(RwLock::new(Vec::new())),
            healthy_rest_endpoints: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn run_health_checks(&self, interval_duration: Duration) {
        let mut interval = interval(interval_duration);
        loop {
            interval.tick().await;
            let mut new_rpc_endpoints = Vec::new();
            let mut new_rest_endpoints = Vec::new();
            let endpoints = self.get_endpoints(None).await;
            for (endpoint, endpoint_type) in endpoints.iter() {
                MessageLog!("DEBUG", "Checking health for endpoint: {}", endpoint);
                match endpoint_type {
                    EndpointType::Rpc => {
                        TENDERMINT_EXPORTER_RPC_HEALTH_CHECK_REQUESTS.inc();
                        if self.check_health(endpoint, endpoint_type).await {
                            new_rpc_endpoints.push(endpoint.clone());
                        } else {
                            TENDERMINT_EXPORTER_RPC_HEALTH_CHECK_FAILURES.inc();
                        }
                    }
                    EndpointType::Rest => {
                        // !NOTE, haven't found any health check for rest endpoints
                        // TENDERMINT_EXPORTER_REST_HEALTH_CHECK_REQUESTS.inc();
                        // if self.check_health(endpoint, endpoint_type).await {
                        //     new_rest_endpoints.push(endpoint.clone());
                        // } else {
                        //     TENDERMINT_EXPORTER_REST_HEALTH_CHECK_FAILURES.inc();
                        // }
                        new_rest_endpoints.push(endpoint.clone());
                    }
                }
            }
            *self.healthy_rpc_endpoints.write().await = new_rpc_endpoints;
            *self.healthy_rest_endpoints.write().await = new_rest_endpoints;

            MessageLog!("DEBUG", "Updated list of healthy RPC and REST endpoints");
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

        MessageLog!("INFO","Checking health for endpoint: {} with URL: {}", endpoint, health_url);

        match client.get(&health_url).send().await {
            Ok(response) => {
                MessageLog!("DEBUG","Get health response from {}", endpoint);
                response.status() == StatusCode::OK
            }
            Err(_) => {
                MessageLog!("ERROR", "Failed to get health response from {}", endpoint);
                false
            }
        }
    }

    pub async fn get_endpoints(&self, filter: Option<EndpointType>) -> Vec<(String, EndpointType)> {
        let mut endpoints: Vec<(String, EndpointType)> = Vec::new();
    
        let rpc_endpoints = self.config.rpc_endpoints.clone();
        for endpoint in rpc_endpoints.split(',') {
            let url = endpoint.trim().to_string();
            if !url.is_empty() {
                endpoints.push((url, EndpointType::Rpc));
            }
        }
    
        let rest_endpoints = self.config.rest_endpoints.clone();
        for endpoint in rest_endpoints.split(',') {
            let url = endpoint.trim().to_string();
            if !url.is_empty() {
                endpoints.push((url, EndpointType::Rest));
            }
        }
        if let Some(endpoint_type) = filter {
            endpoints.into_iter().filter(|(_, etype)| *etype == endpoint_type).collect()
        } else {
            endpoints
        }
    }

    pub fn get_config(&self) -> Arc<config::Settings> {
        MessageLog!("DEBUG", "Get config");
        Arc::clone(&self.config)
    }
}
