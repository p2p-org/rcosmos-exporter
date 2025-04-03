use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde_json::from_str;
use tokio::time::sleep;
use tracing::{error, info};

use crate::{
    blockchains::tendermint::types::{TendermintValidator, ValidatorsResponse},
    core::{
        block_height::BlockHeight, block_window::BlockWindow, chain_id::ChainId,
        clients::blockchain_client::BlockchainClient, exporter::Task,
    },
};

use super::{
    metrics::{
        TENDERMINT_CURRENT_BLOCK_HEIGHT, TENDERMINT_CURRENT_BLOCK_TIME,
        TENDERMINT_VALIDATOR_MISSED_BLOCKS, TENDERMINT_VALIDATOR_PROPOSED_BLOCKS,
        TENDERMINT_VALIDATOR_UPTIME,
    },
    types::{TendermintBlock, TendermintBlockResponse},
};

pub struct TendermintBlockScrapper {
    client: Arc<BlockchainClient>,
    validators: Vec<String>,
    block_window: BlockWindow,
    processed_height: usize,
    chain_id: ChainId,
}

impl TendermintBlockScrapper {
    pub fn new(client: Arc<BlockchainClient>, block_window: usize, chain_id: ChainId) -> Self {
        Self {
            client,
            validators: Vec::default(),
            block_window: BlockWindow::new(block_window),
            processed_height: 0,
            chain_id,
        }
    }

    async fn get_active_validator_set(&mut self) {
        info!("(Tendermint Block Scrapper) Fetching active validator set");
        let mut validators: Vec<TendermintValidator> = Vec::new();

        let mut all_fetched = false;
        let mut page = 1;
        let mut fetched = 0;

        while !all_fetched {
            let res = match self
                .client
                .with_rpc()
                .get(&format!("/validators?page={}", page))
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    error!(
                        "(Tendermint Block Scrapper) Error calling to RPC validators endpoint: {}",
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
                        error!("(Tendermint Block Scrapper) Result key not present at validators rpc endpoint response");
                        break;
                    }
                }
                Err(e) => {
                    error!(
                        "(Tendermint Block Scrapper) Error deserializing JSON: {}",
                        e
                    );
                    error!("(Mezo Validator Info) Raw JSON: {}", res);
                    break;
                }
            };

            validators.extend(fetched_validators);
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

        let res = self.client.with_rpc().get(&path).await?;

        match from_str::<TendermintBlockResponse>(&res) {
            Ok(res) => Ok(res.result.block),
            Err(e) => Err(e.into()),
        }
    }

    async fn process_block_window(&mut self) {
        let last_block_height = match self.get_block(BlockHeight::Latest).await {
            Ok(block) => match block.header.height.parse::<usize>() {
                Ok(height) => height,
                Err(_) => {
                    error!("(Tendermint Block Scrapper) Couldn't parse block height");
                    return;
                }
            },
            Err(e) => {
                error!("(Tendermint Block Scrapper) Failed to obtain last_block_height");
                error!("{:?}", e);
                return;
            }
        };

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
            self.process_block(height_to_process).await;
            height_to_process += 1;
        }

        let uptimes = self.block_window.uptimes();

        info!("(Tendermint Block Scrapper) Calculating uptime for validators");
        for validator in self.validators.iter() {
            let uptime = uptimes.get(validator).unwrap_or(&0.0);
            TENDERMINT_VALIDATOR_UPTIME
                .with_label_values(&[
                    validator,
                    &self.block_window.window.to_string(),
                    &self.chain_id.to_string(),
                ])
                .set(*uptime);
        }
    }

    async fn process_block(&mut self, height: usize) {
        let block = match self.get_block(BlockHeight::Height(height)).await {
            Ok(block) => block,
            Err(e) => {
                error!(
                    "(Tendermint Block Scrapper) Failed to process block at height {}",
                    height
                );
                error!("{:?}", e);
                return;
            }
        };

        let block_height = match block.header.height.parse::<usize>() {
            Ok(height) => height,
            Err(_) => {
                error!("(Tendermint Block Scrapper) Couldn't parse block height");
                return;
            }
        };

        let block_time = block.header.time;
        let block_proposer = block.header.proposer_address.clone();
        let block_signatures = block.last_commit.signatures.clone();

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
            None => {
                error!("(Tendermint Block Scrapper) Block proposer is not on validator map");
                return;
            }
        };

        TENDERMINT_VALIDATOR_PROPOSED_BLOCKS
            .with_label_values(&[proposer_address, &self.chain_id.to_string()])
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
            TENDERMINT_VALIDATOR_MISSED_BLOCKS
                .with_label_values(&[&validator, &self.chain_id.to_string()])
                .inc();
        }

        TENDERMINT_CURRENT_BLOCK_HEIGHT
            .with_label_values(&[&self.chain_id.to_string()])
            .set(block_height.try_into().unwrap());

        TENDERMINT_CURRENT_BLOCK_TIME
            .with_label_values(&[&self.chain_id.to_string()])
            .set(block_time.and_utc().timestamp() as f64);

        self.processed_height = block_height;
    }
}

#[async_trait]
impl Task for TendermintBlockScrapper {
    async fn run(&mut self, delay: Duration) {
        info!("Running Tendermint Block Scrapper");

        loop {
            self.get_active_validator_set().await;
            self.process_block_window().await;

            sleep(delay).await
        }
    }
}
