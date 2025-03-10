use std::{collections::HashMap, sync::Arc};

use base64::{engine::general_purpose, Engine as _};
use cosmrs::tendermint::PublicKey;
use serde_json::from_str;
use tokio::sync::Mutex;
use tracing::{error, info};
use urlencoding::encode;

use crate::{
    blockchains::tendermint::{
        tendermint::Tendermint,
        types::{TendermintValidator, ValidatorsResponse},
    },
    core::blockchain::{BlockScrapper, BlockchainMetrics, BlockchainMonitor, NetworkScrapper},
};

use super::types::{MezoRESTResponse, MezoRESTValidator};

pub struct Mezo {
    base: Tendermint,
}

impl Mezo {
    pub fn new(base: Tendermint) -> Self {
        Self { base }
    }
}

impl BlockchainMonitor for Mezo {
    async fn start_monitoring(self) {
        let self_arc = Arc::new(Mutex::new(self));

        tokio::spawn(async move {
            loop {
                let mut this = self_arc.lock().await;

                if this.base.get_chain_id().await {
                    this.process_validators().await;
                    this.base.process_block_window().await;
                }
            }
        });
    }
}

impl NetworkScrapper for Mezo {
    type RpcValidator = TendermintValidator;
    type RestValidator = MezoRESTValidator;
    type Proposal = Option<()>;

    async fn get_rpc_validators(&self, path: &str) -> Vec<Self::RpcValidator> {
        info!("Fetching RPC validators");
        let mut validators: Vec<TendermintValidator> = Vec::new();

        let mut all_fetched = false;
        let mut page = 1;
        let mut fetched = 0;

        while !all_fetched {
            let res = match self
                .base
                .client
                .with_rpc()
                .get(&format!("{}?page={}", path, page))
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    error!("Error calling to REST validators endpoint: {}", e);
                    break;
                }
            };

            let fetched_validators: Vec<TendermintValidator> =
                match from_str::<ValidatorsResponse>(&res) {
                    Ok(res) => {
                        if let Some(res) = res.result {
                            if res.count.parse::<usize>().unwrap() + fetched
                                == res.total.parse::<usize>().unwrap()
                            {
                                all_fetched = true;
                            } else {
                                fetched += res.count.parse::<usize>().unwrap();
                                page += 1;
                            }

                            res.validators
                        } else {
                            error!("Result key not present at validators rpc endpoint response");
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error deserializing JSON: {}", e);
                        error!("Raw JSON: {}", res);
                        break;
                    }
                };

            validators.extend(fetched_validators);
        }
        validators
    }

    async fn get_rest_validators(&self, path: &str) -> Vec<Self::RestValidator> {
        info!("Fetching REST validators");

        let mut pagination_key: Option<String> = None;
        let mut validators: Vec<MezoRESTValidator> = Vec::new();

        loop {
            let mut url = path.to_string();
            if let Some(key) = &pagination_key {
                let encoded_key = encode(key);
                url = format!("{}?pagination.key={}", path, encoded_key);
            }

            let res = match self.base.client.with_rest().get(&url).await {
                Ok(res) => res,
                Err(e) => {
                    error!("Error calling to validators REST endpoint: {}", e);
                    break;
                }
            };

            let fetched_validators: Vec<MezoRESTValidator> =
                match from_str::<MezoRESTResponse>(&res) {
                    Ok(res) => {
                        if let Some(pagination) = res.pagination {
                            pagination_key = pagination.next_key
                        }
                        res.validators
                    }
                    Err(e) => {
                        error!("Error deserializing JSON: {}", e);
                        error!("Raw JSON: {}", res);
                        break;
                    }
                };

            validators.extend(fetched_validators);
            if pagination_key.is_none() {
                break;
            }
        }
        validators
    }

    async fn process_validators(&mut self) {
        let rest_validators = self.get_rest_validators("/mezo/poa/v1/validators").await;
        let rpc_validators = self.get_rpc_validators("/validators").await;

        let pub_keys: HashMap<String, (String, String, String)> = rpc_validators
            .into_iter()
            .filter_map(|validator| {
                let bytes = match general_purpose::STANDARD.decode(validator.pub_key.value) {
                    Ok(b) => b,
                    Err(_) => {
                        error!("Could not base64 decode validator pub key");
                        return None;
                    }
                };

                let pub_key = match PublicKey::from_raw_ed25519(&bytes) {
                    Some(pk) => pk,
                    None => {
                        error!("Could not transform base64 bytes into tendermint pub key");
                        return None;
                    }
                };
                let address = pub_key.to_bech32("mezovalconspub");

                Some((
                    address,
                    (
                        validator.address,
                        validator.voting_power,
                        validator.proposer_priority,
                    ),
                ))
            })
            .collect();

        for validator in rest_validators {
            let pub_key = &validator.cons_pub_key_bech32;
            let name = &validator.description.moniker;

            if let Some((validator_address, voting_power, proporser_priority)) =
                pub_keys.get(pub_key)
            {
                self.base
                    .validators
                    .insert(validator_address.to_string(), name.to_string());
                self.base.set_validator_voting_power(
                    name,
                    validator_address,
                    voting_power.parse::<i64>().unwrap(),
                );
                self.base.set_validator_proposer_priority(
                    name,
                    validator_address,
                    proporser_priority.parse::<i64>().unwrap(),
                );
            } else {
                info!("No matching address found for pub_key: {}", pub_key);
            }
        }
    }

    async fn get_proposals(&mut self, path: &str) -> Vec<Self::Proposal> {
        todo!()
    }

    async fn process_proposals(&mut self) {
        todo!()
    }
}
