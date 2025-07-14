use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use serde_json::from_str;
use tracing::info;

use crate::{
    blockchains::{
        babylon::types::{CurrentEpoch, GetEpochResponse},
        cometbft::types::{Validator, ValidatorsResponse},
    },
    core::{app_context::AppContext, clients::path::Path, exporter::RunnableModule},
};

use super::{
    metrics::{BABYLON_CURRENT_EPOCH, BABYLON_VALIDATOR_MISSING_BLS_VOTE},
    types::{BlockTxs, Epoch, Tx},
};

pub struct Bls {
    app_context: Arc<AppContext>,
    processed_epoch: usize,
}

impl Bls {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self {
            app_context,
            processed_epoch: 0,
        }
    }

    async fn fetch_validators(&self, path: &str) -> anyhow::Result<Vec<Validator>> {
        let mut validators: Vec<Validator> = Vec::new();
        let mut all_fetched = false;
        let mut page = 1;
        let client = self.app_context.rpc.as_ref().unwrap();

        while !all_fetched {
            let res = client
                .get(Path::from(format!("{}&page={}", path, page)))
                .await
                .context(format!(
                    "Could not fetch active validators page: {}, path: {}",
                    page, path
                ))?;

            let validators_response =
                from_str::<ValidatorsResponse>(&res).context("Could not decode JSON response")?;

            if let Some(res) = validators_response.result {
                let count = res.count.parse::<usize>().context(
                    "Could not parse the count of obtained validators when fetching validators",
                )?;
                let total = res
                    .total
                    .parse::<usize>()
                    .context("Could not parse the total of validators when fetching validators")?;
                if count + validators.len() == total {
                    all_fetched = true;
                } else {
                    page += 1;
                }
                validators.extend(res.validators)
            } else {
                anyhow::bail!("Result key not present at validators rpc endpoint response");
            }
        }

        Ok(validators)
    }

    async fn get_current_epoch(&self) -> anyhow::Result<usize> {
        info!("(Babylon BLS) Getting current epoch");
        let client = self.app_context.lcd.as_ref().unwrap();
        let res = client
            .get(Path::from("/babylon/epoching/v1/current_epoch"))
            .await
            .context("Could not fetch current epoch")?;
        from_str::<CurrentEpoch>(&res)
            .context("Could not deserialize current epoch")?
            .current_epoch
            .parse::<usize>()
            .context("Could not parse current epoch")
    }

    async fn get_epoch(&self, epoch: usize) -> anyhow::Result<Epoch> {
        info!("(Babylon BLS) Getting epoch {}", epoch);
        let client = self.app_context.lcd.as_ref().unwrap();
        let res = client
            .get(Path::from(format!("/babylon/epoching/v1/epochs/{}", epoch)))
            .await
            .context(format!("Could not fetch epoch: {}", epoch))?;
        Ok(from_str::<GetEpochResponse>(&res)
            .context("Could not deserialize epoch response")?
            .epoch)
    }

    async fn get_block_txs(&self, block: usize) -> anyhow::Result<Vec<Tx>> {
        info!("(Babylon BLS) Getting block {} txs", block);
        let client = self.app_context.lcd.as_ref().unwrap();
        let res = client
            .get(Path::from(format!(
                "/cosmos/tx/v1beta1/txs/block/{}?pagination.limit=1",
                block
            )))
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
            .with_label_values(&[
                &self.app_context.config.general.network,
                &self.app_context.config.general.network,
            ])
            .set(current_epoch as i64);
        let epoch_to_process = current_epoch - 1;
        info!("(Babylon BLS) Processing epoch: {}", epoch_to_process);

        if epoch_to_process == self.processed_epoch {
            info!(
                "(Babylon BLS) Epoch to be processed: {}, has been already processed. Skipping... ",
                epoch_to_process
            );
            return Ok(());
        }

        let last_finalized_epoch = self
            .get_epoch(epoch_to_process)
            .await
            .context(format!("Could not obtain epoch {}", epoch_to_process))?;
        let epoch_first_block = last_finalized_epoch
            .first_block_height
            .parse::<usize>()
            .context("Could not parse last finalized epoch first block height")?;
        let validators = self
            .fetch_validators(&format!("/validators?height={}", epoch_first_block))
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
                .app_context
                .config
                .general
                .alerting
                .validators
                .contains(&validator.address)
                .to_string();
            if validators_missing_block.contains(&validator.address) {
                info!(
                    "(Babylon BLS) Found validator missing checkpoint: {}",
                    &validator.address
                );
                BABYLON_VALIDATOR_MISSING_BLS_VOTE
                    .with_label_values(&[
                        &validator.address,
                        &self.app_context.config.general.network,
                        &self.app_context.config.general.network,
                        &fires_alerts,
                    ])
                    .inc();
            }
        }
        self.processed_epoch = epoch_to_process;
        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.rpc.is_none() {
        anyhow::bail!("Config is missing RPC node pool");
    }
    if app_context.lcd.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    Ok(Box::new(Bls::new(app_context)))
}

#[async_trait]
impl RunnableModule for Bls {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_bls().await.context("Could not process BLS")
    }
    fn name(&self) -> &'static str {
        "Babylon BLS"
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.app_context.config.network.babylon.bls.interval as u64)
    }
}
