use std::sync::Arc;

use anyhow::{bail, Context};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use serde_json::from_str;
use sha2::{Digest, Sha256};
use tracing::info;
use urlencoding::encode;

use crate::{
    blockchains::tendermint::types::{
        TendermintRESTResponse, TendermintRESTValidator, TendermintValidator, ValidatorsResponse,
    },
    core::{chain_id::ChainId, clients::blockchain_client::BlockchainClient, exporter::Task},
};

use super::metrics::{
    TENDERMINT_VALIDATORS, TENDERMINT_VALIDATOR_JAILED, TENDERMINT_VALIDATOR_PROPOSER_PRIORITY,
    TENDERMINT_VALIDATOR_TOKENS, TENDERMINT_VALIDATOR_VOTING_POWER,
};

pub struct TendermintValidatorInfoScrapper {
    client: Arc<BlockchainClient>,
    chain_id: ChainId,
    network: String,
    validator_alert_addresses: Vec<String>,
}

impl TendermintValidatorInfoScrapper {
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

    async fn get_rpc_validators(&self, path: &str) -> anyhow::Result<Vec<TendermintValidator>> {
        info!("(Tendermint Validator Info) Fetching RPC validators");
        let mut validators: Vec<TendermintValidator> = Vec::new();

        let mut all_fetched = false;
        let mut page = 1;
        let mut fetched = 0;

        while !all_fetched {
            let res = self
                .client
                .with_rpc()
                .get(&format!("{}?page={}", path, page))
                .await
                .context(format!("Could not fetch active validators page: {}", page))?;

            let validators_response =
                from_str::<ValidatorsResponse>(&res).context("Could not decode JSON response")?;

            if let Some(res) = validators_response.result {
                let count = res.count.parse::<usize>().context("
                    Could not parse the count of obtained validators when fetching active validators"
                )?;
                let total = res.total.parse::<usize>().context(
                    "Could not parse the total of validators when fetching active validators",
                )?;
                if count + fetched == total {
                    all_fetched = true;
                } else {
                    fetched += count;
                    page += 1;
                }

                validators.extend(res.validators)
            } else {
                bail!("Result key not present at validators rpc endpoint response")
            };
        }
        Ok(validators)
    }

    async fn get_rest_validators(
        &self,
        path: &str,
    ) -> anyhow::Result<Vec<TendermintRESTValidator>> {
        info!("(Tendermint Validator Info) Fetching REST validators");

        let mut pagination_key: Option<String> = None;
        let mut validators: Vec<TendermintRESTValidator> = Vec::new();

        loop {
            let mut url = path.to_string();
            if let Some(key) = &pagination_key {
                let encoded_key = encode(key);
                url = format!("{}?pagination.key={}", path, encoded_key);
            }

            let res = self
                .client
                .with_rest()
                .get(&url)
                .await
                .context("Could not fetch rest validators")?;

            let rest_validator_response = from_str::<TendermintRESTResponse>(&res)
                .context("Could not deserialize REST validators response")?;

            pagination_key = rest_validator_response.pagination.next_key;

            validators.extend(rest_validator_response.validators);
            if pagination_key.is_none() {
                break;
            }
        }
        Ok(validators)
    }

    async fn process_validators(&mut self) -> anyhow::Result<()> {
        let rest_validators = self
            .get_rest_validators("/cosmos/staking/v1beta1/validators")
            .await
            .context("Could not obtain REST validators")?;

        info!("(Tendermint Validator Info) Processing REST validators");
        for validator in rest_validators {
            let bytes = general_purpose::STANDARD
                .decode(&validator.consensus_pubkey.key)
                .context("Could not validator pub key")?;

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

        let rpc_validators = self
            .get_rpc_validators("/validators")
            .await
            .context("Could not obtain RPC validators")?;

        info!("(Tendermint Validator Info) Processing RPC validators");
        for validator in rpc_validators {
            TENDERMINT_VALIDATOR_PROPOSER_PRIORITY
                .with_label_values(&[
                    &validator.address,
                    &self.chain_id.to_string(),
                    &self.network,
                ])
                .set(
                    validator
                        .proposer_priority
                        .parse::<i64>()
                        .context("Could not parse validator proposer priority")?,
                );

            TENDERMINT_VALIDATOR_VOTING_POWER
                .with_label_values(&[
                    &validator.address,
                    &self.chain_id.to_string(),
                    &self.network,
                ])
                .set(
                    validator
                        .voting_power
                        .parse::<i64>()
                        .context("Could not parse validator voting power")?,
                );
        }

        Ok(())
    }
}

#[async_trait]
impl Task for TendermintValidatorInfoScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_validators()
            .await
            .context("Failed to process validators")
    }

    fn name(&self) -> &'static str {
        "Tendermint Validator Info Scrapper"
    }
}
