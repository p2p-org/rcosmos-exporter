use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use serde_json::from_str;
use sha2::{Digest, Sha256};
use tokio::time::sleep;
use tracing::{error, info, warn};
use urlencoding::encode;

use crate::{
    blockchains::tendermint::types::{
        TendermintRESTResponse, TendermintRESTValidator, TendermintValidator, ValidatorsResponse,
    },
    core::{
        chain_id::ChainId, clients::blockchain_client::BlockchainClient, exporter::Task,
        network::Network,
    },
};

use super::metrics::{
    TENDERMINT_VALIDATORS, TENDERMINT_VALIDATOR_JAILED, TENDERMINT_VALIDATOR_PROPOSER_PRIORITY,
    TENDERMINT_VALIDATOR_TOKENS, TENDERMINT_VALIDATOR_VOTING_POWER,
};

pub struct TendermintValidatorInfoScrapper {
    client: Arc<BlockchainClient>,
    chain_id: ChainId,
    network: Network,
    validator_alert_addresses: Vec<String>,
}

impl TendermintValidatorInfoScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        chain_id: ChainId,
        network: Network,
        validator_alert_addresses: Vec<String>,
    ) -> Self {
        Self {
            client,
            chain_id,
            network,
            validator_alert_addresses,
        }
    }

    async fn get_rpc_validators(&self, path: &str) -> Vec<TendermintValidator> {
        info!("(Tendermint Validator Info) Fetching RPC validators");
        let mut validators: Vec<TendermintValidator> = Vec::new();

        let mut all_fetched = false;
        let mut page = 1;
        let mut fetched = 0;

        while !all_fetched {
            let res = match self
                .client
                .with_rpc()
                .get(&format!("{}?page={}", path, page))
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    error!("Error calling to RPC validators endpoint: {}", e);
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

    async fn get_rest_validators(&self, path: &str) -> Vec<TendermintRESTValidator> {
        info!("(Tendermint Validator Info) Fetching REST validators");

        let mut pagination_key: Option<String> = None;
        let mut validators: Vec<TendermintRESTValidator> = Vec::new();

        loop {
            let mut url = path.to_string();
            if let Some(key) = &pagination_key {
                let encoded_key = encode(key);
                url = format!("{}?pagination.key={}", path, encoded_key);
            }

            let res = match self.client.with_rest().get(&url).await {
                Ok(res) => res,
                Err(e) => {
                    error!("Error calling to REST validators endpoint: {:?}", e);
                    break;
                }
            };

            let fetched_validators: Vec<TendermintRESTValidator> =
                match from_str::<TendermintRESTResponse>(&res) {
                    Ok(res) => {
                        pagination_key = res.pagination.next_key;
                        res.validators
                    }
                    Err(e) => {
                        error!(
                            "Error deserializing JSON from REST validator endpoint: {}",
                            e
                        );
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
        let rest_validators = self
            .get_rest_validators("/cosmos/staking/v1beta1/validators")
            .await;

        info!("(Tendermint Validator Info) Processing REST validators");
        for validator in rest_validators {
            let bytes = match general_purpose::STANDARD.decode(&validator.consensus_pubkey.key) {
                Ok(b) => b,
                Err(_) => {
                    warn!("Could not base64 decode validator pub key");
                    continue;
                }
            };

            let mut hasher = Sha256::new();
            // Process the input data
            hasher.update(bytes);
            let hash = hasher.finalize();
            let hash = &hash[..20];

            let address: String = hash.iter().map(|byte| format!("{:02x}", byte)).collect();
            let address = address.to_uppercase();

            let name = &validator.description.moniker;
            let tokens: f64 = validator.tokens.parse().unwrap_or(0.0);
            let jailed = validator.jailed;
            let fires_alerts = self
                .validator_alert_addresses
                .contains(&address)
                .to_string();

            TENDERMINT_VALIDATORS
                .with_label_values(&[
                    name,
                    &address,
                    &self.chain_id.to_string(),
                    &self.network.to_string(),
                    &fires_alerts,
                ])
                .set(0);
            TENDERMINT_VALIDATOR_TOKENS
                .with_label_values(&[
                    name,
                    &address,
                    &self.chain_id.to_string(),
                    &self.network.to_string(),
                ])
                .set(tokens);
            TENDERMINT_VALIDATOR_JAILED
                .with_label_values(&[
                    name,
                    &address,
                    &self.chain_id.to_string(),
                    &self.network.to_string(),
                    &fires_alerts,
                ])
                .set(if jailed { 1 } else { 0 });
        }

        let rpc_validators = self.get_rpc_validators("/validators").await;

        info!("(Tendermint Validator Info) Processing RPC validators");
        for validator in rpc_validators {
            TENDERMINT_VALIDATOR_PROPOSER_PRIORITY
                .with_label_values(&[
                    &validator.address,
                    &self.chain_id.to_string(),
                    &self.network.to_string(),
                ])
                .set(validator.proposer_priority.parse::<i64>().unwrap());

            TENDERMINT_VALIDATOR_VOTING_POWER
                .with_label_values(&[
                    &validator.address,
                    &self.chain_id.to_string(),
                    &self.network.to_string(),
                ])
                .set(validator.voting_power.parse::<i64>().unwrap());
        }
    }
}

#[async_trait]
impl Task for TendermintValidatorInfoScrapper {
    async fn run(&mut self, delay: Duration) {
        info!("Running Tendermint Validator Scrapper");
        loop {
            self.process_validators().await;

            sleep(delay).await
        }
    }
}
