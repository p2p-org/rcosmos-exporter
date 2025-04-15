use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::{time::sleep};
use tracing::{error, info};

use crate::{
    blockchains::coredao::{
        metrics::COREDAO_BLOCK_SIGNER,
    },
    core::{clients::blockchain_client::BlockchainClient, exporter::Task},
};

pub struct CoreDaoBlockScrapper {
    client: Arc<BlockchainClient>,
    last_processed_block: u64,
}

impl CoreDaoBlockScrapper {
    pub fn new(client: Arc<BlockchainClient>) -> Self {
        Self {
            client,
            last_processed_block: 0,
        }
    }

    async fn get_latest_block_number(&self) -> Option<u64> {
        info!("(Core DAO Block Scrapper) Getting latest block number");

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        let res = match self.client.post_json("", &payload).await {
            Ok(res) => res,
            Err(e) => {
                error!("(Core DAO Block Scrapper) Error getting latest block number: {}", e);
                return None;
            }
        };

        let result: Value = match serde_json::from_str(&res) {
            Ok(val) => val,
            Err(e) => {
                error!("(Core DAO Block Scrapper) Error parsing JSON for blockNumber: {}", e);
                return None;
            }
        };

        let block_number_hex = match result.get("result") {
            Some(Value::String(hex)) => hex.trim_start_matches("0x"),
            _ => {
                error!("(Core DAO Block Scrapper) Invalid blockNumber result format");
                return None;
            }
        };

        match u64::from_str_radix(block_number_hex, 16) {
            Ok(num) => Some(num),
            Err(e) => {
                error!("(Core DAO Block Scrapper) Error parsing block number: {}", e);
                None
            }
        }
    }

    async fn get_block_by_number(&self, block_number: u64) -> Option<(u64, String)> {
        
        let block_number_hex = format!("0x{:x}", block_number);
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [block_number_hex, false],
            "id": 1
        });

        let res = match self.client.post_json("", &payload).await {
            Ok(res) => res,
            Err(e) => {
                error!("(Core DAO Block Scrapper) Error getting block: {}", e);
                return None;
            }
        };

        let result: Value = match serde_json::from_str(&res) {
            Ok(val) => val,
            Err(e) => {
                error!("(Core DAO Block Scrapper) Error parsing JSON for block {}: {}", block_number, e);
                return None;
            }
        };

        let block = match result.get("result") {
            Some(block) => block,
            None => {
                error!("(Core DAO Block Scrapper) No result field in response for block {}", block_number);
                return None;
            }
        };

        let miner = match block.get("miner") {
            Some(Value::String(miner)) => miner.clone(),
            _ => {
                error!("(Core DAO Block Scrapper) Invalid or missing miner field for block {}", block_number);
                return None;
            }
        };

        Some((block_number, miner))
    }
    
    async fn poll_for_blocks(&mut self, delay: Duration) {
        info!("(Core DAO Block Scrapper) Starting to poll for new blocks");
        
        // Initialize last_processed_block to the current latest block
        if let Some(latest_block) = self.get_latest_block_number().await {
            info!("(Core DAO Block Scrapper) Starting polling from latest block: {}", latest_block);
            self.last_processed_block = latest_block;
        } else {
            error!("(Core DAO Block Scrapper) Failed to get initial latest block number");
        }
        
        loop {
            // Get the latest block number
            if let Some(latest_block) = self.get_latest_block_number().await {
                // If we've seen a new block
                if latest_block > self.last_processed_block {
                    info!("(Core DAO Block Scrapper) Found new block: {}", latest_block);
                    
                    // Process all blocks from last_processed_block+1 to latest_block
                    for block_num in (self.last_processed_block + 1)..=latest_block {
                        if let Some((block_number, consensus_address)) = self.get_block_by_number(block_num).await {
                            
                            // Set the metric 
                            COREDAO_BLOCK_SIGNER
                                .with_label_values(&[
                                    &block_number.to_string(), 
                                    &consensus_address
                                ])
                                .set(1);
                        }
                    }
                    
                    // Update the last processed block
                    self.last_processed_block = latest_block;
                }
            }
            
            // Sleep for the specified delay before checking again
            sleep(delay).await;
        }
    }
}

#[async_trait]
impl Task for CoreDaoBlockScrapper {
    async fn run(&mut self, delay: Duration) {
        info!("(Core DAO Block Scrapper) Executing task");
        
        info!("(Core DAO Block Scrapper) Using polling method for block updates");
        self.poll_for_blocks(delay).await;
        
        // This point should never be reached as polling runs indefinitely
        info!("(Core DAO Block Scrapper) Task completed, next run in {:?}", delay);
        sleep(delay).await;
    }
}
