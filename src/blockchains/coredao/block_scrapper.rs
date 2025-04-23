use std::{sync::Arc, time::Duration, collections::VecDeque};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::{time::sleep};
use tracing::{error, info, debug};

use crate::{
    blockchains::coredao::metrics::{COREDAO_VALIDATOR_PARTICIPATION, COREDAO_VALIDATOR_RECENT_ACTIVITY, COREDAO_VALIDATOR_SIGNED_BLOCKS},
    core::{clients::blockchain_client::BlockchainClient, exporter::Task},
};

pub struct CoreDaoBlockScrapper {
    client: Arc<BlockchainClient>,
    last_processed_block: u64,
    // Store recent blocks and their signers
    recent_blocks: VecDeque<(u64, String)>,
    // Maximum blocks to track
    max_blocks: usize,
    // Validator addresses to monitor and alert on
    validator_alert_addresses: Vec<String>,
    network: String,
}

impl CoreDaoBlockScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        validator_alert_addresses: Vec<String>,
        network: String,
    ) -> Self {
        CoreDaoBlockScrapper {
            client,
            recent_blocks: VecDeque::with_capacity(100),
            max_blocks: 100,
            validator_alert_addresses,
            last_processed_block: 0,
            network,
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

        let res = match self.client.with_rpc().post("", &payload).await {
            Ok(res) => res,
            Err(e) => {
                error!(
                    "(Core DAO Block Scrapper) Error getting latest block number: {}",
                    e
                );
                return None;
            }
        };

        let result: Value = match serde_json::from_str(&res) {
            Ok(val) => val,
            Err(e) => {
                error!(
                    "(Core DAO Block Scrapper) Error parsing JSON for blockNumber: {}",
                    e
                );
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
                error!(
                    "(Core DAO Block Scrapper) Error parsing block number: {}",
                    e
                );
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

        let res = match self.client.with_rpc().post("", &payload).await {
            Ok(res) => res,
            Err(e) => {
                error!("(Core DAO Block Scrapper) Error getting block: {}", e);
                return None;
            }
        };

        let result: Value = match serde_json::from_str(&res) {
            Ok(val) => val,
            Err(e) => {
                error!(
                    "(Core DAO Block Scrapper) Error parsing JSON for block {}: {}",
                    block_number, e
                );
                return None;
            }
        };

        let block = match result.get("result") {
            Some(block) => block,
            None => {
                error!(
                    "(Core DAO Block Scrapper) No result field in response for block {}",
                    block_number
                );
                return None;
            }
        };

        let miner = match block.get("miner") {
            Some(Value::String(miner)) => miner.clone(),
            _ => {
                error!(
                    "(Core DAO Block Scrapper) Invalid or missing miner field for block {}",
                    block_number
                );
                return None;
            }
        };

        Some((block_number, miner))
    }

    async fn process_new_blocks(&mut self) {
        // Get the latest block number
        if let Some(latest_block) = self.get_latest_block_number().await {
            // If we've seen a new block
            if latest_block > self.last_processed_block {
                info!(
                    "(Core DAO Block Scrapper) Found new block: {}",
                    latest_block
                );

                // Process all blocks from last_processed_block+1 to latest_block
                for block_num in (self.last_processed_block + 1)..=latest_block {
                    if let Some((block_number, consensus_address)) = self.get_block_by_number(block_num).await {
                        self.recent_blocks.push_back((block_number, consensus_address.to_lowercase()));
                        
                        // Keep only the most recent max_blocks
                        if self.recent_blocks.len() > self.max_blocks {
                            self.recent_blocks.pop_front();
                        }
                        
                        debug!("(Core DAO Block Scrapper) Block {} signed by {}", block_number, consensus_address);
                    }
                }
                
                self.calculate_validator_participation();
                
                self.last_processed_block = latest_block;
            } else {
                debug!("(Core DAO Block Scrapper) No new blocks found");
            }
        } else {
            error!("(Core DAO Block Scrapper) Failed to get latest block number");
        }
    }
    
    fn calculate_validator_participation(&self) {
        if self.recent_blocks.is_empty() {
            return;
        }
        
        // Get unique validators from recent blocks to determine rotation size
        let mut unique_validators = std::collections::HashSet::new();
        for (_, validator) in &self.recent_blocks {
            unique_validators.insert(validator.clone());
        }
        
        let total_validators = unique_validators.len();
        if total_validators == 0 {
            error!("(Core DAO Block Scrapper) No validators found in recent blocks");
            return;
        }
        
        info!("(Core DAO Block Scrapper) Found {} unique validators in recent blocks", total_validators);
        
        // We need to track validator participation over three rounds
        let blocks_per_round = total_validators;
        let blocks_for_three_rounds = blocks_per_round * 3;
        
        if self.recent_blocks.len() < blocks_for_three_rounds {
            info!("(Core DAO Block Scrapper) Not enough blocks for 3 rounds (need {}, have {})",
                  blocks_for_three_rounds, self.recent_blocks.len());
            return;
        }
        
        let recent_three_rotations: Vec<_> = self.recent_blocks
            .iter()
            .rev()
            .take(blocks_for_three_rounds)
            .collect();
        
        // Count blocks signed by each validator across all three rotations
        let mut validator_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for (_, validator) in &recent_three_rotations {
            *validator_counts.entry(validator.clone()).or_insert(0) += 1;
        }
        
        // Calculate and set participation rates for all validators
        for validator in unique_validators {
            let blocks_signed = validator_counts.get(&validator).cloned().unwrap_or(0);
            
            // In an ideal scenario, each validator would sign exactly 3 blocks (one per rotation)
            // So we calculate participation as (blocks signed / 3) * 100%
            let participation_rate = (blocks_signed as f64 / 3.0) * 100.0;
            
            // Check if this is one of our alert validators
            let fires_alerts = self.validator_alert_addresses.contains(&validator).to_string();
            
            COREDAO_VALIDATOR_PARTICIPATION
                .with_label_values(&[&validator, &self.network.to_string(), &fires_alerts])
                .set(participation_rate);
                
            // Check if this is one of our alert validators
            if self.validator_alert_addresses.contains(&validator) {
                info!("(Core DAO Block Scrapper) Alert validator {} signed {} out of 3 expected blocks across 3 rotations ({}%)",
                      validator, blocks_signed, participation_rate);
            }
        }
        
        // Check if alert validators have signed at least once in the latest rotation
        for target in &self.validator_alert_addresses {
            // Get blocks for the latest rotation only
            let latest_rotation = &recent_three_rotations[0..blocks_per_round];
            
            let has_signed = latest_rotation
                .iter()
                .any(|(_, validator)| validator == target);
            
            let activity_value = if has_signed { 1.0 } else { 0.0 };
            let fires_alerts = "true";
            
            COREDAO_VALIDATOR_RECENT_ACTIVITY
                .with_label_values(&[target, &self.network.to_string(), fires_alerts])
                .set(activity_value);
            
            info!("(Core DAO Block Scrapper) Setting recent activity metric for {} to {} (signed in latest rotation: {})",
                  target, activity_value, has_signed);
            
            if !has_signed {
                info!("(Core DAO Block Scrapper) ALERT: Validator {} has not signed any blocks in the latest rotation!",
                      target);
            }
            
            // Track all blocks signed by the target validator
            let target_signed_blocks: Vec<_> = self.recent_blocks
                .iter()
                .filter(|(_, validator)| validator == target)
                .collect();
            
            for (block_number, _) in target_signed_blocks {
                COREDAO_VALIDATOR_SIGNED_BLOCKS
                    .with_label_values(&[target, &block_number.to_string(), &self.network.to_string(), fires_alerts])
                    .set(1.0);
                
                info!("(Core DAO Block Scrapper) Validator {} signed block {}",
                      target, block_number);
            }
        }
    }
}

#[async_trait]
impl Task for CoreDaoBlockScrapper {
    async fn run(&mut self, delay: Duration) {
        info!("(Core DAO Block Scrapper) Starting task");
        
        // Initialize metrics for all alert validators
        for target_address in &self.validator_alert_addresses {
            debug!("(Core DAO Block Scrapper) Forcibly initializing recent activity metric for {}", target_address);
            COREDAO_VALIDATOR_RECENT_ACTIVITY
                .with_label_values(&[target_address, &self.network.to_string(), "true"])
                .set(-1.0);  // Initialize with -1 to indicate "not enough data yet"
        }
        
        // Initialize last_processed_block to the current latest block
        if let Some(latest_block) = self.get_latest_block_number().await {
            debug!("(Core DAO Block Scrapper) Starting from latest block: {}", latest_block);
            self.last_processed_block = latest_block.saturating_sub(self.max_blocks as u64);
            debug!("(Core DAO Block Scrapper) Will process blocks from {} to {}", 
                  self.last_processed_block + 1, latest_block);
        } else {
            error!("(Core DAO Block Scrapper) Failed to get initial latest block number");
        }

        loop {
            debug!("(Core DAO Block Scrapper) Checking for new blocks");
            
            // Process any new blocks
            self.process_new_blocks().await;

            info!(
                "(Core DAO Block Scrapper) Block processing complete, sleeping for {:?}",
                delay
            );
            sleep(delay).await;
        }
    }
}