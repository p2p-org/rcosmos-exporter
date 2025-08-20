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
        COREDAO_CORE_VALIDATOR_DELEGATOR_APY, COREDAO_BTC_VALIDATOR_DELEGATOR_APY,
        COREDAO_CORE_VALIDATOR_UNCLAIMED_REWARD_RATIO, COREDAO_BTC_VALIDATOR_UNCLAIMED_REWARD_RATIO,
        COREDAO_CORE_VALIDATOR_ROUND_REWARD_TOTAL, COREDAO_BTC_VALIDATOR_ROUND_REWARD_TOTAL,
        COREDAO_BTC_VALIDATOR_STAKE_EXPIRATION_TIMESTAMP,
        COREDAO_VALIDATOR_SLASH_EVENTS_TOTAL, COREDAO_VALIDATOR_PENALTY_AMOUNT_TOTAL,
        COREDAO_CORE_VALIDATOR_CURRENT_STAKE, COREDAO_BTC_VALIDATOR_CURRENT_STAKE,
    },
    blockchains::coredao::validator::ValidatorFetcher,
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
    validator_fetcher: ValidatorFetcher,
}

impl Staking {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self {
            app_context: app_context.clone(),
            validator_stakes: HashMap::new(),
            last_processed_block: 0,
            validator_fetcher: ValidatorFetcher::new(app_context.clone()),
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
        // Attempt to pull from fetcher cache
        if let Ok(guard) = self.validator_fetcher.cache.try_read() {
            if let Some(cached) = guard.as_ref() {
                if let Some(v) = cached.validators.iter().find(|v| v.address.eq_ignore_ascii_case(address)) {
                    return v.name.clone();
                }
            }
        }
        // No cached name available; fallback to address short form
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

        // Process StakeHub roundReward events to accumulate validator rewards
        self.process_stakehub_round_rewards(from_block, to_block).await?;
        
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

