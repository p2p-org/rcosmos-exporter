use std::{collections::{HashMap, VecDeque}, sync::Arc};

use anyhow::Context;
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, error, info};

use crate::{
    blockchains::coredao::metrics::{
        COREDAO_VALIDATOR_PARTICIPATION, COREDAO_VALIDATOR_RECENT_ACTIVITY,
        COREDAO_VALIDATOR_RECENT_ACTIVITY_BLOCK, COREDAO_VALIDATOR_SIGNED_BLOCKS,
        COREDAO_VALIDATOR_UPTIME,
    },
    blockchains::coredao::validator::ValidatorFetcher,
    core::{
        app_context::AppContext, block_window::BlockWindow, clients::path::Path,
        exporter::RunnableModule,
    },
};

pub struct Block {
    pub app_context: Arc<AppContext>,
    last_processed_block: u64,
    // Store recent blocks and their signers
    recent_blocks: VecDeque<(u64, String)>,
    // Maximum blocks to track for participation calculation
    max_blocks: usize,
    // Block window for historical uptime tracking
    block_window: BlockWindow,
    // Validator addresses to monitor and alert on
    validator_alert_addresses: Vec<String>,
    // Validator name cache (address -> name)
    validator_names: HashMap<String, String>,
    // Fetcher to refresh names periodically
    validator_fetcher: ValidatorFetcher,
    // Consensus address -> operator address mapping
    consensus_to_operator: HashMap<String, String>,
    initialized: bool,
}

