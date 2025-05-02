use std::sync::Arc;

use anyhow::{bail, Context};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use serde_json::from_str;

use tracing::info;

use crate::{
    blockchains::{
        babylon::types::{CurrentEpoch, GetEpochResponse},
        tendermint::types::{TendermintValidator, ValidatorsResponse},
    },
    core::{chain_id::ChainId, clients::blockchain_client::BlockchainClient, exporter::Task},
};

use super::{
    metrics::{BABYLON_CURRENT_EPOCH, BABYLON_VALIDATOR_MISSING_BLS_VOTE},
    types::{BlockTxs, Epoch, Tx},
};

pub struct BabylonBlsScrapper {
    client: Arc<BlockchainClient>,
    processed_epoch: usize,
    chain_id: ChainId,
    network: String,
    validator_alert_addresses: Vec<String>,
}

impl BabylonBlsScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        chain_id: ChainId,
        network: String,
        validator_alert_addresses: Vec<String>,
    ) -> Self {
        Self {
            client,
            processed_epoch: 0,
            chain_id,
            network,
            validator_alert_addresses,
        }
    }

    async fn get_rpc_validators(&self, path: &str) -> anyhow::Result<Vec<TendermintValidator>> {
        info!("(Babylon BLS Scrapper) Fetching RPC validators");
        let mut validators: Vec<TendermintValidator> = Vec::new();

        let mut all_fetched = false;
        let mut page = 1;
        let mut fetched = 0;

        while !all_fetched {
            let res = self
                .client
                .with_rpc()
                .get(&format!("{}&page={}", path, page))
                .await
                .context(format!(
                    "Could not fetch active validators page: {}, path: {}",
                    page, path
                ))?;

            let validators_response = from_str::<ValidatorsResponse>(&res)?;

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

    async fn get_current_epoch(&self) -> anyhow::Result<usize> {
        info!("(Babylon BLS Scrapper) Getting current epoch");
        let res = self
            .client
            .with_rest()
            .get("/babylon/epoching/v1/current_epoch")
            .await
            .context("Could not fetch current epoch")?;

        from_str::<CurrentEpoch>(&res)
            .context("Could not deserialize current epoch")?
            .current_epoch
            .parse::<usize>()
            .context("Could not parse current epoch")
    }

    async fn get_epoch(&self, epoch: usize) -> anyhow::Result<Epoch> {
        info!("(Babylon BLS Scrapper) Getting epoch {}", epoch);
        let res = self
            .client
            .with_rest()
            .get(&format!("/babylon/epoching/v1/epochs/{}", epoch))
            .await
            .context(format!("Could not fetch epoch: {}", epoch))?;

        Ok(from_str::<GetEpochResponse>(&res)
            .context("Could not deserialize epoch response")?
            .epoch)
    }

    async fn get_block_txs(&self, block: usize) -> anyhow::Result<Vec<Tx>> {
        info!("(Babylon BLS Scrapper) Getting block {} txs", block);
        let res = self
            .client
            .with_rest()
            .get(&format!(
                "/cosmos/tx/v1beta1/txs/block/{}?pagination.limit=1",
                block
            ))
            .await
            .context(format!("Could not fetch block txs for block: {}", block))?;

        Ok(from_str::<BlockTxs>(&res)
            .context("Could not deserialize block txs")?
            .txs)
    }

    async fn process_bls(&mut self) -> anyhow::Result<()> {
        let current_epoch = self
            .get_current_epoch()
            .await
            .context("Could not obtain current epoch")?;

        BABYLON_CURRENT_EPOCH
            .with_label_values(&[&self.chain_id.to_string(), &self.network])
            .set(current_epoch as i64);

        let epoch_to_process = current_epoch - 1;

        if epoch_to_process == self.processed_epoch {
            info!("(Babylon BLS Scrapper) Epoch to be processed: {}, has been already processed. Skipping... ", epoch_to_process);
            return Ok(());
        }

        info!(
            "(Babylon BLS Scrapper) Processing epoch: {}",
            epoch_to_process
        );

        let last_finalized_epoch = self
            .get_epoch(epoch_to_process)
            .await
            .context(format!("Could not obtain epoch {}", epoch_to_process))?;

        let epoch_first_block = last_finalized_epoch
            .first_block_height
            .parse::<usize>()
            .context("Could not parse last finalized epoch first block height")?;

        let validators = self
            .get_rpc_validators(&format!("/validators?height={}", epoch_first_block))
            .await
            .context("Could not obtain RPC validators")?;

        let block_txs = self
            .get_block_txs(epoch_first_block)
            .await
            .context("Could not obtain epoch first block txs")?;

        let mut validators_missing_block = Vec::new();
        for tx in block_txs {
            for message in tx.body.messages {
                for vote in message.extended_commit_info.votes {
                    if vote.extension_signature.is_none() {
                        let address_bytes = general_purpose::STANDARD
                            .decode(vote.validator.address)
                            .context("Could not decode validator address inside block txs")?;
                        let address: String = address_bytes
                            .iter()
                            .map(|byte| format!("{:02x}", byte))
                            .collect();
                        let address = address.to_uppercase();
                        validators_missing_block.push(address);
                    }
                }
            }
        }

        for validator in validators {
            let fires_alerts = self
                .validator_alert_addresses
                .contains(&validator.address)
                .to_string();

            if validators_missing_block.contains(&validator.address) {
                info!(
                    "(Babylon BLS Scrapper) Found validator missing checkpoint: {}",
                    &validator.address
                );
                BABYLON_VALIDATOR_MISSING_BLS_VOTE
                    .with_label_values(&[
                        &validator.address,
                        &self.chain_id.to_string(),
                        &self.network,
                        &fires_alerts,
                    ])
                    .inc();
            }
        }

        self.processed_epoch = epoch_to_process;
        Ok(())
    }
}

#[async_trait]
impl Task for BabylonBlsScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_bls().await.context("Could not process BLS")
    }

    fn name(&self) -> &'static str {
        "Babylon BLS Scrapper"
    }
}
