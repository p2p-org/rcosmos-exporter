use std::{collections::VecDeque, sync::Arc};

use anyhow::Context;
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, error, info};

use crate::{
    blockchains::coredao::metrics::{
        COREDAO_VALIDATOR_PARTICIPATION, COREDAO_VALIDATOR_RECENT_ACTIVITY,
        COREDAO_VALIDATOR_SIGNED_BLOCKS,
    },
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
    initialized: bool,
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
            initialized: false,
        }
    }

    async fn get_latest_block_number(&self) -> anyhow::Result<u64> {
        info!("(Core DAO Block Scrapper) Getting latest block number");

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        let res = self
            .client
            .with_rpc()
            .post("", &payload)
            .await
            .context("Could not fetch latest block number")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing JSON for latest block number response")?;

        let block_number_hex = result
            .get("result")
            .and_then(Value::as_str)
            .context("Invalid block number result format")?
            .trim_start_matches("0x");

        u64::from_str_radix(block_number_hex, 16).context("Could not parse block number hex")
    }

    async fn get_block_by_number(&self, block_number: u64) -> anyhow::Result<(u64, String)> {
        let block_number_hex = format!("0x{:x}", block_number);

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [block_number_hex, false],
            "id": 1
        });

        let res = self
            .client
            .with_rpc()
            .post("", &payload)
            .await
            .context(format!("Error fetching block: {}", block_number))?;

        let result: Value = serde_json::from_str(&res)
            .context(format!("Could not parse block {}", block_number))?;

        let block = result.get("result").context(format!(
            "No result field in response for block {}",
            block_number
        ))?;

        let miner = block.get("miner").and_then(Value::as_str).context(format!(
            "Invalid or missing miner field for block {}",
            block_number
        ))?;

        Ok((block_number, miner.to_string()))
    }

    async fn process_new_blocks(&mut self) -> anyhow::Result<()> {
        let latest_block = self
            .get_latest_block_number()
            .await
            .context("Could not obtain latest block number")?;

        if latest_block > self.last_processed_block {
            info!(
                "(Core DAO Block Scrapper) Found new block: {}",
                latest_block
            );

            // Process all blocks from last_processed_block+1 to latest_block
            for block_num in (self.last_processed_block + 1)..=latest_block {
                let (block_number, consensus_address) =
                    self.get_block_by_number(block_num)
                        .await
                        .context("Could not obtain block by number")?;

                let consensus_address = consensus_address.to_lowercase();
                self.recent_blocks
                    .push_back((block_number, consensus_address.clone()));

                // Increment the counter if this block was signed by one of our alert validators
                for target in &self.validator_alert_addresses {
                    if &consensus_address == target {
                        COREDAO_VALIDATOR_SIGNED_BLOCKS
                            .with_label_values(&[target, &self.network.to_string(), "true"])
                            .inc();

                        debug!("(Core DAO Block Scrapper) Incrementing signed blocks counter for validator {}", target);
                    }
                }

                // Keep only the most recent max_blocks
                if self.recent_blocks.len() > self.max_blocks {
                    self.recent_blocks.pop_front();
                }

                debug!(
                    "(Core DAO Block Scrapper) Block {} signed by {}",
                    block_number, consensus_address
                );
            }

            self.calculate_validator_participation();

            self.last_processed_block = latest_block;
        } else {
            debug!("(Core DAO Block Scrapper) No new blocks found");
        }
        Ok(())
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

        info!(
            "(Core DAO Block Scrapper) Found {} unique validators in recent blocks",
            total_validators
        );

        // We need to track validator participation over three rounds
        let blocks_per_round = total_validators;
        let blocks_for_three_rounds = blocks_per_round * 3;

        if self.recent_blocks.len() < blocks_for_three_rounds {
            info!(
                "(Core DAO Block Scrapper) Not enough blocks for 3 rounds (need {}, have {})",
                blocks_for_three_rounds,
                self.recent_blocks.len()
            );
            return;
        }

        let recent_three_rotations: Vec<_> = self
            .recent_blocks
            .iter()
            .rev()
            .take(blocks_for_three_rounds)
            .collect();

        // Count blocks signed by each validator across all three rotations
        let mut validator_counts: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
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
            let fires_alerts = self
                .validator_alert_addresses
                .contains(&validator)
                .to_string();

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

            // Count all blocks signed by the target validator (for logging only)
            let target_signed_blocks_count = self
                .recent_blocks
                .iter()
                .filter(|(_, validator)| validator == target)
                .count();

            info!(
                "(Core DAO Block Scrapper) Validator {} has signed {} blocks in the tracked window",
                target, target_signed_blocks_count
            );
        }
    }
}

#[async_trait]
impl Task for CoreDaoBlockScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        if !self.initialized {
            for target_address in &self.validator_alert_addresses {
                debug!(
                    "(Core DAO Block Scrapper) Forcibly initializing recent activity metric for {}",
                    target_address
                );
                COREDAO_VALIDATOR_RECENT_ACTIVITY
                    .with_label_values(&[target_address, &self.network.to_string(), "true"])
                    .set(-1.0); // Initialize with -1 to indicate "not enough data yet"
            }

            // Initialize last_processed_block to the current latest block
            let latest_block = self
                .get_latest_block_number()
                .await
                .context("Failed to obtain initial latest block")?;

            self.last_processed_block = latest_block.saturating_sub(self.max_blocks as u64);

            self.initialized = true;
        }

        self.process_new_blocks()
            .await
            .context("Failed to process new blocks")
    }

    fn name(&self) -> &'static str {
        "Core DAO Block Scrapper"
    }
}
