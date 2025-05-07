use anyhow::{bail, Context};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use serde_json::from_str;
use std::sync::Arc;
use tracing::{error, info};

use crate::{
    blockchains::tendermint::{
        metrics::{TENDERMINT_BLOCK_TXS, TENDERMINT_BLOCK_TX_SIZE},
        types::{TendermintValidator, ValidatorsResponse},
    },
    core::{
        block_height::BlockHeight, block_window::BlockWindow, chain_id::ChainId,
        clients::blockchain_client::BlockchainClient, exporter::Task,
    },
};

use super::{
    metrics::{
        TENDERMINT_BLOCK_GAS_USED, TENDERMINT_BLOCK_GAS_WANTED, TENDERMINT_BLOCK_TX_GAS_USED,
        TENDERMINT_BLOCK_TX_GAS_WANTED, TENDERMINT_CURRENT_BLOCK_HEIGHT,
        TENDERMINT_CURRENT_BLOCK_TIME, TENDERMINT_VALIDATOR_MISSED_BLOCKS,
        TENDERMINT_VALIDATOR_PROPOSED_BLOCKS, TENDERMINT_VALIDATOR_UPTIME,
    },
    types::{TendermintBlock, TendermintBlockResponse, TendermintTx, TendermintTxResponse},
};

pub struct TendermintBlockScrapper {
    client: Arc<BlockchainClient>,
    validators: Vec<String>,
    block_window: BlockWindow,
    processed_height: usize,
    chain_id: ChainId,
    network: String,
    validator_alert_addresses: Vec<String>,
}