impl Block {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        let addresses = app_context.config.general.alerting.validators.clone();
        let window = app_context.config.network.coredao.block.window as usize;
        Block {
            app_context: app_context.clone(),
            recent_blocks: VecDeque::with_capacity(100),
            max_blocks: 100,
            block_window: BlockWindow::new(window),
            validator_alert_addresses: addresses,
            last_processed_block: 0,
            validator_names: HashMap::new(),
            validator_fetcher: ValidatorFetcher::new(app_context.clone()),
            consensus_to_operator: HashMap::new(),
            initialized: false,
        }
    }

    async fn refresh_validator_names(&mut self) {
        if let Ok(list) = self.validator_fetcher.get_validators().await {
            let mut map: HashMap<String, String> = HashMap::with_capacity(list.len());
            for v in list.into_iter() {
                map.insert(v.address.to_lowercase(), v.name);
            }
            self.validator_names = map;
        }
    }

    fn name_for(&self, address: &str) -> String {
        let key = address.to_lowercase();
        self.validator_names
            .get(&key)
            .cloned()
            .unwrap_or_else(|| address.to_string())
    }

    async fn refresh_consensus_operator_map(&mut self) {
        // Read ValidatorSet operator and consensus lists and zip
        let client = if let Some(rpc) = self.app_context.rpc.as_ref() { rpc } else { return; };
        // getValidatorOps()
        let payload_ops = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": "0x0000000000000000000000000000000000001000",
                "data": "0x93f2d404"
            }, "latest"],
            "id": 1
        });
        // getValidators() -> consensus list
        let payload_cons = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": "0x0000000000000000000000000000000000001000",
                "data": "0xb7ab4db5"
            }, "latest"],
            "id": 1
        });
        let (res_ops, res_cons) = match (client.post(Path::from(""), &payload_ops).await.ok(), client.post(Path::from(""), &payload_cons).await.ok()) {
            (Some(a), Some(b)) => (a, b),
            _ => return,
        };
        let parse_addresses = |res: &str| -> Option<Vec<String>> {
            let v: serde_json::Value = serde_json::from_str(res).ok()?;
            let hex_data = v.get("result").and_then(serde_json::Value::as_str)?.trim_start_matches("0x");
            if hex_data.len() < 128 { return None; }
            let length_hex = &hex_data[64..128];
            let length = u64::from_str_radix(length_hex, 16).ok()? as usize;
            let mut out = Vec::with_capacity(length);
            for i in 0..length {
                let start = 128 + i * 64;
                if start + 64 <= hex_data.len() {
                    let addr = format!("0x{}", &hex_data[start + 24..start + 64]);
                    out.push(addr.to_lowercase());
                }
            }
            Some(out)
        };
        let ops = match parse_addresses(&res_ops) { Some(v) => v, None => return };
        let cons = match parse_addresses(&res_cons) { Some(v) => v, None => return };
        if ops.len() != cons.len() { return; }
        let mut map = HashMap::with_capacity(cons.len());
        for (c, o) in cons.into_iter().zip(ops.into_iter()) {
            map.insert(c, o);
        }
        self.consensus_to_operator = map;
    }

    async fn get_latest_block_number(&self) -> anyhow::Result<u64> {
        info!("(Core DAO Block) Getting latest block number");

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        let res = self
            .app_context
            .rpc
            .as_ref()
            .unwrap()
            .post(Path::from(""), &payload)
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
            .app_context
            .rpc
            .as_ref()
            .unwrap()
            .post(Path::from(""), &payload)
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
        // Refresh names at the start of each cycle so metrics always use latest names
        self.refresh_validator_names().await;
        self.refresh_consensus_operator_map().await;
        let latest_block = self
            .get_latest_block_number()
            .await
            .context("Could not obtain latest block number")?;

        if latest_block > self.last_processed_block {
            info!("(Core DAO Block) Found new block: {}", latest_block);

            // Process all blocks from last_processed_block+1 to latest_block
            for block_num in (self.last_processed_block + 1)..=latest_block {
                let (block_number, consensus_address) =
                    self.get_block_by_number(block_num)
                        .await
                        .context("Could not obtain block by number")?;

                let consensus_address = consensus_address.to_lowercase();
                let operator_address = self
                    .consensus_to_operator
                    .get(&consensus_address)
                    .cloned()
                    .unwrap_or(consensus_address.clone());
                self.recent_blocks
                    .push_back((block_number, operator_address.clone()));

                // Add operator address to block window for historical uptime tracking
                self.block_window
                    .add_block_signers(vec![operator_address.clone()]);

                // Increment the counter if this block was signed by one of our alert validators
                for target in &self.validator_alert_addresses {
                    if &operator_address == target {
                        let validator_name = self.name_for(target);
                        COREDAO_VALIDATOR_SIGNED_BLOCKS
                            .with_label_values(&[
                                target,
                                &validator_name,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                "true",
                            ])
                            .inc();

                        debug!(
                            "(Core DAO Block) Incrementing signed blocks counter for validator {}",
                            target
                        );
                    }
                }

                // Keep only the most recent max_blocks
                if self.recent_blocks.len() > self.max_blocks {
                    self.recent_blocks.pop_front();
                }

                debug!(
                    "(Core DAO Block) Block {} signed by {}",
                    block_number, consensus_address
                );
            }

            self.calculate_validator_participation();
            self.calculate_historical_uptime();

            self.last_processed_block = latest_block;
        } else {
            debug!("(Core DAO Block) No new blocks found");
        }
        Ok(())
    }

    fn calculate_validator_participation(&mut self) {
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
            error!("(Core DAO Block) No validators found in recent blocks");
            return;
        }

        info!(
            "(Core DAO Block) Found {} unique validators in recent blocks",
            total_validators
        );

        // We need to track validator participation over three rounds
        let blocks_per_round = total_validators;
        let blocks_for_three_rounds = blocks_per_round * 3;

        if self.recent_blocks.len() < blocks_for_three_rounds {
            info!(
                "(Core DAO Block) Not enough blocks for 3 rounds (need {}, have {})",
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
        for validator in &unique_validators {
            let blocks_signed = validator_counts.get(validator).cloned().unwrap_or(0);
            let participation_rate = (blocks_signed as f64 / 3.0) * 100.0;
            let fires_alerts = self
                .validator_alert_addresses
                .contains(&validator)
                .to_string();
            let validator_name = self.name_for(validator);
            COREDAO_VALIDATOR_PARTICIPATION
                .with_label_values(&[
                    // Use operator address for labeling consistently
                    validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(participation_rate);
            if self.validator_alert_addresses.contains(&validator) {
                info!("(Core DAO Block) Alert validator {} signed {} out of 3 expected blocks across 3 rotations ({}%)", validator, blocks_signed, participation_rate);
            }
        }

        // Check recent activity for all validators
        for validator in &unique_validators {
            // Get blocks for the latest rotation only
            let latest_rotation = &recent_three_rotations[0..blocks_per_round];
            let has_signed = latest_rotation.iter().any(|(_, v)| v == validator);
            let activity_value = if has_signed { 1.0 } else { 0.0 };
            let fires_alerts = self
                .validator_alert_addresses
                .contains(validator)
                .to_string();
            let validator_name = self.name_for(validator);
            let block_number_value = latest_rotation
                .iter()
                .rev()
                .find(|(_, v)| v == validator)
                .map(|(b, _)| *b)
                .unwrap_or_else(|| latest_rotation.first().map(|(b, _)| *b).unwrap_or(0));
            // Set the recent activity metric (1/0) for ALL validators
            COREDAO_VALIDATOR_RECENT_ACTIVITY
                .with_label_values(&[
                    validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(activity_value);
            // Set the recent activity block metric
            COREDAO_VALIDATOR_RECENT_ACTIVITY_BLOCK
                .with_label_values(&[
                    validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(block_number_value as f64);
            info!("(Core DAO Block) Setting recent activity metric for {} to {} (signed in latest rotation: {}, block: {})", validator, activity_value, has_signed, block_number_value);
        }
    }

    fn calculate_historical_uptime(&self) {
        info!("(Core DAO Block) Calculating historical uptime over block window");

        let window_size = self.block_window.window;
        let blocks = self.block_window.blocks();

        if blocks.is_empty() {
            info!("(Core DAO Block) No blocks in window for uptime calculation");
            return;
        }

        // Count actual signs per validator in the window and gather unique validators
        let mut sign_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        let mut unique_validators: std::collections::HashSet<String> = std::collections::HashSet::new();
        for block_signers in blocks {
            for signer in block_signers {
                *sign_counts.entry(signer.clone()).or_insert(0) += 1;
                unique_validators.insert(signer.clone());
            }
        }

        if unique_validators.is_empty() {
            info!("(Core DAO Block) No validators found in block window");
            return;
        }

        let rotation_size = unique_validators.len() as f64;
        let total_blocks = blocks.len() as f64;
        let expected_per_validator = if rotation_size > 0.0 { total_blocks / rotation_size } else { 0.0 };

        // Uptime = signed_count / expected_per_validator, capped at 100%
        for validator_address in &unique_validators {
            let signed_count = *sign_counts.get(validator_address).unwrap_or(&0) as f64;
            let uptime_percentage = if expected_per_validator > 0.0 {
                (signed_count / expected_per_validator).min(1.0) * 100.0
            } else { 0.0 };

            let fires_alerts = self
                .validator_alert_addresses
                .contains(validator_address)
                .to_string();

            let validator_name = self.name_for(validator_address);
            COREDAO_VALIDATOR_UPTIME
                .with_label_values(&[
                    validator_address,
                    &validator_name,
                    &window_size.to_string(),
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(uptime_percentage);

            if self.validator_alert_addresses.contains(validator_address) {
                info!(
                    "(Core DAO Block) Validator {} historical uptime: {:.2}% (signed {:.0} of expected {:.1}) over {} blocks",
                    validator_address, uptime_percentage, signed_count, expected_per_validator, window_size
                );
            }
        }

        // Set 0% for alert validators that didn't appear in the window
        for validator_address in &self.validator_alert_addresses {
            if !unique_validators.contains(validator_address) {
                let validator_name = self.name_for(validator_address);
                COREDAO_VALIDATOR_UPTIME
                    .with_label_values(&[
                        validator_address,
                        &validator_name,
                        &window_size.to_string(),
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        "true",
                    ])
                    .set(0.0);

                info!(
                    "(Core DAO Block) Alert validator {} not present in window - 0% uptime over {} blocks",
                    validator_address, window_size
                );
            }
        }
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.rpc.is_none() {
        anyhow::bail!("Config is missing RPC node pool");
    }
    Ok(Box::new(Block::new(app_context)))
}

#[async_trait]
impl RunnableModule for Block {
    async fn run(&mut self) -> anyhow::Result<()> {
        if !self.initialized {
            for target_address in &self.validator_alert_addresses {
                debug!(
                    "(Core DAO Block) Forcibly initializing recent activity metric for {}",
                    target_address
                );
                COREDAO_VALIDATOR_RECENT_ACTIVITY
                    .with_label_values(&[
                        target_address,
                        // name fallback at init
                        target_address,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        "true",
                    ])
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
        "Core DAO Block"
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.app_context.config.network.coredao.block.interval)
    }
}
