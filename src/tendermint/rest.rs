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


pub async fn initialize_rest_client() -> Result<(), String> {
    let manager = get_endpoint_manager().await;
    let rest_client = match manager {
        Ok(endpoint_manager) => {
            match REST::new(endpoint_manager).await {
                Ok(rest) => {
                    MessageLog!("INFO", "REST client created successfully");
                    Some(Arc::new(rest))
                }
                Err(err) => {
                    let err_msg = format!("Failed to create REST client: {:?}", err);
                    MessageLog!("ERROR", "{}", err_msg);
                    return Err(err_msg);
                }
            }
        }
        Err(err) => {
            let err_msg = format!("Failed to initialize EndpointManager: {:?}", err);
            MessageLog!("ERROR", "{}", err_msg);
            return Err(err_msg);
        }
    };

    *REST_CLIENT.lock().unwrap() = rest_client;
    Ok(())
}



impl REST {
    pub async fn new(endpoint_manager: Arc<EndpointManager>) -> Result<Self, Box<dyn StdError>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(REST { client, endpoint_manager })
    }

    async fn choose_endpoint(&self, exclude_endpoint: Option<&str>) -> Result<String, ReqwestError> {
        let mut healthy_endpoints = self.endpoint_manager.get_endpoints(Some(EndpointType::Rest), true).await;    
        if healthy_endpoints.is_empty() {
            MessageLog!("ERROR", "No healthy endpoints available, non-stable");
            healthy_endpoints = self.endpoint_manager.get_endpoints(Some(EndpointType::Rest), false).await;
        }
    
        if let Some(exclude) = exclude_endpoint {
            healthy_endpoints.retain(|(url, _)| url != exclude);
        }
        if healthy_endpoints.is_empty() {
            MessageLog!("ERROR", "No endpoints available after exclusion");
        }
        let endpoint_index = rand::random::<usize>() % healthy_endpoints.len();
        let (endpoint_url, _endpoint_type) = &healthy_endpoints[endpoint_index];    
        Ok(endpoint_url.clone())
    }

    pub async fn get_active_validators(&self) -> Result<Vec<TendermintRESTValidator>, Box<dyn StdError + Send + Sync>> {
        let mut active_validators = Vec::new();
        let mut pagination_key: Option<String> = None;
        let mut exclude_endpoint: Option<String> = None;
    
        loop {
            let endpoint = self.choose_endpoint(exclude_endpoint.as_deref()).await?;
            let mut url = format!("{}/cosmos/staking/v1beta1/validators", endpoint);
            if let Some(key) = &pagination_key {
                url = format!("{}?pagination.key={}", url, key);
            }
    
            let response = match self.client.get(&url).send().await {
                Ok(res) => res,
                Err(err) => {
                    MessageLog!(
                        "ERROR",
                        "Failed to fetch validators from {}: {:?}, trying next endpoint",
                        url,
                        err
                    );
                    exclude_endpoint = Some(endpoint);
                    continue;
                }
            };

            let rest_response: TendermintRESTResponse = response.json::<TendermintRESTResponse>().await?;
            let filtered_validators: Vec<TendermintRESTValidator> = rest_response
                .validators
                .into_iter()
                .filter(|validator| !validator.jailed)
                .collect();
    
            active_validators.extend(filtered_validators);
    
            if let Some(next_key) = rest_response.pagination.next_key {
                pagination_key = Some(next_key);
            } else {
                break;
            }
        }
    
        Ok(active_validators)
    }

    pub async fn get_proposals(&self) -> Result<Vec<Proposal>, Box<dyn StdError + Send + Sync>> {
        let mut proposals = Vec::new();
        let mut pagination_key: Option<String> = None;
        let mut exclude_endpoint: Option<String> = None;

        loop {
            let endpoint = self.choose_endpoint(exclude_endpoint.as_deref()).await?;
            let mut url = format!("{}/cosmos/gov/v1/proposals", endpoint);

            if let Some(key) = &pagination_key {
                url = format!("{}?pagination.key={}", url, key);
            }

            let response = match self.client.get(&url).send().await {
                Ok(res) => {
                    MessageLog!(
                        "DEBUG",
                        "Fetch proposals list chunk {:?}",
                        pagination_key.as_deref(),
                    );
                    res
                }
                Err(err) => {
                    MessageLog!(
                        "ERROR",
                        "Failed to fetch proposals from {}: {:?}, trying next endpoint",
                        url,
                        err
                    );
                    exclude_endpoint = Some(endpoint);
                    continue;
                }
            };
            let rest_response: TendermintProposalsResponse = response.json::<TendermintProposalsResponse>().await?;
            proposals.extend(rest_response.proposals);
            if let Some(next_key) = rest_response.pagination.next_key {
                pagination_key = Some(next_key);
            } else {
                break;
            }
        }
        Ok(proposals)
    }

}
