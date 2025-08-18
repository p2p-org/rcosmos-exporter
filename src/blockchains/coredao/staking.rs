use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::info;

use crate::{
    blockchains::coredao::metrics::{
        COREDAO_CORE_VALIDATOR_STAKE_SHARE, COREDAO_BTC_VALIDATOR_STAKE_SHARE,
        COREDAO_CORE_VALIDATOR_STAKE_IN, COREDAO_CORE_VALIDATOR_STAKE_OUT,
        COREDAO_BTC_VALIDATOR_STAKE_IN, COREDAO_BTC_VALIDATOR_STAKE_OUT,
        COREDAO_TOTAL_CORE_STAKED, COREDAO_TOTAL_BTC_STAKED,
        COREDAO_VALIDATOR_COMMISSION, COREDAO_VALIDATOR_COMMISSION_PEER_MEDIAN,
        COREDAO_CORE_VALIDATOR_TOP1_SHARE, COREDAO_BTC_VALIDATOR_TOP1_SHARE,
    },
    core::{app_context::AppContext, clients::path::Path, exporter::RunnableModule},
};

#[derive(Debug, Clone)]
struct ValidatorStakeInfo {
    core_stake: f64,
    btc_stake: f64,
    core_stake_flows: StakeFlows,
    btc_stake_flows: StakeFlows,
    core_delegators: HashMap<String, f64>, // delegator_address -> stake_amount
    btc_delegators: HashMap<String, f64>,   // delegator_address -> stake_amount
}

#[derive(Debug, Clone, Default)]
struct StakeFlows {
    total_in: f64,
    total_out: f64,
}

pub struct Staking {
    pub app_context: Arc<AppContext>,
    validator_stakes: HashMap<String, ValidatorStakeInfo>,
    last_processed_block: u64,
}

