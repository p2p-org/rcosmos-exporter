use std::{sync::Arc, time::Duration};

use async_trait::async_trait;

use serde_json::from_str;
use sha2::{Digest, Sha256};
use tokio::time::sleep;
use tracing::{error, info};
use urlencoding::encode;

use crate::{
    blockchains::{
        mezo::types::MezoRESTResponse,
        tendermint::{
            metrics::{
                TENDERMINT_VALIDATORS, TENDERMINT_VALIDATOR_PROPOSER_PRIORITY,
                TENDERMINT_VALIDATOR_VOTING_POWER,
            },
            types::{TendermintValidator, ValidatorsResponse},
        },
    },
    core::{chain_id::ChainId, clients::blockchain_client::BlockchainClient, exporter::Task},
};

use super::types::MezoRESTValidator;

pub struct MezoValidatorInfoScrapper {
    client: Arc<BlockchainClient>,
    chain_id: ChainId,
    network: String,
    validator_alert_addresses: Vec<String>,
}

impl MezoValidatorInfoScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        chain_id: ChainId,
        network: String,
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
        info!("(Mezo Validator Info) Fetching RPC validators");
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
                    error!(
                        "(Mezo Validator Info) Error calling to RPC validators endpoint: {}",
                        e
                    );
                    break;
                }
            };

            let fetched_validators: Vec<TendermintValidator> = match from_str::<ValidatorsResponse>(
                &res,
            ) {
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
                        error!("(Mezo Validator Info) Result key not present at validators rpc endpoint response");
                        break;
                    }
                }
                Err(e) => {
                    error!("(Mezo Validator Info) Error deserializing JSON: {}", e);
                    error!("(Mezo Validator Info) Raw JSON: {}", res);
                    break;
                }
            };

            validators.extend(fetched_validators);
        }
        validators
    }

    async fn get_rest_validators(&self, path: &str) -> Vec<MezoRESTValidator> {
        info!("(Mezo Validator Info) Fetching REST validators");

        let mut pagination_key: Option<String> = None;
        let mut validators: Vec<MezoRESTValidator> = Vec::new();

        loop {
            let mut url = path.to_string();
            if let Some(key) = &pagination_key {
                let encoded_key = encode(key);
                url = format!("{}?pagination.key={}", path, encoded_key);
            }

            let res = match self.client.with_rest().get(&url).await {
                Ok(res) => res,
                Err(e) => {
                    error!(
                        "(Mezo Validator Info) Error calling to REST validators endpoint: {:?}",
                        e
                    );
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
                        error!("(Mezo Validator Info) Error deserializing JSON: {}", e);
                        error!("(Mezo Validator Info) Raw JSON: {}", res);
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

        info!("(Mezo Validator Info) Processing REST validators");
        for validator in rest_validators {
            let (_, hash) = bech32::decode(&validator.cons_pub_key_bech32).unwrap();

            let mut hasher = Sha256::new();

            // Process the input data
            hasher.update(&hash[5..]);

            let hash = hasher.finalize();
            let hash = &hash[0..20];

            let address: String = hash.iter().map(|byte| format!("{:02x}", byte)).collect();
            let address = address.to_uppercase();

            let fires_alerts = self
                .validator_alert_addresses
                .contains(&address)
                .to_string();

            let name = &validator.description.moniker;

            TENDERMINT_VALIDATORS
                .with_label_values(&[
                    name,
                    &address,
                    &self.chain_id.to_string(),
                    &self.network,
                    &fires_alerts,
                ])
                .set(0);
        }

        let rpc_validators = self.get_rpc_validators("/validators").await;

        info!("(Mezo Validator Info) Processing RPC validators");
        for validator in rpc_validators {
            TENDERMINT_VALIDATOR_PROPOSER_PRIORITY
                .with_label_values(&[
                    &validator.address,
                    &self.chain_id.to_string(),
                    &self.network,
                ])
                .set(validator.proposer_priority.parse::<i64>().unwrap());

            TENDERMINT_VALIDATOR_VOTING_POWER
                .with_label_values(&[
                    &validator.address,
                    &self.chain_id.to_string(),
                    &self.network,
                ])
                .set(validator.voting_power.parse::<i64>().unwrap());
        }
    }
}

#[async_trait]
impl Task for MezoValidatorInfoScrapper {
    async fn run(&mut self, delay: Duration) {
        info!("Running Mezo Validator Info Scrapper");
        loop {
            self.process_validators().await;

            sleep(delay).await
        }
    }
}