impl TendermintBlockScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        block_window: usize,
        chain_id: ChainId,
        network: String,
        validator_alert_addresses: Vec<String>,
    ) -> Self {
        Self {
            client,
            validators: Vec::default(),
            block_window: BlockWindow::new(block_window),
            processed_height: 0,
            chain_id,
            network,
            validator_alert_addresses,
        }
    }

    async fn get_active_validator_set(&mut self) -> anyhow::Result<()> {
        info!("(Tendermint Block Scrapper) Fetching active validator set");
        let mut validators: Vec<TendermintValidator> = Vec::new();

        let mut all_fetched = false;
        let mut page = 1;
        let mut fetched = 0;

        while !all_fetched {
            let res = self
                .client
                .with_rpc()
                .get(&format!("/validators?page={}", page))
                .await
                .context(format!("Could not fetch active validators page: {}", page))?;

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

        for validator in validators {
            if !self.validators.contains(&validator.address) {
                info!(
                    "(Tendermint Block Scrapper) Tracking new validator (RPC Call): {}",
                    validator.address
                );
                self.validators.push(validator.address);
            }
        }
        Ok(())
    }

    async fn get_block_txs(&mut self, height: usize) -> anyhow::Result<Vec<TendermintTx>> {
        let res = self
            .client
            .with_rpc()
            .get(&format!("tx_search?query=\"tx.height={}\"", height))
            .await
            .context(format!("Could not fetch txs for height {}", height))?;

        Ok(from_str::<TendermintTxResponse>(&res)
            .context("Could not deserialize txs response")?
            .result
            .txs)
    }

    async fn get_block(&mut self, height: BlockHeight) -> anyhow::Result<TendermintBlock> {
        let path = match height {
            BlockHeight::Height(h) => {
                info!(
                    "(Tendermint Block Scrapper) Obtaining block with height: {}",
                    h
                );
                format!("/block?height={}", h)
            }
            BlockHeight::Latest => {
                info!("(Tendermint Block Scrapper) Obtaining latest block");
                "/block".to_string()
            }
        };

        let res = self
            .client
            .with_rpc()
            .get(&path)
            .await
            .context(format!("Could not fetch block {}", path))?;

        Ok(from_str::<TendermintBlockResponse>(&res)
            .context("Could not deserialize block response")?
            .result
            .block)
    }

    async fn process_block_window(&mut self) -> anyhow::Result<()> {
        let last_block = self
            .get_block(BlockHeight::Latest)
            .await
            .context("Could not obtain last block")?;

        let last_block_height = last_block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse last block height")?;

        let mut height_to_process;

        if self.processed_height == 0 {
            height_to_process = last_block_height - self.block_window.window;

            if height_to_process < 1 {
                height_to_process = 1;
            }
        } else {
            height_to_process = self.processed_height + 1;
        }

        while height_to_process < last_block_height {
            // Skip the block if error was encountered during processing
            self.process_block(height_to_process)
                .await
                .context(format!("Failed to process block {}", height_to_process))?;
            height_to_process += 1;
        }

        let uptimes = self.block_window.uptimes();

        info!("(Tendermint Block Scrapper) Calculating uptime for validators");
        for validator in self.validators.iter() {
            let uptime = uptimes.get(validator).unwrap_or(&0.0);
            let fires_alerts = self
                .validator_alert_addresses
                .contains(validator)
                .to_string();

            TENDERMINT_VALIDATOR_UPTIME
                .with_label_values(&[
                    validator,
                    &self.block_window.window.to_string(),
                    &self.chain_id.to_string(),
                    &self.network,
                    &fires_alerts,
                ])
                .set(*uptime);
        }
        Ok(())
    }

    async fn process_block(&mut self, height: usize) -> anyhow::Result<()> {
        let block = self
            .get_block(BlockHeight::Height(height))
            .await
            .context(format!("Could not obtain block {}", height))?;

        let block_height = block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse block height")?;

        let block_time = block.header.time;
        let block_proposer = block.header.proposer_address.clone();
        let block_signatures = block.last_commit.signatures.clone();

        TENDERMINT_BLOCK_TXS
            .with_label_values(&[&self.chain_id.to_string(), &self.network])
            .set(block.data.txs.len() as f64);

        let mut block_avg_tx_size: f64 = 0.0;
        let mut block_gas_wanted: f64 = 0.0;
        let mut block_gas_used: f64 = 0.0;
        let mut block_avg_tx_gas_wanted: f64 = 0.0;
        let mut block_avg_tx_gas_used: f64 = 0.0;

        if !block.data.txs.is_empty() {
            block_avg_tx_size = block
                .data
                .txs
                .iter()
                .filter_map(|tx| {
                    general_purpose::STANDARD
                        .decode(tx)
                        .context("Could not decode tx")
                        .ok()
                        .map(|decoded| decoded.len())
                })
                .sum::<usize>() as f64
                / block.data.txs.len() as f64;

            let txs_info = self
                .get_block_txs(height)
                .await
                .context(format!("Could not obtain txs info from block {}", height))?;

            let mut gas_wanted = Vec::new();
            let mut gas_used = Vec::new();

            for tx in txs_info {
                gas_wanted.push(
                    tx.tx_result
                        .gas_wanted
                        .parse::<usize>()
                        .context("Could not parse tx gas used")?,
                );
                gas_used.push(
                    tx.tx_result
                        .gas_used
                        .parse::<usize>()
                        .context("Could not parse tx gas used")?,
                );
            }

            block_gas_wanted = gas_wanted.iter().sum::<usize>() as f64;
            block_gas_used = gas_used.iter().sum::<usize>() as f64;
            block_avg_tx_gas_wanted =
                gas_wanted.iter().sum::<usize>() as f64 / gas_wanted.len() as f64;
            block_avg_tx_gas_used = gas_used.iter().sum::<usize>() as f64 / gas_used.len() as f64;
        }

        TENDERMINT_BLOCK_TX_SIZE
            .with_label_values(&[&self.chain_id.to_string(), &self.network])
            .set(block_avg_tx_size);

        TENDERMINT_BLOCK_GAS_WANTED
            .with_label_values(&[&self.chain_id.to_string(), &self.network])
            .set(block_gas_wanted);

        TENDERMINT_BLOCK_GAS_USED
            .with_label_values(&[&self.chain_id.to_string(), &self.network])
            .set(block_gas_used);

        TENDERMINT_BLOCK_TX_GAS_WANTED
            .with_label_values(&[&self.chain_id.to_string(), &self.network])
            .set(block_avg_tx_gas_wanted);

        TENDERMINT_BLOCK_TX_GAS_USED
            .with_label_values(&[&self.chain_id.to_string(), &self.network])
            .set(block_avg_tx_gas_used);

        for sig in block_signatures.iter() {
            if !sig.validator_address.is_empty()
                && !self.validators.contains(&sig.validator_address)
            {
                self.validators.push(sig.validator_address.clone());
                info!(
                    "(Tendermint Block Scrapper) Tracking new validator (found in block): {}",
                    sig.validator_address
                )
            }
        }

        self.block_window.add_block_signers(
            block_signatures
                .iter()
                .map(|sig| sig.validator_address.clone())
                .collect(),
        );

        let proposer_address = match self
            .validators
            .iter()
            .find(|validator_adress| **validator_adress == block_proposer)
        {
            Some(name) => name,
            None => bail!("Found a proposer that address is not on the validator list"),
        };

        TENDERMINT_VALIDATOR_PROPOSED_BLOCKS
            .with_label_values(&[proposer_address, &self.chain_id.to_string(), &self.network])
            .inc();

        let validators_missing_block: Vec<String> = self
            .validators
            .iter()
            .filter(|validator| {
                block_signatures
                    .iter()
                    .all(|sig| sig.validator_address != validator.as_str())
            })
            .cloned()
            .collect();

        for validator in validators_missing_block {
            let fires_alerts = self
                .validator_alert_addresses
                .contains(&validator)
                .to_string();

            TENDERMINT_VALIDATOR_MISSED_BLOCKS
                .with_label_values(&[
                    &validator,
                    &self.chain_id.to_string(),
                    &self.network,
                    &fires_alerts,
                ])
                .inc();
        }

        TENDERMINT_CURRENT_BLOCK_HEIGHT
            .with_label_values(&[&self.chain_id.to_string(), &self.network])
            .set(
                block_height
                    .try_into()
                    .context("Failed to parse block height to i64")?,
            );

        TENDERMINT_CURRENT_BLOCK_TIME
            .with_label_values(&[&self.chain_id.to_string(), &self.network])
            .set(block_time.and_utc().timestamp() as f64);

        self.processed_height = block_height;

        Ok(())
    }
}

#[async_trait]
impl Task for TendermintBlockScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.get_active_validator_set()
            .await
            .context("Could not get active validator set")?;
        self.process_block_window()
            .await
            .context("Could not process block window")
    }

    fn name(&self) -> &'static str {
        "Tendermint Block Scrapper"
    }
}