    async fn process_stakehub_round_rewards(&mut self, from_block: u64, to_block: u64) -> Result<()> {
        // roundReward(string name, uint256 round, address[] validator, uint256[] amount)
        // topic0 = keccak('roundReward(string,uint256,address[],uint256[])')
        let topic0 = "0xd91b286bba7f90b8abe1c6445f75d50b2b4f2790251e196e83922a94e2ba4a7c";
        let stakehub_addr = "0x0000000000000000000000000000000000001010";
        let client = self.app_context.rpc.clone().unwrap();
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_getLogs",
            "params": [{
                "fromBlock": format!("0x{:x}", from_block),
                "toBlock": format!("0x{:x}", to_block),
                "address": stakehub_addr,
                "topics": [topic0]
            }],
            "id": 1
        });
        let res = client.post(Path::from(""), &payload).await.context("Error fetching StakeHub roundReward events")?;
        let result: Value = serde_json::from_str(&res).context("Error parsing StakeHub roundReward response")?;
        if let Some(logs) = result.get("result").and_then(Value::as_array) {
            for log in logs {
                self.process_round_reward_event(log).await?;
            }
        }
        Ok(())
    }

    async fn process_round_reward_event(&mut self, log: &Value) -> Result<()> {
        // topics[1] = keccak(name) for indexed string 'name' (contract emits indexed string)
        // data contains round, arrays; but we can parse validators and amounts from ABI-encoded data
        if let (Some(topics), Some(data)) = (log.get("topics").and_then(Value::as_array), log.get("data").and_then(Value::as_str)) {
            if topics.len() >= 2 {
                let asset_topic = topics[1].as_str().unwrap_or("");
                let is_core = asset_topic.eq_ignore_ascii_case("0x907208bc2088fa777f18b43edd8b766e7243504cf8497f7ed936c65c7a446bbc");
                let is_btc = asset_topic.eq_ignore_ascii_case("0xe98e2830be1a7e4156d656a7505e65d08c67660dc618072422e9c78053c261e9");
                if !is_core && !is_btc { return Ok(()); }

                // Decode ABI dynamic: round (uint256), validators (address[]), amounts (uint256[])
                let bytes = hex::decode(data.trim_start_matches("0x")).unwrap_or_default();
                if bytes.len() < 32 * 4 { return Ok(()); }
                // Offsets
                let round = u128::from_str_radix(&hex::encode(&bytes[0..32]), 16).unwrap_or(0) as u64;
                let validators_off = usize::from_str_radix(&hex::encode(&bytes[32..64]), 16).unwrap_or(0);
                let amounts_off = usize::from_str_radix(&hex::encode(&bytes[64..96]), 16).unwrap_or(0);
                let base = 0;
                // Validators array
                let v_base = base + validators_off;
                if bytes.len() < v_base + 32 { return Ok(()); }
                let v_len = usize::from_str_radix(&hex::encode(&bytes[v_base..v_base+32]), 16).unwrap_or(0);
                // Amounts array
                let a_base = base + amounts_off;
                if bytes.len() < a_base + 32 { return Ok(()); }
                let a_len = usize::from_str_radix(&hex::encode(&bytes[a_base..a_base+32]), 16).unwrap_or(0);
                if v_len == 0 || a_len == 0 || v_len != a_len { return Ok(()); }

                for i in 0..v_len {
                    let v_start = v_base + 32 + i * 32;
                    let a_start = a_base + 32 + i * 32;
                    if bytes.len() < v_start + 32 || bytes.len() < a_start + 32 { break; }
                    let addr_hex = &hex::encode(&bytes[v_start+12..v_start+32]);
                    let validator = format!("0x{}", addr_hex);
                    let amount_hex = &hex::encode(&bytes[a_start..a_start+32]);
                    let amount_wei = u128::from_str_radix(amount_hex, 16).unwrap_or(0) as f64;
                    let amount_core = amount_wei / 1e18;
                    let validator_name = self.get_validator_name(&validator);
                    let fires_alerts = self.app_context.config.general.alerting.validators.contains(&validator).to_string();
                    if is_core {
                        COREDAO_CORE_VALIDATOR_ROUND_REWARD_TOTAL
                            .with_label_values(&[&validator, &validator_name, &self.app_context.chain_id, &self.app_context.config.general.network, &fires_alerts])
                            .inc_by(amount_core);
                    } else if is_btc {
                        COREDAO_BTC_VALIDATOR_ROUND_REWARD_TOTAL
                            .with_label_values(&[&validator, &validator_name, &self.app_context.chain_id, &self.app_context.config.general.network, &fires_alerts])
                            .inc_by(amount_core);
                    }
                }
                let _ = round; // not used further now
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

            // Set current stake absolute values
            COREDAO_CORE_VALIDATOR_CURRENT_STAKE
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(core_stake);

            COREDAO_BTC_VALIDATOR_CURRENT_STAKE
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(btc_stake);

            // Initialize event-based reward counters so they appear in /metrics even before first event
            COREDAO_CORE_VALIDATOR_ROUND_REWARD_TOTAL
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .inc_by(0.0);
            COREDAO_BTC_VALIDATOR_ROUND_REWARD_TOTAL
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .inc_by(0.0);

            // Initialize validator info if it doesn't exist (ensures stake flow metrics are always set)
            let validator_info = self.validator_stakes.entry(validator.clone()).or_default();
            validator_info.core_stake = core_stake;
            validator_info.btc_stake = btc_stake;

            // Calculate top-1 delegator share for CORE and capture address
            let mut core_top1_addr: String = String::from("");
            let core_top1_share = if !validator_info.core_delegators.is_empty() && core_stake > 0.0 {
                let (addr, amount) = validator_info.core_delegators.iter()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(k, v)| (k.clone(), *v))
                    .unwrap_or((String::new(), 0.0));
                core_top1_addr = addr;
                (amount / core_stake) * 100.0
            } else {
                0.0
            };

            // Calculate top-1 delegator share for BTC and capture address
            let mut btc_top1_addr: String = String::from("");
            let btc_top1_share = if !validator_info.btc_delegators.is_empty() && btc_stake > 0.0 {
                let (addr, amount) = validator_info.btc_delegators.iter()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(k, v)| (k.clone(), *v))
                    .unwrap_or((String::new(), 0.0));
                btc_top1_addr = addr;
                (amount / btc_stake) * 100.0
            } else {
                0.0
            };

            // Set delegator concentration metrics
            COREDAO_CORE_VALIDATOR_TOP1_SHARE
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &core_top1_addr,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(core_top1_share);

            COREDAO_BTC_VALIDATOR_TOP1_SHARE
                .with_label_values(&[
                    &validator,
                    &validator_name,
                    &btc_top1_addr,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(btc_top1_share);

            // No per-delegator share metrics (reverted to original behavior)

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

    async fn calculate_delegator_apy(&self) -> Result<()> {
        info!("(CoreDAO Staking) Calculating delegator APY");
        
        
        // Ensure validator names are cached before emitting metrics
        let _ = self.validator_fetcher.get_validators().await.ok();
        // Get validators and their commissions
        let validators = self.get_validators().await?;
        let commissions = self.get_validator_commissions().await?;
        
        for validator_addr in validators.iter() {
            let validator_name = self.get_validator_name(validator_addr);
            let fires_alerts = self.app_context.config.general.alerting.validators.contains(validator_addr).to_string();
            
            // Get commission rate (default to 0 if not found)
            let commission_rate = commissions.get(validator_addr).unwrap_or(&0.0);
            
            // Calculate CORE delegator APY
            // This is a simplified calculation - in practice, you'd need to get actual reward rates from contracts
            let core_apy = self.calculate_core_delegator_apy(validator_addr, *commission_rate).await?;
            
            COREDAO_CORE_VALIDATOR_DELEGATOR_APY
                .with_label_values(&[
                    validator_addr,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(core_apy);
            
            // Calculate BTC delegator APY
            let btc_apy = self.calculate_btc_delegator_apy(validator_addr, *commission_rate).await?;
            
            COREDAO_BTC_VALIDATOR_DELEGATOR_APY
                .with_label_values(&[
                    validator_addr,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(btc_apy);
        }
        
        Ok(())
    }

    #[allow(dead_code)]
    async fn calculate_core_delegator_apy(&self, validator_addr: &str, commission_rate: f64) -> Result<f64> {
        let client = self.app_context.rpc.clone().unwrap();
        let (_, core_agent_addr, _, _) = self.get_contract_addresses();

        // APY via accruedRewardMap delta between last two rounds
        let round = self.get_current_round(&client).await.unwrap_or(0);
        let prev_round = round.saturating_sub(1);
        let per_unit_today = self.get_core_accrued_per_unit_at_round(validator_addr, &client, &core_agent_addr, round).await.unwrap_or(0.0);
        let per_unit_prev = self.get_core_accrued_per_unit_at_round(validator_addr, &client, &core_agent_addr, prev_round).await.unwrap_or(0.0);
        let per_day_rate = (per_unit_today - per_unit_prev).max(0.0);
        let annual_rate = per_day_rate * 365.0;
        let apy = (annual_rate * (1.0 - commission_rate / 100.0)) * 100.0;
        
        info!("(CoreDAO Staking) Validator {} CORE APY calculation: per_day={:.6}, annual_rate={:.6}, commission={:.2}%, apy={:.2}%", 
            validator_addr, per_day_rate, annual_rate, commission_rate, apy);
        
        Ok(apy)
    }

    #[allow(dead_code)]
    async fn calculate_btc_delegator_apy(&self, validator_addr: &str, commission_rate: f64) -> Result<f64> {
        let client = self.app_context.rpc.clone().unwrap();
        let (_, _, btc_stake_addr, _) = self.get_contract_addresses();

        let round = self.get_current_round(&client).await.unwrap_or(0);
        let prev_round = round.saturating_sub(1);
        let per_btc_today = self.get_btc_accrued_per_btc_at_round(validator_addr, &client, &btc_stake_addr, round).await.unwrap_or(0.0);
        let per_btc_prev = self.get_btc_accrued_per_btc_at_round(validator_addr, &client, &btc_stake_addr, prev_round).await.unwrap_or(0.0);
        let per_day_rate = (per_btc_today - per_btc_prev).max(0.0);
        let annual_rate = per_day_rate * 365.0;
        let apy = (annual_rate * (1.0 - commission_rate / 100.0)) * 100.0;
        
        info!("(CoreDAO Staking) Validator {} BTC APY calculation: per_day={:.6}, annual_rate={:.6}, commission={:.2}%, apy={:.2}%", 
            validator_addr, per_day_rate, annual_rate, commission_rate, apy);
        
        Ok(apy)
    }

    #[allow(dead_code)]
    async fn get_core_reward_rate(&self, _validator_addr: &str, _client: &crate::core::clients::http_client::NodePool, _core_agent_addr: &str) -> Result<f64> { Ok(0.0) }

    #[allow(dead_code)]
    async fn get_btc_reward_rate(&self, _validator_addr: &str, _client: &crate::core::clients::http_client::NodePool, _btc_stake_addr: &str) -> Result<f64> { Ok(0.0) }

    #[allow(dead_code)]
    async fn get_current_round(&self, client: &crate::core::clients::http_client::NodePool) -> Result<u64> {
        // CandidateHub roundTag() selector: function roundTag() public returns (uint256)
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": "0x0000000000000000000000000000000000001005", // CandidateHub
                "data": "0x8d859f3e"
            }, "latest"],
            "id": 1
        });
        let res = client.post(crate::core::clients::path::Path::from(""), &payload).await?;
        let v: serde_json::Value = serde_json::from_str(&res)?;
        if let Some(hex) = v.get("result").and_then(serde_json::Value::as_str) {
            if hex != "0x" && hex.len() >= 66 {
                let n = u128::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0);
                return Ok(n as u64);
            }
        }
        Ok(0)
    }

    #[allow(dead_code)]
    async fn get_core_accrued_per_unit_at_round(
        &self,
        validator_addr: &str,
        client: &crate::core::clients::http_client::NodePool,
        core_agent_addr: &str,
        round: u64,
    ) -> Result<f64> {
        // accruedRewardMap(address,uint256) public mapping getter
        let mut data = String::from("0x");
        // keccak("accruedRewardMap(address,uint256)")[0..4] = 0x6f6f2f9d (example; relies on solidity encoding for mapping getter)
        // But mapping getters are accessed via slot hashing, not a simple selector. Instead, use exposed helper:
        // CoreAgent has a public view getContinuousRewardEndRoundsByCandidate; however direct mapping getter exists via ABI encoder when compiled.
        // We will call an internal-like binary search wrapper by triggering a read path using viewCollectRewardFromCandidate? Not suitable.
        // So we rely on storage getter for public mapping: function signature selector of accruedRewardMap(address,uint256) is keccak.
        use tiny_keccak::{Hasher, Keccak};
        let sig = "accruedRewardMap(address,uint256)";
        let mut keccak = Keccak::v256();
        let mut out = [0u8; 32];
        keccak.update(sig.as_bytes());
        keccak.finalize(&mut out);
        let selector = &out[0..4];
        data.push_str(&hex::encode(selector));
        // Append address (32 bytes)
        let mut addr = validator_addr.trim_start_matches("0x").to_lowercase();
        addr = format!("{:0>64}", addr);
        data.push_str(&addr);
        // Append round (uint256)
        data.push_str(&format!("{:064x}", round));

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": core_agent_addr,
                "data": data
            }, "latest"],
            "id": 1
        });
        let res = client.post(crate::core::clients::path::Path::from(""), &payload).await?;
        let v: serde_json::Value = serde_json::from_str(&res)?;
        if let Some(hex) = v.get("result").and_then(serde_json::Value::as_str) {
            if hex != "0x" && hex.len() >= 66 {
                let val = u128::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0) as f64;
                return Ok(val / 1e24);
            }
        }
        Ok(0.0)
    }

    #[allow(dead_code)]
    async fn get_btc_accrued_per_btc_at_round(
        &self,
        validator_addr: &str,
        client: &crate::core::clients::http_client::NodePool,
        btc_stake_addr: &str,
        round: u64,
    ) -> Result<f64> {
        // accruedRewardPerBTCMap(address,uint256)
        use tiny_keccak::{Hasher, Keccak};
        let sig = "accruedRewardPerBTCMap(address,uint256)";
        let mut keccak = Keccak::v256();
        let mut out = [0u8; 32];
        keccak.update(sig.as_bytes());
        keccak.finalize(&mut out);
        let selector = &out[0..4];
        let mut data = String::from("0x");
        data.push_str(&hex::encode(selector));
        let mut addr = validator_addr.trim_start_matches("0x").to_lowercase();
        addr = format!("{:0>64}", addr);
        data.push_str(&addr);
        data.push_str(&format!("{:064x}", round));

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": data
            }, "latest"],
            "id": 1
        });
        let res = client.post(crate::core::clients::path::Path::from(""), &payload).await?;
        let v: serde_json::Value = serde_json::from_str(&res)?;
        if let Some(hex) = v.get("result").and_then(serde_json::Value::as_str) {
            if hex != "0x" && hex.len() >= 66 {
                let val = u128::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0) as f64;
                return Ok(val / 1e8);
            }
        }
        Ok(0.0)
    }

    #[allow(dead_code)]
    async fn try_get_core_reward_rate_methods(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, core_agent_addr: &str) -> Result<f64> {
        // Method 1: getValidatorRewardRate(address)
        let reward_rate_data = format!(
            "0x89d49494000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": core_agent_addr,
                "data": reward_rate_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching CORE validator reward rate")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing CORE validator reward rate response")?;

        if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                let reward_rate = u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18; // Convert from wei to CORE
                if reward_rate > 0.0 {
                    return Ok(reward_rate);
                }
            }
        }

        // Method 2: getRewardRate(address)
        let reward_rate_data2 = format!(
            "0xea7cbff1000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload2 = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": core_agent_addr,
                "data": reward_rate_data2
            }, "latest"],
            "id": 1
        });

        let res2 = client
            .post(Path::from(""), &payload2)
            .await
            .context("Error fetching CORE reward rate (method 2)")?;

        let result2: Value = serde_json::from_str(&res2)
            .context("Error parsing CORE reward rate response (method 2)")?;

        if let Some(hex_data) = result2.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                let reward_rate = u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18;
                if reward_rate > 0.0 {
                    return Ok(reward_rate);
                }
            }
        }

        Ok(0.0)
    }

    #[allow(dead_code)]
    async fn try_get_btc_reward_rate_methods(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str) -> Result<f64> {
        // Method 1: getValidatorRewardRate(address)
        let reward_rate_data = format!(
            "0x89d49494000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": reward_rate_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching BTC validator reward rate")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing BTC validator reward rate response")?;

        if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                let reward_rate = u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e8; // Convert from satoshis to BTC
                if reward_rate > 0.0 {
                    return Ok(reward_rate);
                }
            }
        }

        // Method 2: getRewardRate(address)
        let reward_rate_data2 = format!(
            "0xea7cbff1000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload2 = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": reward_rate_data2
            }, "latest"],
            "id": 1
        });

        let res2 = client
            .post(Path::from(""), &payload2)
            .await
            .context("Error fetching BTC reward rate (method 2)")?;

        let result2: Value = serde_json::from_str(&res2)
            .context("Error parsing BTC reward rate response (method 2)")?;

        if let Some(hex_data) = result2.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                let reward_rate = u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e8;
                if reward_rate > 0.0 {
                    return Ok(reward_rate);
                }
            }
        }

        Ok(0.0)
    }

    #[allow(dead_code)]
    async fn get_total_core_reward_rate(&self, client: &crate::core::clients::http_client::NodePool, core_agent_addr: &str) -> Result<f64> {
        // Try getAnnualRewardRate() - no parameters
        let reward_rate_data = "0xd73246f8";
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": core_agent_addr,
                "data": reward_rate_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching total CORE reward rate")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing total CORE reward rate response")?;

        let reward_rate = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(reward_rate)
    }

    #[allow(dead_code)]
    async fn get_total_btc_reward_rate(&self, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str) -> Result<f64> {
        // Try getAnnualRewardRate() - no parameters
        let reward_rate_data = "0xd73246f8";
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": reward_rate_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching total BTC reward rate")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing total BTC reward rate response")?;

        let reward_rate = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e8
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(reward_rate)
    }

    #[allow(dead_code)]
    async fn get_total_core_stake(&self, client: &crate::core::clients::http_client::NodePool, core_agent_addr: &str) -> Result<f64> {
        // Try getTotalStake() - no parameters
        let total_stake_data = "0x18160ddd";
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": core_agent_addr,
                "data": total_stake_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching total CORE stake")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing total CORE stake response")?;

        let total_stake = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(total_stake)
    }

    #[allow(dead_code)]
    async fn get_total_btc_stake(&self, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str) -> Result<f64> {
        // Try getTotalStake() - no parameters
        let total_stake_data = "0x18160ddd";
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": total_stake_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching total BTC stake")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing total BTC stake response")?;

        let total_stake = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e8
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(total_stake)
    }

    async fn calculate_unclaimed_reward_ratios(&self) -> Result<()> {
        info!("(CoreDAO Staking) Calculating unclaimed reward ratios");
        
        let client = self.app_context.rpc.clone().unwrap();
        let (_, core_agent_addr, btc_stake_addr, _) = self.get_contract_addresses();
        
        // Get validators
        let validators = self.get_validators().await?;
        
        for validator_addr in validators.iter() {
            let validator_name = self.get_validator_name(validator_addr);
            let fires_alerts = self.app_context.config.general.alerting.validators.contains(validator_addr).to_string();
            
            // Calculate CORE unclaimed reward ratio
            let core_ratio = self.calculate_core_unclaimed_reward_ratio(validator_addr, &client, &core_agent_addr).await?;
            
            COREDAO_CORE_VALIDATOR_UNCLAIMED_REWARD_RATIO
                .with_label_values(&[
                    validator_addr,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(core_ratio);
            
            // Calculate BTC unclaimed reward ratio
            let btc_ratio = self.calculate_btc_unclaimed_reward_ratio(validator_addr, &client, &btc_stake_addr).await?;
            
            COREDAO_BTC_VALIDATOR_UNCLAIMED_REWARD_RATIO
                .with_label_values(&[
                    validator_addr,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(btc_ratio);
        }
        
        Ok(())
    }

    async fn calculate_core_unclaimed_reward_ratio(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, core_agent_addr: &str) -> Result<f64> {
        // Get unclaimed CORE rewards
        let unclaimed_data = format!(
            "0x91fcd9a9000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": core_agent_addr,
                "data": unclaimed_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching unclaimed CORE rewards")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing unclaimed CORE rewards response")?;

        let unclaimed_rewards = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18 // Convert from wei to CORE
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Get current CORE stake amount
        let current_stake = self.get_validator_core_stake(validator_addr, client, core_agent_addr).await?;
        
        // Calculate ratio: (unclaimed_rewards / total_stake) * 100
        let ratio = if current_stake > 0.0 {
            (unclaimed_rewards / current_stake) * 100.0
        } else {
            0.0
        };

        info!("(CoreDAO Staking) Validator {} CORE unclaimed ratio: {:.2}% (unclaimed: {:.2} CORE, stake: {:.2} CORE)", 
            validator_addr, ratio, unclaimed_rewards, current_stake);

        Ok(ratio)
    }

    async fn calculate_btc_unclaimed_reward_ratio(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str) -> Result<f64> {
        // Get unclaimed BTC rewards
        let unclaimed_data = format!(
            "0x91fcd9a9000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": unclaimed_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching unclaimed BTC rewards")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing unclaimed BTC rewards response")?;

        let unclaimed_rewards = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e8 // Convert from satoshis to BTC
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Get current BTC stake amount
        let current_stake = self.get_validator_btc_stake(validator_addr, client, btc_stake_addr).await?;
        
        // Calculate ratio: (unclaimed_rewards / total_stake) * 100
        let ratio = if current_stake > 0.0 {
            (unclaimed_rewards / current_stake) * 100.0
        } else {
            0.0
        };

        info!("(CoreDAO Staking) Validator {} BTC unclaimed ratio: {:.2}% (unclaimed: {:.8} BTC, stake: {:.8} BTC)", 
            validator_addr, ratio, unclaimed_rewards, current_stake);

        Ok(ratio)
    }

    async fn get_validator_core_stake(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, core_agent_addr: &str) -> Result<f64> {
        // Get current CORE stake using getValidatorStake(address)
        let stake_data = format!(
            "0x34664846000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": core_agent_addr,
                "data": stake_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching CORE stake")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing CORE stake response")?;

        let stake = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18 // Convert from wei to CORE
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(stake)
    }

    async fn get_validator_btc_stake(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str) -> Result<f64> {
        // Get current BTC stake using getValidatorStake(address)
        let stake_data = format!(
            "0x34664846000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": stake_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching BTC stake")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing BTC stake response")?;

        let stake = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e8 // Convert from satoshis to BTC
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(stake)
    }

    // Removed calculate_validator_rewards (per-unit accrued); using StakeHub events instead

    #[allow(dead_code)]
    async fn calculate_core_delegator_hhi(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, core_agent_addr: &str) -> Result<f64> {
        // Get delegator list for this validator
        let delegators = self.get_core_delegators(validator_addr, client, core_agent_addr).await?;
        
        if delegators.is_empty() {
            return Ok(0.0); // No delegators means no concentration
        }
        
        // Calculate total stake
        let total_stake: f64 = delegators.values().sum();
        
        if total_stake == 0.0 {
            return Ok(0.0);
        }
        
        // Calculate HHI: sum of (individual_stake / total_stake)^2 * 10000
        let hhi: f64 = delegators.values()
            .map(|&stake| {
                let market_share = stake / total_stake;
                market_share * market_share * 10000.0
            })
            .sum();
        
        info!("(CoreDAO Staking) Validator {} CORE HHI: {:.2} ({} delegators, total stake: {:.2} CORE)", 
        validator_addr, hhi, delegators.len(), total_stake);
        
        Ok(hhi)
    }

    #[allow(dead_code)]
    async fn calculate_btc_delegator_hhi(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str) -> Result<f64> {
        // Get delegator list for this validator
        let delegators = self.get_btc_delegators(validator_addr, client, btc_stake_addr).await?;
        
        if delegators.is_empty() {
            return Ok(0.0); // No delegators means no concentration
        }
        
        // Calculate total stake
        let total_stake: f64 = delegators.values().sum();
        
        if total_stake == 0.0 {
            return Ok(0.0);
        }
        
        // Calculate HHI: sum of (individual_stake / total_stake)^2 * 10000
        let hhi: f64 = delegators.values()
            .map(|&stake| {
                let market_share = stake / total_stake;
                market_share * market_share * 10000.0
            })
            .sum();
        
        info!("(CoreDAO Staking) Validator {} BTC HHI: {:.2} ({} delegators, total stake: {:.8} BTC)", 
        validator_addr, hhi, delegators.len(), total_stake);
        
        Ok(hhi)
    }

    #[allow(dead_code)]
    async fn get_core_delegators(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, core_agent_addr: &str) -> Result<HashMap<String, f64>> {
        // Get delegator list using getDelegatorList(address)
        let delegator_list_data = format!(
            "0xe318df11000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": core_agent_addr,
                "data": delegator_list_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching CORE delegator list")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing CORE delegator list response")?;

        let delegators = HashMap::new();
        
        if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() > 66 {
                // Parse the array of delegator addresses and their stakes
                // This is a simplified implementation - the actual parsing depends on the contract's return format
                // For now, we'll use a placeholder approach
                
                // TODO: Implement proper parsing of delegator list based on actual contract return format
                // The contract might return a struct array or separate arrays for addresses and stakes
                
                info!("(CoreDAO Staking) Raw delegator list data for {}: {}", validator_addr, hex_data);
            }
        }
        
        // For now, return empty map as placeholder
        // TODO: Implement actual delegator list parsing
        Ok(delegators)
    }

    #[allow(dead_code)]
    async fn get_btc_delegators(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str) -> Result<HashMap<String, f64>> {
        // Get delegator list using getDelegatorList(address)
        let delegator_list_data = format!(
            "0xe318df11000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": delegator_list_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching BTC delegator list")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing BTC delegator list response")?;

        let delegators = HashMap::new();
        
        if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() > 66 {
                // Parse the array of delegator addresses and their stakes
                // This is a simplified implementation - the actual parsing depends on the contract's return format
                // For now, we'll use a placeholder approach
                
                // TODO: Implement proper parsing of delegator list based on actual contract return format
                // The contract might return a struct array or separate arrays for addresses and stakes
                
                info!("(CoreDAO Staking) Raw delegator list data for {}: {}", validator_addr, hex_data);
            }
        }
        
        // For now, return empty map as placeholder
        // TODO: Implement actual delegator list parsing
        Ok(delegators)
    }

    // Helpers to read reward maps and current roundTag
    #[allow(dead_code)]
    async fn get_round_tag(&self, client: &crate::core::clients::http_client::NodePool, contract_addr: &str) -> Result<u64> {
        // roundTag() -> uint256, selector 0x75b10c71
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": contract_addr,
                "data": "0x75b10c71"
            }, "latest"],
            "id": 1
        });
        let res = client.post(Path::from(""), &payload).await.context("Error fetching roundTag")?;
        let result: Value = serde_json::from_str(&res).context("Error parsing roundTag response")?;
        let round = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as u64
            } else { 0 }
        } else { 0 };
        Ok(round)
    }

    #[allow(dead_code)]
    async fn get_accrued_reward_map_value(&self, client: &crate::core::clients::http_client::NodePool, core_agent_addr: &str, validator_addr: &str, round: u64) -> Result<f64> {
        // CoreAgent.accruedRewardMap(address,uint256) selector 0x8397f244
        // Encode address + uint256
        let mut data = String::from("0x8397f244");
        data.push_str(&format!("{:0>64}", validator_addr.trim_start_matches("0x")));
        data.push_str(&format!("{:064x}", round));
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{"to": core_agent_addr, "data": data}, "latest"],
            "id": 1
        });
        let res = client.post(Path::from(""), &payload).await.context("Error fetching accruedRewardMap")?;
        let result: Value = serde_json::from_str(&res).context("Error parsing accruedRewardMap response")?;
        let value = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18
            } else { 0.0 }
        } else { 0.0 };
        Ok(value)
    }

    #[allow(dead_code)]
    async fn get_accrued_reward_per_btc_value(&self, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str, validator_addr: &str, round: u64) -> Result<f64> {
        // BitcoinStake.accruedRewardPerBTCMap(address,uint256) selector 0xe8beb1c0
        let mut data = String::from("0xe8beb1c0");
        data.push_str(&format!("{:0>64}", validator_addr.trim_start_matches("0x")));
        data.push_str(&format!("{:064x}", round));
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{"to": btc_stake_addr, "data": data}, "latest"],
            "id": 1
        });
        let res = client.post(Path::from(""), &payload).await.context("Error fetching accruedRewardPerBTCMap")?;
        let result: Value = serde_json::from_str(&res).context("Error parsing accruedRewardPerBTCMap response")?;
        let value = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18
            } else { 0.0 }
        } else { 0.0 };
        Ok(value)
    }

    #[allow(dead_code)]
    async fn get_last_reward_end_round(&self, client: &crate::core::clients::http_client::NodePool, contract_addr: &str, validator_addr: &str) -> Result<Option<u64>> {
        // getContinuousRewardEndRoundsByCandidate(address) selector 0x5efc83de
        let mut data = String::from("0x5efc83de");
        data.push_str(&format!("{:0>64}", validator_addr.trim_start_matches("0x")));
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{"to": contract_addr, "data": data}, "latest"],
            "id": 1
        });
        let res = client.post(Path::from(""), &payload).await.context("Error fetching continuousRewardEndRounds")?;
        let result: Value = serde_json::from_str(&res).context("Error parsing continuousRewardEndRounds response")?;
        if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            // ABI-encoded dynamic array. If empty, it's just 0x. Otherwise, decode last 32-byte word as last element.
            if hex_data == "0x" { return Ok(None); }
            let bytes = hex::decode(hex_data.trim_start_matches("0x")).unwrap_or_default();
            if bytes.len() < 96 { return Ok(None); } // minimal dyn array encoding
            // offset (ignored), length at bytes[32..64]
            let len = u128::from_str_radix(&hex::encode(&bytes[32..64]), 16).unwrap_or(0) as usize;
            if len == 0 { return Ok(None); }
            // last element position: 64 + (len-1)*32 .. +32
            let start = 64 + (len - 1) * 32;
            if bytes.len() < start + 32 { return Ok(None); }
            let last_hex = hex::encode(&bytes[start..start+32]);
            let last = u128::from_str_radix(&last_hex, 16).unwrap_or(0) as u64;
            return Ok(Some(last));
        }
        Ok(None)
    }

    async fn calculate_btc_expiration_timestamps(&self) -> Result<()> {
        info!("(CoreDAO Staking) Calculating BTC expiration timestamps");
        
        let client = self.app_context.rpc.clone().unwrap();
        let (_, _, btc_stake_addr, _) = self.get_contract_addresses();
        
        // Get validators
        let validators = self.get_validators().await?;
        
        for validator_addr in validators.iter() {
            let validator_name = self.get_validator_name(validator_addr);
            let fires_alerts = self.app_context.config.general.alerting.validators.contains(validator_addr).to_string();
            
            // Get BTC expiration timestamp
            let expiration_timestamp = self.get_btc_expiration_timestamp(validator_addr, &client, &btc_stake_addr).await?;
            
            COREDAO_BTC_VALIDATOR_STAKE_EXPIRATION_TIMESTAMP
                .with_label_values(&[
                    validator_addr,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(expiration_timestamp);
            
            if expiration_timestamp > 0.0 {
                let expiration_date = chrono::DateTime::from_timestamp(expiration_timestamp as i64, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "Invalid timestamp".to_string());
                
                info!("(CoreDAO Staking) Validator {} BTC expires at: {} (timestamp: {})", 
                      validator_addr, expiration_date, expiration_timestamp);
            }
        }
        
        Ok(())
    }

    async fn get_btc_expiration_timestamp(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str) -> Result<f64> {
        // Get expiration time using getExpirationTime(address)
        let expiration_data = format!(
            "0x2b5fef2f000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": expiration_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching BTC expiration time")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing BTC expiration time response")?;

        let expiration_timestamp = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u64::from_str_radix(hex_clean, 16).unwrap_or(0) as f64
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(expiration_timestamp)
    }

    async fn calculate_slashing_event_counters(&self) -> Result<()> {
        info!("(CoreDAO Staking) Calculating slashing event counters");
        
        let client = self.app_context.rpc.clone().unwrap();
        let (validator_set_addr, _, _, _) = self.get_contract_addresses();
        
        // Get validators
        let validators = self.get_validators().await?;
        
        for validator_addr in validators.iter() {
            let validator_name = self.get_validator_name(validator_addr);
            let fires_alerts = self.app_context.config.general.alerting.validators.contains(validator_addr).to_string();
            
            // Get slashing event count
            let slash_count = self.get_slashing_event_count(validator_addr, &client, &validator_set_addr).await?;
            
            COREDAO_VALIDATOR_SLASH_EVENTS_TOTAL
                .with_label_values(&[
                    validator_addr,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .inc_by(slash_count as f64);
            
            // Get penalty amount
            let penalty_amount = self.get_penalty_amount(validator_addr, &client, &validator_set_addr).await?;
            
            COREDAO_VALIDATOR_PENALTY_AMOUNT_TOTAL
                .with_label_values(&[
                    validator_addr,
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .inc_by(penalty_amount);
            
            if slash_count > 0 || penalty_amount > 0.0 {
                info!("(CoreDAO Staking) Validator {} slashing: {} events, {:.2} CORE penalty", 
                      validator_addr, slash_count, penalty_amount);
            }
        }
        
        Ok(())
    }

    async fn get_slashing_event_count(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, validator_set_addr: &str) -> Result<u32> {
        // Get slash count using getSlashCount(address)
        let slash_count_data = format!(
            "0x66c36875000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": validator_set_addr,
                "data": slash_count_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching slash count")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing slash count response")?;

        let slash_count = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u32::from_str_radix(hex_clean, 16).unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        Ok(slash_count)
    }

    async fn get_penalty_amount(&self, validator_addr: &str, client: &crate::core::clients::http_client::NodePool, validator_set_addr: &str) -> Result<f64> {
        // Get penalty amount using getPenaltyAmount(address)
        let penalty_data = format!(
            "0x456f1731000000000000000000000000{}",
            validator_addr.trim_start_matches("0x")
        );
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": validator_set_addr,
                "data": penalty_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching penalty amount")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing penalty amount response")?;

        let penalty_amount = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18 // Convert from wei to CORE
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(penalty_amount)
    }

    #[allow(dead_code)]
    async fn get_core_reward_pool(&self, client: &crate::core::clients::http_client::NodePool, core_agent_addr: &str) -> Result<f64> {
        // Try getRewardPool() - no parameters
        let reward_pool_data = "0x1b8b13a7";
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": core_agent_addr,
                "data": reward_pool_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching CORE reward pool")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing CORE reward pool response")?;

        let reward_pool = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(reward_pool)
    }

    #[allow(dead_code)]
    async fn get_btc_reward_pool(&self, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str) -> Result<f64> {
        // Try getRewardPool() - no parameters
        let reward_pool_data = "0x1b8b13a7";
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": reward_pool_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching BTC reward pool")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing BTC reward pool response")?;

        let reward_pool = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e8
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(reward_pool)
    }

    #[allow(dead_code)]
    async fn get_core_annual_reward(&self, client: &crate::core::clients::http_client::NodePool, core_agent_addr: &str) -> Result<f64> {
        // Try getAnnualReward() - no parameters
        let annual_reward_data = "0x1d52ecaf";
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": core_agent_addr,
                "data": annual_reward_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching CORE annual reward")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing CORE annual reward response")?;

        let annual_reward = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e18
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(annual_reward)
    }

    #[allow(dead_code)]
    async fn get_btc_annual_reward(&self, client: &crate::core::clients::http_client::NodePool, btc_stake_addr: &str) -> Result<f64> {
        // Try getAnnualReward() - no parameters
        let annual_reward_data = "0x1d52ecaf";
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": btc_stake_addr,
                "data": annual_reward_data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching BTC annual reward")?;

        let result: Value = serde_json::from_str(&res)
            .context("Error parsing BTC annual reward response")?;

        let annual_reward = if let Some(hex_data) = result.get("result").and_then(Value::as_str) {
            if hex_data != "0x" && hex_data.len() >= 66 {
                let hex_clean = hex_data.trim_start_matches("0x");
                u128::from_str_radix(hex_clean, 16).unwrap_or(0) as f64 / 1e8
            } else {
                0.0
            }
        } else {
            0.0
        };

        Ok(annual_reward)
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

        // Calculate delegator APY
        self.calculate_delegator_apy().await
            .context("Failed to calculate delegator APY")?;

        // Calculate unclaimed reward ratios
        self.calculate_unclaimed_reward_ratios().await
            .context("Failed to calculate unclaimed reward ratios")?;

        // Emit per-validator reward metrics
        // Rewards are updated from StakeHub events

        // Calculate BTC expiration timestamps
        self.calculate_btc_expiration_timestamps().await
            .context("Failed to calculate BTC expiration timestamps")?;

        // Calculate slashing event counters
        self.calculate_slashing_event_counters().await
            .context("Failed to calculate slashing event counters")?;

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
