use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use serde_json::from_str;
use tracing::info;

use crate::{
    blockchains::tendermint::{
        metrics::{
            TENDERMINT_VALIDATORS, TENDERMINT_VALIDATOR_MISSED_BLOCKS,
            TENDERMINT_VALIDATOR_PROPOSER_PRIORITY, TENDERMINT_VALIDATOR_VOTING_POWER,
        },
        types::{TendermintBlockResponse, TendermintValidator, ValidatorsResponse},
    },
    core::{
        chain_id::ChainId,
        clients::{blockchain_client::BlockchainClient, path::Path},
        exporter::Task,
    },
};

pub struct CometBftValidatorInfoScrapper {
    client: Arc<BlockchainClient>,
    chain_id: ChainId,
    network: String,
    validator_alert_addresses: Vec<String>,
}

impl CometBftValidatorInfoScrapper {
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
        info!("(CometBFT Validator Info) Fetching RPC validators");
        let mut validators: Vec<TendermintValidator> = Vec::new();

        let mut all_fetched = false;
        let mut page = 1;

        while !all_fetched {
            let url = format!("{}?page={}", path, page);
            let res = self
                .client
                .with_rpc()
                .get(Path::from(url))
                .await
                .context(format!("Could not fetch active validators page: {}", page))?;

            let validators_response =
                from_str::<ValidatorsResponse>(&res).context("Could not decode JSON response")?;

            if let Some(res) = validators_response.result {
                let count = res.count.parse::<usize>().context(
                    "Could not parse the count of obtained validators when fetching active validators",
                )?;
                let total = res.total.parse::<usize>().context(
                    "Could not parse the total of validators when fetching active validators",
                )?;
                if count + validators.len() == total {
                    all_fetched = true;
                } else {
                    page += 1;
                }

                validators.extend(res.validators)
            } else {
                anyhow::bail!("Result key not present at validators rpc endpoint response")
            };
        }
        Ok(validators)
    }

    async fn get_latest_block(&self) -> anyhow::Result<(String, Vec<String>)> {
        let res = self
            .client
            .with_rpc()
            .get(Path::from("/block"))
            .await
            .context("Could not fetch latest block")?;

        let block_response: TendermintBlockResponse =
            from_str(&res).context("Could not parse block response as JSON")?;

        let height = block_response.result.block.header.height;

        // Get all validator addresses that signed the block (block_id_flag == 2 means committed)
        let signatures = block_response
            .result
            .block
            .last_commit
            .signatures
            .into_iter()
            .filter(|sig| sig.signature.is_some()) // Only consider signatures that are present
            .map(|sig| sig.validator_address)
            .collect();

        Ok((height, signatures))
    }

    async fn process_validators(&mut self) -> anyhow::Result<()> {
        let rpc_validators = self
            .get_rpc_validators("/validators")
            .await
            .context("Could not obtain RPC validators")?;

        // Get latest block to check for missed blocks
        let latest_block = self.get_latest_block().await?;
        let signed_validators: Vec<String> = latest_block.1;

        info!("(CometBFT Validator Info) Processing RPC validators");
        for validator in rpc_validators {
            let address = validator.address;
            let fires_alerts = self
                .validator_alert_addresses
                .contains(&address)
                .to_string();

            // Check if validator missed the block
            if !signed_validators.contains(&address) {
                info!(
                    "(CometBFT Validator Info) Validator {} missed block {}",
                    address, latest_block.0
                );
                TENDERMINT_VALIDATOR_MISSED_BLOCKS
                    .with_label_values(&[
                        &address,
                        &self.chain_id.to_string(),
                        &self.network,
                        &fires_alerts,
                    ])
                    .inc();
            }

            // For CometBFT, we'll use the address as both name and address since we don't have moniker
            TENDERMINT_VALIDATORS
                .with_label_values(&[
                    &address, // name
                    &address, // address
                    &self.chain_id.to_string(),
                    &self.network,
                    &fires_alerts,
                ])
                .set(0);

            TENDERMINT_VALIDATOR_VOTING_POWER
                .with_label_values(&[&address, &self.chain_id.to_string(), &self.network])
                .set(validator.voting_power.parse().unwrap_or(0));

            TENDERMINT_VALIDATOR_PROPOSER_PRIORITY
                .with_label_values(&[&address, &self.chain_id.to_string(), &self.network])
                .set(validator.proposer_priority.parse().unwrap_or(0));
        }

        Ok(())
    }
}

#[async_trait]
impl Task for CometBftValidatorInfoScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_validators().await
    }

    fn name(&self) -> &'static str {
        "CometBFT Validator Info Scrapper"
    }
}
