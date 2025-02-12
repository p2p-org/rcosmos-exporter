use std::sync::Mutex;
use std::sync::Arc;
use std::error::Error as StdError;
use std::time::Duration;

use serde_json::from_str;
use reqwest::Client;
use crate::{
    MessageLog,
    config::Settings,
    tendermint::types::*,
    tendermint::manager::*,
    tendermint::metrics::TENDERMINT_EXPORTER_RPC_FAILURES,
};


pub static RPC_CLIENT: Mutex<Option<Arc<RPC>>> = Mutex::new(None);


#[derive(Debug)]
pub struct RPC {
    client: Client,
    pub chain_id: String,
    endpoint_manager: Arc<EndpointManager>,
}


pub async fn initialize_rpc_client() -> Result<(), String> {
    let manager = get_endpoint_manager().await;
    let rpc_client = match manager {
        Ok(endpoint_manager) => {
            match RPC::new(endpoint_manager).await {
                Ok(mut rpc) => {
                    MessageLog!("INFO", "RPC client created successfully");
    
                    match rpc.get_status().await {
                        Ok(chain_id) => {
                            MessageLog!("INFO", "Successfully retrieved chain_id: {}", chain_id);
                        }
                        Err(err) => {
                            let err_msg = format!("Failed to get status: {:?}", err);
                            MessageLog!("ERROR", "{}", err_msg);
                            return Err(err_msg);
                        }
                    }
    
                    Some(Arc::new(rpc))
                }
                Err(err) => {
                    let err_msg = format!("Failed to create RPC client: {:?}", err);
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

    *RPC_CLIENT.lock().unwrap() = rpc_client;
    Ok(())
}



impl RPC {
    pub async fn new(endpoint_manager: Arc<EndpointManager>) -> Result<Self, Box<dyn StdError + Send + Sync>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        
        let rpc = RPC {
            client,
            chain_id: "".to_string(),
            endpoint_manager,
        };
        Ok(rpc)
    }

    async fn choose_endpoint(&self, exclude_endpoint: Option<&str>) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut healthy_endpoints = self.endpoint_manager.get_endpoints(Some(EndpointType::Rpc), true).await;
        if healthy_endpoints.is_empty() {
            MessageLog!("ERROR", "No healthy endpoints available, non-stable using");
            healthy_endpoints = self.endpoint_manager.get_endpoints(Some(EndpointType::Rpc), false).await;
        }
        if let Some(exclude) = exclude_endpoint {
            healthy_endpoints.retain(|(url, _)| url != exclude);
        }    
        if healthy_endpoints.is_empty() {
            MessageLog!("DEBUG", "No endpoints available after exclusion");
            return Err(Box::new(EndpointError("No endpoints available.".to_string())));
        }
        let endpoint_index = rand::random::<usize>() % healthy_endpoints.len();
        let (endpoint_url, _endpoint_type) = &healthy_endpoints[endpoint_index];
    
        Ok(endpoint_url.clone())
    }

    pub async fn get_consensus_state(&self) -> Result<ConsensusStateResponse, Box<dyn std::error::Error + Send + Sync>>  {
        let mut exclude_endpoint: Option<String> = None;
    
        loop {
            let endpoint = match self.choose_endpoint(exclude_endpoint.as_deref()).await {
                Ok(endpoint) => endpoint,
                Err(err) => {
                    MessageLog!("ERROR", "Failed to choose an endpoint: {:?}", err);
                    return Err(err);
                }
            };
    
            let url = format!("{}/consensus_state", endpoint);
            match self.client.get(&url).send().await {
                Ok(response) => match response.json::<ConsensusStateResponse>().await {
                    Ok(consensus_state_response) => {
                        MessageLog!("INFO", "Get consensus state request successful");
                        return Ok(consensus_state_response);
                    }
                    Err(err) => {
                        MessageLog!(
                            "ERROR",
                            "Failed to parse JSON response from {}: {:?}, excluding this endpoint",
                            url,
                            err
                        );
                        exclude_endpoint = Some(endpoint);
                        continue;
                    }
                },
                Err(err) => {
                    MessageLog!(
                        "ERROR",
                        "Failed to fetch consensus state from {}: {:?}, excluding this endpoint",
                        url,
                        err
                    );
                    exclude_endpoint = Some(endpoint);
                    continue;
                }
            };
        }
    }

    pub async fn get_status(&mut self) -> Result<String, Box<dyn StdError + Send + Sync>> {
        let endpoints = self.endpoint_manager.get_endpoints(Some(EndpointType::Rpc), true).await;
        let mut last_error: Option<String> = None;

        for endpoint_structure in endpoints {
            let endpoint = endpoint_structure.0; 
            let url = format!("{}/status", endpoint);
            match self.client.get(&url).send().await {
                Ok(response) => match response.json::<TendermintStatusResponse>().await {
                    Ok(status_response) => {
                        MessageLog!("INFO", "Get status request successful from endpoint: {}", url);
                        self.chain_id = status_response.result.node_info.network.clone();
                        return Ok(status_response.result.node_info.network);
                    }
                    Err(err) => {
                        MessageLog!(
                            "ERROR",
                            "Failed to parse JSON response from {}: {:?}, excluding this endpoint",
                            url,
                            err
                        );
                        self.endpoint_manager.update_endpoint_health(&endpoint, EndpointType::Rpc, false).await;
                        TENDERMINT_EXPORTER_RPC_FAILURES.with_label_values(&[&endpoint]).inc();
                        last_error = Some(format!("Failed to parse JSON from {}: {:?}", url, err));
                    }
                },
                Err(err) => {
                    MessageLog!(
                        "ERROR",
                        "Failed to fetch status from {}: {:?}, excluding this endpoint",
                        url,
                        err
                    );
                    self.endpoint_manager.update_endpoint_health(&endpoint, EndpointType::Rpc, false).await;
                    TENDERMINT_EXPORTER_RPC_FAILURES.with_label_values(&[&endpoint]).inc();
                    last_error = Some(format!("Failed to fetch status from {}: {:?}", url, err));
                }
            }
        }

        if let Some(error) = last_error {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, error)));
        }

        Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "No valid status found")))
    }

    pub async fn get_block(&self, height: i64) -> Result<TendermintBlockResponse, RpcBlockErrorResponse> {
        let endpoint = match self.choose_endpoint(None).await {
            Ok(ep) => {
                MessageLog!("DEBUG", "Chosen endpoint: {}", ep);
                ep
            },
            Err(err) => {
                MessageLog!("ERROR", "Error choosing endpoint: {:?}", err);
                return Err(RpcBlockErrorResponse {
                    jsonrpc: "2.0".to_string(),
                    id: -1,
                    error: RpcError {
                        code: -1,
                        message: format!("Error choosing endpoint: {:?}", err),
                        data: None,
                    },
                });
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
                MessageLog!("DEBUG", "The response has been received, with status: {}", resp.status());
                resp
            }
            Err(err) => {
                MessageLog!("ERROR", "Couldn't get a response: {:?}", err);
                self.endpoint_manager.update_endpoint_health(&endpoint, EndpointType::Rpc, false).await;
                TENDERMINT_EXPORTER_RPC_FAILURES.with_label_values(&[&endpoint]).inc();
                return Err(RpcBlockErrorResponse {
                    jsonrpc: "2.0".to_string(),
                    id: -1,
                    error: RpcError {
                        code: -2,
                        message: format!("Couldn't get a response: {:?}", err),
                        data: None,
                    },
                });
            }
        };
        let response_text = match response.text().await {
            Ok(text) => text,
            Err(err) => {
                MessageLog!("ERROR", "Failed to read response text: {:?}", err);
                self.endpoint_manager.update_endpoint_health(&endpoint, EndpointType::Rpc, false).await;
                return Err(RpcBlockErrorResponse {
                    jsonrpc: "2.0".to_string(),
                    id: -1,
                    error: RpcError {
                        code: -3,
                        message: format!("Failed to read response text: {:?}", err),
                        data: None,
                    },
                });
            }
        };
        match from_str::<TendermintBlockResponse>(&response_text) {
            Ok(block_response) => {
                MessageLog!("DEBUG", "Block {} was fetched", height);
                self.endpoint_manager.update_endpoint_health(&endpoint, EndpointType::Rpc, true).await;
                Ok(block_response)
            }
            Err(parse_error) => {
                match from_str::<RpcBlockErrorResponse>(&response_text) {
                    Ok(error_response) => {
                        MessageLog!(
                            "ERROR",
                            "Received RPC error: code = {}, message = {}, data = {:?}",
                            error_response.error.code,
                            error_response.error.message,
                            error_response.error.data
                        );
                        if error_response.error.code != -32603
                        || !error_response
                            .error
                            .data
                            .as_deref()
                            .unwrap_or("")
                            .contains("must be less")
                        {
                            self.endpoint_manager.update_endpoint_health(&endpoint, EndpointType::Rpc, false).await;
                        }
                        Err(error_response)
                    }
                    Err(_) => {
                        MessageLog!("ERROR", "Error converting JSON to either block response or RPC error: {:?}", parse_error);
                        self.endpoint_manager.update_endpoint_health(&endpoint, EndpointType::Rpc, false).await;
                        TENDERMINT_EXPORTER_RPC_FAILURES.with_label_values(&[&endpoint]).inc();
                        Err(RpcBlockErrorResponse {
                            jsonrpc: "2.0".to_string(),
                            id: -1,
                            error: RpcError {
                                code: -4,
                                message: format!("Error converting JSON: {:?}", parse_error),
                                data: Some(response_text),
                            },
                        })
                    }
                }
            }
        }
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

    pub async fn get_validators_at_page(
        &self,
        page: i32,
    ) -> Result<ValidatorsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mut exclude_endpoint: Option<String> = None;
        loop {
            let endpoint = match self.choose_endpoint(exclude_endpoint.as_deref()).await {
                Ok(endpoint) => endpoint,
                Err(err) => {
                    MessageLog!("ERROR", "Failed to choose an endpoint: {:?}", err);
                    return Err(err);
                }
            };
    
            let url = format!("{}/validators?page={}&per_page=1", endpoint, page);
            match self.client.get(&url).send().await {
                Ok(response) => match response.json::<ValidatorsResponse>().await {
                    Ok(validator_response) => {
                        MessageLog!("DEBUG", "Get validators at page {} request successful", page);
                        return Ok(validator_response);
                    }
                    Err(err) => {
                        MessageLog!(
                            "ERROR",
                            "Failed to parse JSON response from {}: {:?}, excluding this endpoint",
                            url,
                            err
                        );
                        exclude_endpoint = Some(endpoint);
                        continue;
                    }
                },
                Err(err) => {
                    MessageLog!(
                        "ERROR",
                        "Failed to fetch validators at page {} from {}: {:?}, excluding this endpoint",
                        page,
                        url,
                        err
                    );
                    exclude_endpoint = Some(endpoint);
                    continue;
                }
            };
        }
    }
    
}