impl Staking {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self {
            app_context,
            validator_stakes: HashMap::new(),
            last_processed_block: 0,
        }
    }

    fn get_contract_addresses(&self) -> (String, String, String, String) {
        // CoreDAO system contract addresses
        (
            "0x0000000000000000000000000000000000001000".to_string(), // ValidatorSet
            "0x0000000000000000000000000000000000001011".to_string(), // CoreAgent
            "0x0000000000000000000000000000000000001014".to_string(), // BitcoinStake
            "0x0000000000000000000000000000000000001005".to_string(), // CandidateHub
        )
    }

    async fn get_latest_block_number(&self) -> Result<u64> {
        let client = self.app_context.rpc.clone().unwrap();
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching latest block number")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing latest block number response")?;

        let block_hex = result
            .get("result")
            .and_then(Value::as_str)
            .context("Invalid block number format")?;

        let block_number = u64::from_str_radix(block_hex.trim_start_matches("0x"), 16)
            .context("Could not parse block number")?;

        Ok(block_number)
    }

    fn get_validator_name(&self, address: &str) -> String {
        // For now, use a shortened version of the address as the validator name
        // In the future, this could be enhanced to fetch actual validator names from contracts
        format!("{}...{}", &address[..6], &address[address.len()-4..])
    }

    async fn get_validator_commissions(&self) -> Result<HashMap<String, f64>> {
        info!("(CoreDAO Staking) Fetching validator commissions");
        
        let client = self.app_context.rpc.clone().unwrap();
        let (_, _, _, candidate_hub_addr) = self.get_contract_addresses();
        
        let mut commissions = HashMap::new();
        
        // First, get the validator list
        let validators = self.get_validators().await?;
        
        for validator_addr in validators.iter() {
            // Step 1: Get the candidate index using operateMap(address)
            let operate_map_selector = "0xc6a9dcc0"; // operateMap(address)
            let addr_param = format!("{:0>64}", validator_addr.trim_start_matches("0x"));
            let operate_data = format!("{}{}", operate_map_selector, addr_param);
            
            let payload = json!({
                "jsonrpc": "2.0",
                "method": "eth_call",
                "params": [{
                    "to": candidate_hub_addr,
                    "data": operate_data
                }, "latest"],
                "id": 1
            });

            let res = client
                .post(Path::from(""), &payload)
                .await
                .context("Error fetching candidate index")?;

            let result: Value = serde_json::from_str(&res)
                .context("Error parsing candidate index response")?;

            if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
                if hex_data != "0x" && !hex_data.contains("revert") && hex_data.len() >= 66 {
                    let index_hex = hex_data.trim_start_matches("0x");
                    if let Ok(index_raw) = u64::from_str_radix(index_hex, 16) {
                        if index_raw > 0 {
                            // Step 2: Get candidate data using candidateSet(index-1)
                            let candidate_set_selector = "0xb894aac5"; // candidateSet(uint256)
                            let index_param = format!("{:0>64x}", index_raw - 1); // Convert to 0-based index
                            let candidate_data = format!("{}{}", candidate_set_selector, index_param);
                            
                            let payload = json!({
                                "jsonrpc": "2.0",
                                "method": "eth_call",
                                "params": [{
                                    "to": candidate_hub_addr,
                                    "data": candidate_data
                                }, "latest"],
                                "id": 1
                            });

                            let res = client
                                .post(Path::from(""), &payload)
                                .await
                                .context("Error fetching candidate data")?;

                            let result: Value = serde_json::from_str(&res)
                                .context("Error parsing candidate data response")?;

                            if let Some(candidate_hex) = result.get("result").and_then(Value::as_str) {
                                if candidate_hex.len() >= 258 { // 0x + 8*32 bytes for Candidate struct
                                    let hex_clean = candidate_hex.trim_start_matches("0x");
                                    
                                    // Extract commissionThousandths (4th field, offset 3*64 = 192)
                                    let commission_hex = &hex_clean[192..256];
                                    if let Ok(commission_raw) = u64::from_str_radix(commission_hex, 16) {
                                        // Convert from thousandths to percentage (divide by 10)
                                        let commission_percent = commission_raw as f64 / 10.0;
                                        commissions.insert(validator_addr.clone(), commission_percent);
                                        info!("(CoreDAO Staking) Validator {} commission: {}% (raw: {})", validator_addr, commission_percent, commission_raw);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(commissions)
    }

    async fn get_current_core_stakes(&self) -> Result<HashMap<String, f64>> {
        info!("(CoreDAO Staking) Fetching current CORE stakes");
        
        let client = self.app_context.rpc.clone().unwrap();
        let (_, core_agent_addr, _, _) = self.get_contract_addresses();
        
        // First get all validators
        let validators = self.get_validators().await?;
        let mut stakes = HashMap::new();
        
        for (index, validator) in validators.iter().enumerate() {
            info!("(CoreDAO Staking) Processing validator {}/{}: {}", index + 1, validators.len(), validator);
            
            // Use the correct function signature for candidateMap(address)
            let data = format!(
                "0x20c94d98000000000000000000000000{}",
                validator.trim_start_matches("0x")
            );
            
            let payload = json!({
                "jsonrpc": "2.0",
                "method": "eth_call",
                "params": [{
                    "to": core_agent_addr,
                    "data": data
                }, "latest"],
                "id": 1
            });

            let res = client
                .post(Path::from(""), &payload)
                .await
                .context("Error fetching CORE stake")?;

            let result: Value = serde_json::from_str(&res)
                .context("Error parsing CORE stake response")?;

            info!("(CoreDAO Staking) Validator {} response: {:?}", validator, result);

            if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
                if hex_data != "0x" && !hex_data.contains("revert") && hex_data.len() >= 130 {
                    // Parse the candidateMap struct
                    // candidateMap returns a Candidate struct where:
                    // - amount at slot 0 (32 bytes) - staked amount on last snapshot
                    // - realtimeAmount at slot 1 (32 bytes) - current realtime staked amount
                    
                    // Try to get realtimeAmount first (offset 66), then fallback to amount (offset 2)
                    for (offset, desc) in [(66, "realtimeAmount"), (2, "amount")] {
                        if hex_data.len() >= offset + 64 {
                            let amount_hex = &hex_data[offset..offset + 64];
                            if let Ok(amount) = u128::from_str_radix(amount_hex, 16) {
                                if amount > 0 {
                                    let stake_amount = amount as f64 / 1e18; // Convert from wei to CORE
                                    stakes.insert(validator.clone(), stake_amount);
                                    info!("(CoreDAO Staking) Validator {} CORE stake ({}): {}", validator, desc, stake_amount);
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    info!("(CoreDAO Staking) No data or error for validator {}: {}", validator, hex_data);
                }
            }
        }
        
        info!("(CoreDAO Staking) Found {} validators with CORE stakes", stakes.len());
        Ok(stakes)
    }

    async fn get_current_btc_stakes(&self) -> Result<HashMap<String, f64>> {
        info!("(CoreDAO Staking) Fetching current BTC stakes");
        
        let client = self.app_context.rpc.clone().unwrap();
        
        // First get all validators
        let validators = self.get_validators().await?;
        let mut stakes = HashMap::new();
        
        for (index, validator) in validators.iter().enumerate() {
            info!("(CoreDAO Staking) Processing BTC validator {}/{}: {}", index + 1, validators.len(), validator);
            
            // Use the correct function signature for candidateMap(address)
            let data = format!(
                "0x20c94d98000000000000000000000000{}",
                validator.trim_start_matches("0x")
            );
            
            let payload = json!({
                "jsonrpc": "2.0",
                "method": "eth_call",
                "params": [{
                    "to": "0x0000000000000000000000000000000000001014", // BTC_STAKE_ADDR
                    "data": data
                }, "latest"],
                "id": 1
            });

            let res = client
                .post(Path::from(""), &payload)
                .await
                .context("Error fetching BTC stake")?;

            let result: Value = serde_json::from_str(&res)
                .context("Error parsing BTC stake response")?;

            info!("(CoreDAO Staking) BTC Validator {} response: {:?}", validator, result);

            if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
                if hex_data != "0x" && !hex_data.contains("revert") && hex_data.len() >= 130 {
                    // Parse the candidateMap struct for BitcoinStake
                    // candidateMap returns a Candidate struct where:
                    // - stakedAmount at slot 0 (32 bytes) - staked amount on last snapshot
                    // - realtimeAmount at slot 1 (32 bytes) - current realtime staked amount
                    
                    // Try to get realtimeAmount first (offset 66), then fallback to stakedAmount (offset 2)
                    for (offset, desc) in [(66, "realtimeAmount"), (2, "stakedAmount")] {
                        if hex_data.len() >= offset + 64 {
                            let amount_hex = &hex_data[offset..offset + 64];
                            if let Ok(amount) = u128::from_str_radix(amount_hex, 16) {
                                if amount > 0 {
                                    let stake_amount = amount as f64 / 1e8; // Convert from satoshi to BTC
                                    stakes.insert(validator.clone(), stake_amount);
                                    info!("(CoreDAO Staking) Validator {} BTC stake ({}): {}", validator, desc, stake_amount);
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    info!("(CoreDAO Staking) No BTC data or error for validator {}: {}", validator, hex_data);
                }
            }
        }
        
        info!("(CoreDAO Staking) Found {} validators with BTC stakes", stakes.len());
        Ok(stakes)
    }

    async fn get_validators(&self) -> Result<Vec<String>> {
        let client = self.app_context.rpc.clone().unwrap();
        let (validator_set_addr, _, _, _) = self.get_contract_addresses();
        
        // Use contract ValidatorSet.sol using function getValidatorOps()
        let data = "0x93f2d404";

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": validator_set_addr,
                "data": data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching validators")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing validators response")?;

        let hex_data = result
            .get("result")
            .and_then(Value::as_str)
            .context("Invalid result format")?
            .trim_start_matches("0x")
            .to_string();

        // Parse the ABI-encoded array of addresses
        let length_hex = &hex_data.get(64..128).context("Could not get length hex")?;
        let length = u64::from_str_radix(length_hex, 16).unwrap_or(0) as usize;
        let mut validators = Vec::with_capacity(length);

        for i in 0..length {
            let start = 128 + i * 64;
            if start + 64 <= hex_data.len() {
                let address = format!("0x{}", &hex_data[start + 24..start + 64]);
                validators.push(address);
            }
        }

        Ok(validators)
    }

    async fn process_staking_events(&mut self, from_block: u64, to_block: u64) -> Result<()> {
        info!("(CoreDAO Staking) Processing staking events from block {} to {}", from_block, to_block);
        
        // First, diagnose what events are available
        self.diagnose_staking_events(from_block, to_block).await?;
        
        // Process CORE staking events
        self.process_core_events(from_block, to_block).await?;
        
        // Process BTC staking events  
        self.process_btc_events(from_block, to_block).await?;
        
        Ok(())
    }

    async fn process_core_events(&mut self, from_block: u64, to_block: u64) -> Result<()> {
        // Get the correct event signatures for CoreAgent events
        let (delegated_sig, undelegated_sig, transferred_sig) = self.get_core_event_signatures();
        let (_, core_agent_addr, _, _) = self.get_contract_addresses();

        for (event_name, event_sig) in [
            ("delegated", delegated_sig), 
            ("undelegated", undelegated_sig), 
            ("transferred", transferred_sig)
        ] {
            info!("(CoreDAO Staking) Searching for {} events from block {} to {} with signature {}", event_name, from_block, to_block, event_sig);
            
            // Clone the client reference to avoid borrowing issues
            let client = self.app_context.rpc.clone().unwrap();
            
            let payload = json!({
                "jsonrpc": "2.0",
                "method": "eth_getLogs",
                "params": [{
                    "fromBlock": format!("0x{:x}", from_block),
                    "toBlock": format!("0x{:x}", to_block),
                    "address": core_agent_addr,
                    "topics": [event_sig]
                }],
                "id": 1
            });

            let res = client
                .post(Path::from(""), &payload)
                .await
                .context("Error fetching CORE events")?;

            let result: Value = serde_json::from_str(&res)
                .context("Error parsing CORE events response")?;

            if let Some(logs) = result.get("result").and_then(Value::as_array) {
                info!("(CoreDAO Staking) Found {} {} events", logs.len(), event_name);
                for log in logs {
                    self.process_core_event(log, &event_name).await?;
                }
            } else {
                info!("(CoreDAO Staking) No {} events found or error in response: {:?}", event_name, result);
            }
        }
        
        Ok(())
    }

    async fn process_btc_events(&mut self, from_block: u64, to_block: u64) -> Result<()> {
        // Get the correct event signatures for BitcoinStake events
        let (delegated_sig, undelegated_sig, transferred_sig) = self.get_btc_event_signatures();
        let (_, _, btc_stake_addr, _) = self.get_contract_addresses();

        for (event_name, event_sig) in [
            ("delegated", delegated_sig), 
            ("undelegated", undelegated_sig), 
            ("transferred", transferred_sig)
        ] {
            // Clone the client reference to avoid borrowing issues
            let client = self.app_context.rpc.clone().unwrap();
            
            let payload = json!({
                "jsonrpc": "2.0",
                "method": "eth_getLogs",
                "params": [{
                    "fromBlock": format!("0x{:x}", from_block),
                    "toBlock": format!("0x{:x}", to_block),
                    "address": btc_stake_addr,
                    "topics": [event_sig]
                }],
                "id": 1
            });

            let res = client
                .post(Path::from(""), &payload)
                .await
                .context("Error fetching BTC events")?;

            let result: Value = serde_json::from_str(&res)
                .context("Error parsing BTC events response")?;

            if let Some(logs) = result.get("result").and_then(Value::as_array) {
                for log in logs {
                    self.process_btc_event(log, &event_name).await?;
                }
            }
        }
        
        Ok(())
    }

    async fn process_core_event(&mut self, log: &Value, event_name: &str) -> Result<()> {
        // Extract candidate (validator) address, delegator address, and amount from log data
        if let (Some(topics), Some(data)) = (log.get("topics").and_then(Value::as_array), log.get("data").and_then(Value::as_str)) {
            if topics.len() >= 3 {
                // For CoreAgent events:
                // topics[0] = event signature
                // topics[1] = candidate address (indexed)
                // topics[2] = delegator address (indexed)
                // data = amount, realtimeAmount (for delegatedCoin), or just amount (for undelegatedCoin)
                
                if let Some(candidate_hex) = topics[1].as_str() {
                    let candidate = format!("0x{}", &candidate_hex[26..]); // Last 20 bytes
                    
                    // Extract delegator address
                    let delegator = if let Some(delegator_hex) = topics[2].as_str() {
                        format!("0x{}", &delegator_hex[26..]) // Last 20 bytes
                    } else {
                        "unknown".to_string()
                    };
                    
                    // Amount is in the data field - get first 32 bytes for amount
                    if data.len() >= 66 {
                        let amount_hex = &data[2..66]; // Skip 0x, get first 32 bytes
                        if let Ok(amount) = u128::from_str_radix(amount_hex, 16) {
                            let amount_core = amount as f64 / 1e18;
                            
                            match event_name {
                                "delegated" => {
                                    let validator_info = self.validator_stakes.entry(candidate.clone()).or_default();
                                    validator_info.core_stake_flows.total_in += amount_core;
                                    
                                    // Track delegator stake
                                    *validator_info.core_delegators.entry(delegator.clone()).or_insert(0.0) += amount_core;
                                    
                                    // Increment the Prometheus counter directly
                                    let validator_name = self.get_validator_name(&candidate);
                                    let fires_alerts = self.app_context.config.general.alerting.validators.contains(&candidate).to_string();
                                    COREDAO_CORE_VALIDATOR_STAKE_IN
                                        .with_label_values(&[
                                            &candidate,
                                            &validator_name,
                                            &self.app_context.chain_id,
                                            &self.app_context.config.general.network,
                                            &fires_alerts,
                                        ])
                                        .inc_by(amount_core);
                                    
                                    info!("(CoreDAO Staking) CORE delegated: {} CORE to validator {} by {}", amount_core, candidate, delegator);
                                }
                                "undelegated" => {
                                    let validator_info = self.validator_stakes.entry(candidate.clone()).or_default();
                                    validator_info.core_stake_flows.total_out += amount_core;
                                    
                                    // Track delegator unstaking (subtract from their balance)
                                    if let Some(delegator_balance) = validator_info.core_delegators.get_mut(&delegator) {
                                        *delegator_balance = (*delegator_balance - amount_core).max(0.0);
                                        if *delegator_balance == 0.0 {
                                            validator_info.core_delegators.remove(&delegator);
                                        }
                                    }
                                    
                                    // Increment the Prometheus counter directly
                                    let validator_name = self.get_validator_name(&candidate);
                                    let fires_alerts = self.app_context.config.general.alerting.validators.contains(&candidate).to_string();
                                    COREDAO_CORE_VALIDATOR_STAKE_OUT
                                        .with_label_values(&[
                                            &candidate,
                                            &validator_name,
                                            &self.app_context.chain_id,
                                            &self.app_context.config.general.network,
                                            &fires_alerts,
                                        ])
                                        .inc_by(amount_core);
                                    
                                    info!("(CoreDAO Staking) CORE undelegated: {} CORE from validator {} by {}", amount_core, candidate, delegator);
                                }
                                "transferred" => {
                                    // For transfers, we track both out from source and in to target
                                    // This log shows the target validator, need to check if there's a source in topics[1]
                                    if topics.len() >= 4 {
                                        // topics[3] might be target candidate for transfer events
                                        if let Some(target_hex) = topics[3].as_str() {
                                            let target = format!("0x{}", &target_hex[26..]);
                                            
                                            // Handle source validator first
                                            {
                                                let source_info = self.validator_stakes.entry(candidate.clone()).or_default();
                                                source_info.core_stake_flows.total_out += amount_core;
                                                
                                                // Remove stake from source delegator
                                                if let Some(delegator_balance) = source_info.core_delegators.get_mut(&delegator) {
                                                    *delegator_balance = (*delegator_balance - amount_core).max(0.0);
                                                    if *delegator_balance == 0.0 {
                                                        source_info.core_delegators.remove(&delegator);
                                                    }
                                                }
                                                
                                                let source_name = self.get_validator_name(&candidate);
                                                let source_alerts = self.app_context.config.general.alerting.validators.contains(&candidate).to_string();
                                                COREDAO_CORE_VALIDATOR_STAKE_OUT
                                                    .with_label_values(&[
                                                        &candidate,
                                                        &source_name,
                                                        &self.app_context.chain_id,
                                                        &self.app_context.config.general.network,
                                                        &source_alerts,
                                                    ])
                                                    .inc_by(amount_core);
                                            }
                                            
                                            // Handle target validator
                                            {
                                                let target_info = self.validator_stakes.entry(target.clone()).or_default();
                                                target_info.core_stake_flows.total_in += amount_core;
                                                
                                                // Add stake to target delegator
                                                *target_info.core_delegators.entry(delegator.clone()).or_insert(0.0) += amount_core;
                                                
                                                let target_name = self.get_validator_name(&target);
                                                let target_alerts = self.app_context.config.general.alerting.validators.contains(&target).to_string();
                                                COREDAO_CORE_VALIDATOR_STAKE_IN
                                                    .with_label_values(&[
                                                        &target,
                                                        &target_name,
                                                        &self.app_context.chain_id,
                                                        &self.app_context.config.general.network,
                                                        &target_alerts,
                                                    ])
                                                    .inc_by(amount_core);
                                            }
                                            
                                            info!("(CoreDAO Staking) CORE transferred: {} CORE from {} to {} by {}", amount_core, candidate, target, delegator);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn process_btc_event(&mut self, log: &Value, event_name: &str) -> Result<()> {
        // Extract candidate (validator) address and amount from log data
        if let (Some(topics), Some(data)) = (log.get("topics").and_then(Value::as_array), log.get("data").and_then(Value::as_str)) {
            
            match event_name {
                "delegated" => {
                    // For BTC delegated event:
                    // topics[0] = event signature  
                    // topics[1] = txid (indexed)
                    // topics[2] = candidate address (indexed)
                    // topics[3] = delegator address (indexed)
                    // data = script, outputIndex, amount, fee
                    
                    if topics.len() >= 3 {
                        if let Some(candidate_hex) = topics[2].as_str() {
                            let candidate = format!("0x{}", &candidate_hex[26..]); // Last 20 bytes
                            
                            // For BTC delegated, amount is at a specific offset in data
                            // Need to parse the ABI-encoded data more carefully
                            // Simplified: assume amount is in the expected position
                            if data.len() >= 258 { // 0x + multiple 32-byte fields
                                // Amount (uint64) would be at offset after script and outputIndex
                                let amount_hex = &data[194..226]; // Approximate position
                                if let Ok(amount) = u64::from_str_radix(amount_hex, 16) {
                                    let amount_btc = amount as f64 / 1e8; // Convert from satoshi
                                    
                                    let validator_info = self.validator_stakes.entry(candidate.clone()).or_default();
                                    validator_info.btc_stake_flows.total_in += amount_btc;
                                    
                                    // Increment the Prometheus counter directly
                                    let validator_name = self.get_validator_name(&candidate);
                                    let fires_alerts = self.app_context.config.general.alerting.validators.contains(&candidate).to_string();
                                    COREDAO_BTC_VALIDATOR_STAKE_IN
                                        .with_label_values(&[
                                            &candidate,
                                            &validator_name,
                                            &self.app_context.chain_id,
                                            &self.app_context.config.general.network,
                                            &fires_alerts,
                                        ])
                                        .inc_by(amount_btc);
                                    
                                    info!("(CoreDAO Staking) BTC delegated: {} BTC to validator {}", amount_btc, candidate);
                                }
                            }
                        }
                    }
                }
                "undelegated" => {
                    // For BTC undelegated event:
                    // topics[0] = event signature
                    // topics[1] = outpointHash (indexed)
                    // topics[2] = outpointIndex (indexed)  
                    // data = usedTxid
                    // This event doesn't directly contain amount/candidate, would need to track from delegated events
                    info!("(CoreDAO Staking) BTC undelegated event detected");
                }
                "transferred" => {
                    // For BTC transferred event:
                    // topics[0] = event signature
                    // topics[1] = txid (indexed)
                    // data = sourceCandidate, targetCandidate, delegator, amount
                    
                    if data.len() >= 162 { // 0x + 5*32 bytes
                        let source_hex = &data[26..66]; // sourceCandidate  
                        let target_hex = &data[90..130]; // targetCandidate
                        let amount_hex = &data[130..162]; // amount
                        
                        if let (Ok(amount), source_ok, target_ok) = (
                            u128::from_str_radix(amount_hex, 16),
                            source_hex.len() == 40,
                            target_hex.len() == 40
                        ) {
                            if source_ok && target_ok {
                                let source = format!("0x{}", source_hex);
                                let target = format!("0x{}", target_hex);
                                let amount_btc = amount as f64 / 1e8;
                                
                                // Handle source validator first
                                {
                                    let source_info = self.validator_stakes.entry(source.clone()).or_default();
                                    source_info.btc_stake_flows.total_out += amount_btc;
                                    
                                    let source_name = self.get_validator_name(&source);
                                    let source_alerts = self.app_context.config.general.alerting.validators.contains(&source).to_string();
                                    COREDAO_BTC_VALIDATOR_STAKE_OUT
                                        .with_label_values(&[
                                            &source,
                                            &source_name,
                                            &self.app_context.chain_id,
                                            &self.app_context.config.general.network,
                                            &source_alerts,
                                        ])
                                        .inc_by(amount_btc);
                                }
                                
                                // Handle target validator
                                {
                                    let target_info = self.validator_stakes.entry(target.clone()).or_default();
                                    target_info.btc_stake_flows.total_in += amount_btc;
                                    
                                    let target_name = self.get_validator_name(&target);
                                    let target_alerts = self.app_context.config.general.alerting.validators.contains(&target).to_string();
                                    COREDAO_BTC_VALIDATOR_STAKE_IN
                                        .with_label_values(&[
                                            &target,
                                            &target_name,
                                            &self.app_context.chain_id,
                                            &self.app_context.config.general.network,
                                            &target_alerts,
                                        ])
                                        .inc_by(amount_btc);
                                }
                                
                                info!("(CoreDAO Staking) BTC transferred: {} BTC from {} to {}", amount_btc, source, target);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn update_current_stakes(&mut self) -> Result<()> {
        let core_stakes = self.get_current_core_stakes().await?;
        let btc_stakes = self.get_current_btc_stakes().await?;
        let commissions = self.get_validator_commissions().await?;

        // Calculate totals
        let total_core: f64 = core_stakes.values().sum();
        let total_btc: f64 = btc_stakes.values().sum();

        // Calculate commission median for peer comparison
        let mut commission_values: Vec<f64> = commissions.values().copied().collect();
        commission_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let commission_median = if commission_values.is_empty() {
            0.0
        } else if commission_values.len() % 2 == 0 {
            let mid = commission_values.len() / 2;
            (commission_values[mid - 1] + commission_values[mid]) / 2.0
        } else {
            commission_values[commission_values.len() / 2]
        };

        // Set commission peer median metric
        COREDAO_VALIDATOR_COMMISSION_PEER_MEDIAN
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(commission_median);

        // Update metrics for each validator
        for (validator, core_stake) in core_stakes {
            let btc_stake = btc_stakes.get(&validator).copied().unwrap_or(0.0);
            
            let core_share = if total_core > 0.0 { core_stake / total_core * 100.0 } else { 0.0 };
            let btc_share = if total_btc > 0.0 { btc_stake / total_btc * 100.0 } else { 0.0 };
            
            let fires_alerts = self.app_context.config.general.alerting.validators.contains(&validator).to_string();
            let validator_name = self.get_validator_name(&validator);
            
            // Set commission metric
            if let Some(commission) = commissions.get(&validator) {
                COREDAO_VALIDATOR_COMMISSION
                    .with_label_values(&[
                        &validator,
                        &validator_name,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        &fires_alerts,
                    ])
                    .set(*commission);
            }
            
            // Set market share metrics
            COREDAO_CORE_VALIDATOR_STAKE_SHARE
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(core_share);

            COREDAO_BTC_VALIDATOR_STAKE_SHARE
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(btc_share);

            // Initialize validator info if it doesn't exist (ensures stake flow metrics are always set)
            let validator_info = self.validator_stakes.entry(validator.clone()).or_default();
            validator_info.core_stake = core_stake;
            validator_info.btc_stake = btc_stake;

            // Calculate top-1 delegator share for CORE
            let core_top1_share = if !validator_info.core_delegators.is_empty() && core_stake > 0.0 {
                let max_core_delegator = validator_info.core_delegators.values()
                    .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .copied()
                    .unwrap_or(0.0);
                (max_core_delegator / core_stake) * 100.0
            } else {
                0.0
            };

            // Calculate top-1 delegator share for BTC
            let btc_top1_share = if !validator_info.btc_delegators.is_empty() && btc_stake > 0.0 {
                let max_btc_delegator = validator_info.btc_delegators.values()
                    .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .copied()
                    .unwrap_or(0.0);
                (max_btc_delegator / btc_stake) * 100.0
            } else {
                0.0
            };

            // Set delegator concentration metrics
            COREDAO_CORE_VALIDATOR_TOP1_SHARE
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(core_top1_share);

            COREDAO_BTC_VALIDATOR_TOP1_SHARE
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(btc_top1_share);

            // Ensure stake flow counters are initialized (sets to current total if not yet set)
            // This ensures all validators appear in metrics even if they have zero flow activity
            let current_core_in = COREDAO_CORE_VALIDATOR_STAKE_IN
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ]).get();
            
            if current_core_in == 0.0 {
                COREDAO_CORE_VALIDATOR_STAKE_IN
                    .with_label_values(&[
                        &validator,
                        &validator_name,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        &fires_alerts,
                    ]).inc_by(validator_info.core_stake_flows.total_in);
            }

            let current_core_out = COREDAO_CORE_VALIDATOR_STAKE_OUT
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ]).get();
            
            if current_core_out == 0.0 {
                COREDAO_CORE_VALIDATOR_STAKE_OUT
                    .with_label_values(&[
                        &validator,
                        &validator_name,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        &fires_alerts,
                    ]).inc_by(validator_info.core_stake_flows.total_out);
            }

            let current_btc_in = COREDAO_BTC_VALIDATOR_STAKE_IN
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ]).get();
            
            if current_btc_in == 0.0 {
                COREDAO_BTC_VALIDATOR_STAKE_IN
                    .with_label_values(&[
                        &validator,
                        &validator_name,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        &fires_alerts,
                    ]).inc_by(validator_info.btc_stake_flows.total_in);
            }

            let current_btc_out = COREDAO_BTC_VALIDATOR_STAKE_OUT
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ]).get();
            
            if current_btc_out == 0.0 {
                COREDAO_BTC_VALIDATOR_STAKE_OUT
                    .with_label_values(&[
                        &validator,
                        &validator_name,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        &fires_alerts,
                    ]).inc_by(validator_info.btc_stake_flows.total_out);
            }

            // Log current stake flow totals for debugging
            info!("(CoreDAO Staking) Validator {}: CORE flows (total_in={}, total_out={}), BTC flows (total_in={}, total_out={}), CORE top1: {}%, BTC top1: {}%, Commission: {}%", 
                validator, validator_info.core_stake_flows.total_in, validator_info.core_stake_flows.total_out,
                validator_info.btc_stake_flows.total_in, validator_info.btc_stake_flows.total_out,
                core_top1_share, btc_top1_share, commissions.get(&validator).unwrap_or(&0.0));
        }

        // Set total stake metrics
        COREDAO_TOTAL_CORE_STAKED
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(total_core);

        COREDAO_TOTAL_BTC_STAKED
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(total_btc);

        Ok(())
    }

    fn get_core_event_signatures(&self) -> (String, String, String) {
        // These are the correct keccak256 hashes of the CoreAgent event signatures:
        // delegatedCoin(address,address,uint256,uint256)
        // undelegatedCoin(address,address,uint256)  
        // transferredCoin(address,address,address,uint256,uint256)
        
        let delegated = "0x69e36aaf9558a3c30415c0a2bc6cb4c2d592c041a0718697bf69c2e7c7e0bdac".to_string();
        let undelegated = "0x888585afd9421c43b48dc50229aa045dd1048c03602b46c83ad2aa36be798d42".to_string();
        let transferred = "0x037bbd0a1321bedfe51f505a5e6cede0b346e57521d957f9e76cb348b7758cb1".to_string();
        
        (delegated, undelegated, transferred)
    }

    fn get_btc_event_signatures(&self) -> (String, String, String) {
        // These are the correct keccak256 hashes of the BitcoinStake event signatures:
        // delegated(bytes32,address,address,bytes,uint32,uint64,uint256)
        // undelegated(bytes32,uint32,bytes32)
        // transferredBtc(bytes32,address,address,address,uint256)
        
        let delegated = "0x3391934a441f8a4f5bd3ffdc8b4c59b386061114e16b83d51cc73b1e41c0c0a0".to_string();
        let undelegated = "0x11e4685d914d513c078f2520ce18170550bf421495a0b11d9a2e82b0ac02ac32".to_string();
        let transferred = "0x131a10ab89910bd3a30ed9bbf71f1bce939e3d654a7cd7474ca5887eab499c82".to_string();
        
        (delegated, undelegated, transferred)
    }

    async fn diagnose_staking_events(&self, from_block: u64, to_block: u64) -> Result<()> {
        info!("(CoreDAO Staking) Diagnosing staking events from block {} to {}", from_block, to_block);
        
        let client = self.app_context.rpc.clone().unwrap();
        let (_, core_agent_addr, btc_stake_addr, _) = self.get_contract_addresses();
        
        // Search for ALL events from CoreAgent contract
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_getLogs",
            "params": [{
                "fromBlock": format!("0x{:x}", from_block),
                "toBlock": format!("0x{:x}", to_block),
                "address": core_agent_addr,
                "topics": []
            }],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching CoreAgent events")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing CoreAgent events response")?;

        if let Some(logs) = result.get("result").and_then(Value::as_array) {
            info!("(CoreDAO Staking) Found {} total events from CoreAgent", logs.len());
            for (i, log) in logs.iter().take(5).enumerate() {
                if let Some(topics) = log.get("topics").and_then(Value::as_array) {
                    if !topics.is_empty() {
                        info!("(CoreDAO Staking) Event {}: {}", i + 1, topics[0].as_str().unwrap_or("unknown"));
                    }
                }
            }
        }
        
        // Search for ALL events from BitcoinStake contract
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_getLogs",
            "params": [{
                "fromBlock": format!("0x{:x}", from_block),
                "toBlock": format!("0x{:x}", to_block),
                "address": btc_stake_addr,
                "topics": []
            }],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching BitcoinStake events")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing BitcoinStake events response")?;

        if let Some(logs) = result.get("result").and_then(Value::as_array) {
            info!("(CoreDAO Staking) Found {} total events from BitcoinStake", logs.len());
            for (i, log) in logs.iter().take(5).enumerate() {
                if let Some(topics) = log.get("topics").and_then(Value::as_array) {
                    if !topics.is_empty() {
                        info!("(CoreDAO Staking) Event {}: {}", i + 1, topics[0].as_str().unwrap_or("unknown"));
                    }
                }
            }
        }
        
        Ok(())
    }
}

impl Default for ValidatorStakeInfo {
    fn default() -> Self {
        Self {
            core_stake: 0.0,
            btc_stake: 0.0,
            core_stake_flows: StakeFlows::default(),
            btc_stake_flows: StakeFlows::default(),
            core_delegators: HashMap::new(),
            btc_delegators: HashMap::new(),
        }
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.rpc.is_none() {
        anyhow::bail!("Config is missing RPC node pool");
    }
    Ok(Box::new(Staking::new(app_context)))
}

#[async_trait]
impl RunnableModule for Staking {
    async fn run(&mut self) -> Result<()> {
        let latest_block = self.get_latest_block_number().await
            .context("Failed to get latest block number")?;

        // If first run, start from recent blocks to avoid processing entire history
        if self.last_processed_block == 0 {
            self.last_processed_block = latest_block.saturating_sub(10000); // Start from 10000 blocks ago to capture more events
        }

        // Process new blocks for staking events
        if latest_block > self.last_processed_block {
            self.process_staking_events(self.last_processed_block + 1, latest_block).await
                .context("Failed to process staking events")?;
            self.last_processed_block = latest_block;
        }

        // Update current stake amounts and market shares
        self.update_current_stakes().await
            .context("Failed to update current stakes")?;

        Ok(())
    }

    fn name(&self) -> &'static str {
        "CoreDAO Staking"
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.coredao.staking.interval as u64
        )
    }
}
