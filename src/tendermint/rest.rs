use std::sync::Mutex;
use std::sync::Arc;
use std::error::Error as StdError;
use std::time::Duration;

use reqwest::{Client, Error as ReqwestError};
use crate::{
    MessageLog,
    config::Settings,
    tendermint::types::*,
    tendermint::manager::*,
};


#[derive(Debug)]
pub struct REST {
    client: Client,
    endpoint_manager: Arc<EndpointManager>,
}


pub static REST_CLIENT: Mutex<Option<Arc<REST>>> = Mutex::new(None);


pub async fn initialize_rest_client() {
    let manager = get_endpoint_manager().await;
    let rest_client = match manager {
        Ok(endpoint_manager) => {
            match REST::new(endpoint_manager).await {
                Ok(rest) => Some(Arc::new(rest)),
                Err(err) => {
                    MessageLog!("ERROR", "Failed to create REST client: {:?}", err);
                    None
                }
            }
        }
        Err(err) => {
            MessageLog!("ERROR", "Failed to initialize EndpointManager: {:?}", err);
            None
        },
    };
    *REST_CLIENT.lock().unwrap() = rest_client;
}


impl REST {
    pub async fn new(endpoint_manager: Arc<EndpointManager>) -> Result<Self, Box<dyn StdError>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(REST { client, endpoint_manager })
    }

    async fn choose_endpoint(&self) -> Result<String, ReqwestError> {
        let mut healthy_endpoints = self.endpoint_manager.get_endpoints(Some(EndpointType::Rest), true).await;
        if healthy_endpoints.is_empty() {
            MessageLog!("ERROR", "No healthy endpoints available, non-stable");
            healthy_endpoints = self.endpoint_manager.get_endpoints(Some(EndpointType::Rest), false).await;
        }
        let endpoint_index = rand::random::<usize>() % healthy_endpoints.len();
        let (endpoint_url, _endpoint_type) = &healthy_endpoints[endpoint_index];
        
        Ok(endpoint_url.clone())
    }

    pub async fn get_active_validators(&self) -> Result<Vec<TendermintRESTValidator>, Box<dyn StdError + Send + Sync>> {
        let endpoint = self.choose_endpoint().await?;
        let url = format!("{}/cosmos/staking/v1beta1/validators", endpoint);
        let response = self.client.get(&url).send().await?;
        let rest_response: TendermintRESTResponse = response.json::<TendermintRESTResponse>().await?;
        let active_validators: Vec<TendermintRESTValidator> = rest_response
            .validators
            .into_iter()
            .filter(|validator| !validator.jailed)
            .collect();

        Ok(active_validators)
    }

}
