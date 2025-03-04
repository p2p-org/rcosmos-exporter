use std::{collections::VecDeque, sync::Arc};

use chrono::NaiveDateTime;
use serde_json::from_str;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::core::{
    blockchain::{
        BlockHeight, BlockScrapper, BlockchainMonitor, NetworkScrapper, ValidatorMetrics,
    },
    blockchain_client::BlockchainClient,
    http_client::HTTPClientErrors,
};

use super::{
    metrics::{
        TENDERMINT_CURRENT_BLOCK_HEIGHT, TENDERMINT_CURRENT_BLOCK_TIME,
        TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS, TENDERMINT_MY_VALIDATOR_PROPOSER_PRIORITY,
        TENDERMINT_MY_VALIDATOR_VOTING_POWER,
    },
    types::{
        TendermintBlockResponse, TendermintStatusResponse, TendermintValidator, ValidatorsResponse,
    },
};

pub struct Tendermint {
    pub client: Arc<Mutex<BlockchainClient>>,
}

impl BlockchainMonitor for Tendermint {
    async fn start_monitoring(self) {
        tokio::spawn(async move {
            loop {
                let mut client = self.client.lock().await;
                self.get_chain_id(&mut client).await;
                self.process_block_window(&mut client).await;
            }
        });
    }
}

impl BlockScrapper for Tendermint {
    type BlockResponse = TendermintBlockResponse;
    type Error = HTTPClientErrors;

    async fn get_chain_id(&self, client: &mut BlockchainClient) {
        info!("Getting chain_id");
        let res = match client.with_rpc().get("/status").await {
            Ok(res) => res,
            Err(e) => {
                error!("Error in the call to obtain chain_id: {:?}", e);
                panic!("Could not get chain_id");
            }
        };

        match from_str::<TendermintStatusResponse>(&res) {
            Ok(res) => client.chain_id = res.result.node_info.network,
            Err(e) => {
                error!("Error deserializing JSON: {}", e);
                error!("Raw JSON: {}", res);
                panic!("Could not obtain chain_id from JSON")
            }
        }
    }

    async fn get_block(
        &self,
        client: &mut BlockchainClient,
        height: BlockHeight,
    ) -> Result<TendermintBlockResponse, HTTPClientErrors> {
        let path = match height {
            BlockHeight::Height(h) => format!("block?height={}", h),
            BlockHeight::Latest => "block".to_string(),
        };

        info!("Obtaining block: {}", path);
        let res = match client.with_rpc().get(&path).await {
            Ok(res) => res,
            Err(e) => return Err(e),
        };

        match from_str::<TendermintBlockResponse>(&res) {
            Ok(block_res) => Ok(block_res),
            Err(e) => {
                error!("Error deserializing block JSON: {}", e);
                error!("Raw JSON: {}", res);
                panic!("Could not obtain chain_id from JSON")
            }
        }
    }

    async fn process_block_window(&self, client: &mut BlockchainClient) {
        let last_block_height = match self.get_block(client, BlockHeight::Latest).await {
            Ok(block) => block
                .result
                .block
                .header
                .height
                .parse::<i64>()
                .expect("Failed parsing block height"),
            Err(e) => {
                error!("Failed to obtain last_block_height");
                error!("{:?}", e);
                return;
            }
        };

        let mut height_to_process: i64;

        if client.proccessed_height == 0 {
            height_to_process = last_block_height - client.block_window;

            if height_to_process < 1 {
                height_to_process = 1;
            }
        } else {
            height_to_process = client.proccessed_height + 1;
        }

        while height_to_process < last_block_height {
            self.process_block(client, height_to_process).await;
            height_to_process += 1;
        }
    }

    async fn process_block(&self, client: &mut BlockchainClient, height: i64) {
        let block = match self.get_block(client, BlockHeight::Height(height)).await {
            Ok(block) => block,
            Err(e) => {
                error!("Failed to process block at height {}", height);
                error!("{:?}", e);
                return;
            }
        };

        let block_height = block
            .result
            .block
            .header
            .height
            .parse::<i64>()
            .expect("Failed parsing block height");
        let block_time = block.result.block.header.time;

        let block_signatures = block.result.block.last_commit.signatures;

        let signed = block_signatures
            .iter()
            .any(|sig| sig.validator_address == client.validator_address);

        if !signed {}

        self.set_current_block_height(block_height, client.chain_id.clone())
            .await;
        self.set_current_block_time(block_time, client.chain_id.clone())
            .await;

        client.proccessed_height = height
    }
}

impl ValidatorMetrics for Tendermint {
    async fn set_current_block_height(&self, height: i64, chain_id: String) {
        TENDERMINT_CURRENT_BLOCK_HEIGHT
            .with_label_values(&[&chain_id])
            .set(height.try_into().unwrap());
    }

    async fn set_current_block_time(&self, block_time: NaiveDateTime, chain_id: String) {
        TENDERMINT_CURRENT_BLOCK_TIME
            .with_label_values(&[&chain_id])
            .set(block_time.and_utc().timestamp() as f64);
    }

    async fn set_my_validator_missed_blocks(&self, chain_id: String, validator_address: String) {
        TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS
            .with_label_values(&[&validator_address, &chain_id])
            .inc();
    }

    async fn set_my_validator_voting_power(&self, chain_id: String, validator_address: String) {}
}

impl NetworkScrapper for Tendermint {
    async fn get_validators(self, client: &mut BlockchainClient) {
        let res = match client.with_rpc().get("/validators").await {
            Ok(res) => res,
            Err(e) => {
                error!("Error calling to validators endpoint: {}", e);
                return;
            }
        };

        let validators: Vec<TendermintValidator> = match from_str::<ValidatorsResponse>(&res) {
            Ok(res) => {
                if let Some(result) = res.result {
                    result.validators
                } else {
                    return;
                }
            }
            Err(e) => {
                error!("Error deserializing JSON: {}", e);
                error!("Raw JSON: {}", res);
                panic!("Could not obtain chain_id from JSON")
            }
        };

        for validator in validators {
            if validator.address == client.validator_address {
                TENDERMINT_MY_VALIDATOR_VOTING_POWER
                    .with_label_values(&[&client.validator_address, &client.chain_id])
                    .set(validator.voting_power.parse::<f64>().unwrap_or(0.0));

                TENDERMINT_MY_VALIDATOR_PROPOSER_PRIORITY
                    .with_label_values(&[&client.validator_address, &client.chain_id])
                    .set(validator.proposer_priority.parse::<f64>().unwrap_or(0.0));
            }
        }
    }

    async fn get_proposals(self, client: &mut BlockchainClient) {
        todo!()
    }
}
