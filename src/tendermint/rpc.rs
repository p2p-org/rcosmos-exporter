use std::sync::Mutex;
use std::sync::Arc;
use std::error::Error as StdError;
use std::time::Duration;

use reqwest::{Client, Error as ReqwestError};
use crate::{
    MessageLog,
    internal::logger::JsonLog,
    tendermint::types::*,
    tendermint::manager::*,
};


pub static RPC_CLIENT: Mutex<Option<Arc<RPC>>> = Mutex::new(None);


#[derive(Debug)]
pub struct RPC {
    client: Client,
    endpoint_manager: Arc<EndpointManager>,
}


pub async fn initialize_rpc_client() {
    let manager = get_endpoint_manager().await;
    let rpc_client = match manager {
        Ok(endpoint_manager) => {
            match RPC::new(endpoint_manager).await {
                Ok(rpc) => Some(Arc::new(rpc)),
                Err(err) => {
                    MessageLog!("Failed to create RPC client: {:?}", err);
                    None
                }
            }
        }
        Err(err) => {
            MessageLog!("Failed to initialize EndpointManager: {:?}", err);
            None
        },
    };
    *RPC_CLIENT.lock().unwrap() = rpc_client;
}


impl RPC {
    pub async fn new(endpoint_manager: Arc<EndpointManager>) -> Result<Self, Box<dyn StdError>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        Ok(RPC { client, endpoint_manager })
    }

    async fn choose_endpoint(&self) -> Result<String, ReqwestError> {
        let endpoints = self.endpoint_manager.get_endpoints(Some(EndpointType::Rpc)).await;
        if endpoints.is_empty() {
            MessageLog!("No healthy endpoints available");
        }
        let endpoint_index = rand::random::<usize>() % endpoints.len();
        let (endpoint_url, _endpoint_type) = &endpoints[endpoint_index];

        Ok(endpoint_url.clone())
    }

    pub async fn get_consensus_state(&self) -> Result<ConsensusStateResponse, ReqwestError> {
        let endpoint = self.choose_endpoint().await?;
        let url = format!("{}/consensus_state", endpoint);
        let response = self.client.get(&url).send().await?;
        let consensus_state_response = response.json::<ConsensusStateResponse>().await?;
        MessageLog!("Get consensus state request");
        Ok(consensus_state_response)
    }

    pub async fn get_status(&self) -> Result<TendermintStatusResponse, ReqwestError> {
        let endpoint = self.choose_endpoint().await?;
        let url = format!("{}/status", endpoint);
        let response = self.client.get(&url).send().await?;
        let status_response = response.json::<TendermintStatusResponse>().await?;
        MessageLog!("Get status request");
        Ok(status_response)
    }

    pub async fn get_block(&self, height: i64) -> Result<TendermintBlockResponse, ReqwestError> {
        let endpoint = match self.choose_endpoint().await {
            Ok(ep) => {
                MessageLog!("Chosen endpoint: {}", ep);
                ep
            },
            Err(err) => {
                MessageLog!("Error choosing endpoint: {}", err);
                return Err(err.into());
            }
        };
        let url = if height != 0 {
            format!("{}/block?height={}", endpoint, height)
        } else {
            format!("{}/block", endpoint)
        };
        let response_result = self.client.get(&url).send().await;
        let response = match response_result {
            Ok(resp) => {
                MessageLog!("Received response with status: {}", resp.status());
                resp
            },
            Err(err) => {
                MessageLog!("Error sending request: {}", err);
                return Err(err);
            }
        };
        let block_response_result = response.json::<TendermintBlockResponse>().await;
        let block_response = match block_response_result {
            Ok(res) => res,
            Err(err) => {
                MessageLog!("Error converting JSON: {}", err);
                return Err(err.into());
            }
        };
        MessageLog!("Get block request");
        Ok(block_response)
    }

    pub async fn get_validators(&self) -> Result<Vec<TendermintValidator>, Box<dyn StdError + Send + Sync>> {
        let mut page = 1;
        let mut validators = Vec::new();
        loop {
            let response = self.get_validators_at_page(page).await?;
            let total = response.result.as_ref().map_or(0, |result| {
                result.total.parse::<i64>().unwrap_or_default()
            });
            if let Some(result) = response.result {
                validators.extend(result.validators);
            } else {
                return Err("malformed response from node".into());
            }
            if validators.len() as i64 >= total {
                break;
            }
            page += 1;
        }
        Ok(validators)
    }

    // pub async fn get_validators(&self) -> Result<Vec<TendermintValidator>, Box<dyn StdError>> {
    //     let mut page = 1;
    //     let mut validators = Vec::new();
    //     loop {
    //         let response = self.get_validators_at_page(page).await?;
    //         let total = response.result.as_ref().map_or(0, |result| {
    //             result.total.parse::<i64>().unwrap_or_default()
    //         });
    //         if let Some(result) = response.result {
    //             validators.extend(result.validators);
    //         } else {
    //             return Err("malformed response from node".into());
    //         }
    //         if validators.len() as i64 >= total {
    //             break;
    //         }
    //         page += 1;
    //     }
    //     Ok(validators)
    // }

    pub async fn get_validators_at_page(
        &self,
        page: i32,
    ) -> Result<ValidatorsResponse, ReqwestError> {
        let endpoint = self.choose_endpoint().await?;
        let url = format!("{}/validators?page={}&per_page=1", endpoint, page);
        let response = self.client.get(&url).send().await?;
        let validator_response = response.json::<ValidatorsResponse>().await?;
        MessageLog!("Get validators at page request");
        Ok(validator_response)
    }
}