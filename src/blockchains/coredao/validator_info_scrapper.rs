use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::time::sleep;
use tracing::{debug, error, info};

use crate::{
    blockchains::coredao::metrics::{
        COREDAO_VALIDATORS, COREDAO_VALIDATOR_JAILED, COREDAO_VALIDATOR_SLASH_BLOCK,
        COREDAO_VALIDATOR_SLASH_COUNT,
    },
    core::{clients::blockchain_client::BlockchainClient, exporter::Task, network::Network},
};

pub struct CoreDaoValidatorInfoScrapper {
    client: Arc<BlockchainClient>,
    validator_alert_addresses: Vec<String>,
    network: Network,
}

impl CoreDaoValidatorInfoScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        validator_alert_addresses: Vec<String>,
        network: Network,
    ) -> Self {
        Self {
            client,
            validator_alert_addresses,
            network,
        }
    }

    async fn get_validators(&self) -> Vec<String> {
        info!("(Core DAO Validator Info) Fetching validators");

        // Use contract ValidatorSet.sol using function getValidatorOps()
        let data = "0x93f2d404";

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": "0x0000000000000000000000000000000000001000",
                "data": data
            }, "latest"],
            "id": 1
        });

        info!(
            "(Core DAO Validator Info) Sending request to RPC endpoint with payload: {}",
            payload
        );

        let res = match self.client.with_rpc().post("", &payload).await {
            Ok(res) => {
                info!(
                    "(Core DAO Validator Info) Received response from endpoint: {}",
                    res
                );
                res
            }
            Err(e) => {
                error!(
                    "(Core DAO Validator Info) Error calling validators endpoint: {}",
                    e
                );
                return Vec::new();
            }
        };

        let result: Value = match serde_json::from_str(&res) {
            Ok(val) => val,
            Err(e) => {
                error!("(Core DAO Validator Info) Error parsing JSON: {}", e);
                return Vec::new();
            }
        };

        let hex_data = match result.get("result") {
            Some(Value::String(hex)) => hex.trim_start_matches("0x"),
            _ => {
                error!("(Core DAO Validator Info) Invalid result format");
                return Vec::new();
            }
        };

        // Parse the ABI-encoded array of addresses
        // Skip first 64 hex chars (32 bytes) for the offset
        // Next 64 hex chars (32 bytes) contain the array length
        let length_hex = &hex_data[64..128];
        let length = u64::from_str_radix(length_hex, 16).unwrap_or(0) as usize;

        let mut validators = Vec::with_capacity(length);

        // Each address is 32 bytes (64 hex chars), but we only need the last 20 bytes (40 hex chars)
        for i in 0..length {
            let start = 128 + i * 64;
            if start + 64 <= hex_data.len() {
                // Take the last 40 hex chars of each 64-char segment (20 bytes of address)
                let address = format!("0x{}", &hex_data[start + 24..start + 64]);
                validators.push(address);
            }
        }

        info!(
            "(Core DAO Validator Info) Found {} validators",
            validators.len()
        );
        validators
    }

    async fn get_all_candidates(&self) -> Vec<String> {
        info!("(Core DAO Validator Info) Fetching all candidates (including inactive validators)");

        // Use contract CandidateHub.sol using function getCandidates()
        let data = "0x06a49fce";

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": "0x0000000000000000000000000000000000001005",
                "data": data
            }, "latest"],
            "id": 1
        });

        info!(
            "(Core DAO Validator Info) Sending request to get all candidates with payload: {}",
            payload
        );

        let res = match self.client.with_rpc().post("", &payload).await {
            Ok(res) => {
                info!(
                    "(Core DAO Validator Info) Received response for all candidates: {}",
                    res
                );
                res
            }
            Err(e) => {
                error!(
                    "(Core DAO Validator Info) Error calling all candidates endpoint: {}",
                    e
                );
                return Vec::new();
            }
        };

        let result: Value = match serde_json::from_str(&res) {
            Ok(val) => val,
            Err(e) => {
                error!(
                    "(Core DAO Validator Info) Error parsing JSON for all candidates: {}",
                    e
                );
                return Vec::new();
            }
        };

        let hex_data = match result.get("result") {
            Some(Value::String(hex)) => hex.trim_start_matches("0x"),
            _ => {
                error!("(Core DAO Validator Info) Invalid result format for all candidates");
                return Vec::new();
            }
        };

        // Parse the ABI-encoded array of addresses
        // Skip first 64 hex chars (32 bytes) for the offset
        // Next 64 hex chars (32 bytes) contain the array length
        let length_hex = &hex_data[64..128];
        let length = u64::from_str_radix(length_hex, 16).unwrap_or(0) as usize;

        let mut candidates = Vec::with_capacity(length);

        // Each address is 32 bytes (64 hex chars), but we only need the last 20 bytes (40 hex chars)
        for i in 0..length {
            let start = 128 + i * 64;
            if start + 64 <= hex_data.len() {
                // Take the last 40 hex chars of each 64-char segment (20 bytes of address)
                let address = format!("0x{}", &hex_data[start + 24..start + 64]);
                candidates.push(address);
            }
        }

        info!(
            "(Core DAO Validator Info) Found {} candidates",
            candidates.len()
        );
        candidates
    }

    async fn check_if_jailed(&self, validator_address: &str) -> bool {
        info!(
            "(Core DAO Validator Info) Checking if validator {} is jailed",
            validator_address
        );

        // Use contract CandidateHub.sol using function isJailed(address)
        let data = format!(
            "0x14bfb527000000000000000000000000{}",
            validator_address.trim_start_matches("0x")
        );
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": "0x0000000000000000000000000000000000001005",
                "data": data
            }, "latest"],
            "id": 1
        });

        let res = match self.client.with_rpc().post("", &payload).await {
            Ok(res) => res,
            Err(e) => {
                error!(
                    "(Core DAO Validator Info) Error checking if validator is jailed: {}",
                    e
                );
                return false;
            }
        };

        info!(
            "(Core DAO Validator Info) Jail check response for target validator: {}",
            res
        );

        let result: Value = match serde_json::from_str(&res) {
            Ok(val) => val,
            Err(e) => {
                error!("(Core DAO Validator Info) Error parsing JSON for target validator jail check: {}", e);
                return false;
            }
        };

        let result_hex = match result.get("result") {
            Some(Value::String(hex)) => hex,
            _ => {
                error!("(Core DAO Validator Info) Invalid jail check result format for target validator");
                return false;
            }
        };

        // The result is 1 if jailed, 0 if not jailed (expression check)
        result_hex == "0x1"
    }

    async fn check_slash_info(&self, validator_address: &str) -> (i64, i64) {
        info!(
            "(Core DAO Validator Info) Checking slash info for validator {}",
            validator_address
        );

        // Use contract SlashIndicator.sol with function getSlashIndicator(address)
        let data = format!(
            "0x37c8dab9000000000000000000000000{}",
            validator_address.trim_start_matches("0x")
        );
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": "0x0000000000000000000000000000000000001001",
                "data": data
            }, "latest"],
            "id": 1
        });

        info!(
            "(Core DAO Validator Info) Sending slash info request with payload: {}",
            payload
        );

        let res = match self.client.with_rpc().post("", &payload).await {
            Ok(res) => {
                info!(
                    "(Core DAO Validator Info) Received slash info response: {}",
                    res
                );
                res
            }
            Err(e) => {
                error!(
                    "(Core DAO Validator Info) Error checking slash info for {}: {}",
                    validator_address, e
                );
                return (-1, -1);
            }
        };

        let result: Value = match serde_json::from_str(&res) {
            Ok(val) => val,
            Err(e) => {
                error!(
                    "(Core DAO Validator Info) Error parsing JSON for slash info: {}",
                    e
                );
                return (-1, -1);
            }
        };

        // Check if there's an error in the response
        if let Some(error) = result.get("error") {
            error!(
                "(Core DAO Validator Info) RPC error in slash info response: {:?}",
                error
            );
            // If we get an execution reverted error, it likely means the validator doesn't exist
            // or has no slash info, so return -1 value
            return (-1, -1);
        }

        let result_hex = match result.get("result") {
            Some(Value::String(hex)) => {
                let trimmed = hex.trim_start_matches("0x");
                info!(
                    "(Core DAO Validator Info) Extracted hex result: {}",
                    trimmed
                );
                trimmed
            }
            Some(other) => {
                error!(
                    "(Core DAO Validator Info) Unexpected result type for slash info: {:?}",
                    other
                );
                return (-1, -1);
            }
            None => {
                error!("(Core DAO Validator Info) No result field in slash info response");
                return (-1, -1);
            }
        };

        // Check if the result is empty
        if result_hex.is_empty() {
            info!(
                "(Core DAO Validator Info) Empty slash info result for {} - likely not slashed",
                validator_address
            );
            return (-1, -1);
        }

        // Parse the result - for empty or error responses, return -1
        // This handles the case where the validator has no slash info
        if result_hex == "0x" || result_hex.len() < 128 {
            info!(
                "(Core DAO Validator Info) No slash info for validator {}",
                validator_address
            );
            return (-1, -1);
        }

        // Parse the two uint256 values
        // Each uint256 is 32 bytes (64 hex chars)
        // The first 64 characters (32 bytes) represent the block height
        let block_height_hex = &result_hex[0..64];
        // The next 64 characters represent the slashing count
        let slash_count_hex = &result_hex[64..128];

        let block_height = match i64::from_str_radix(block_height_hex, 16) {
            Ok(val) => val,
            Err(e) => {
                error!(
                    "(Core DAO Validator Info) Error parsing block height hex: {}",
                    e
                );
                -1
            }
        };

        let slash_count = match i64::from_str_radix(slash_count_hex, 16) {
            Ok(val) => val,
            Err(e) => {
                error!(
                    "(Core DAO Validator Info) Error parsing slash count hex: {}",
                    e
                );
                -1
            }
        };

        info!(
            "(Core DAO Validator Info) Parsed slash info - Block Height: {}, Slash Count: {}",
            block_height, slash_count
        );

        (block_height, slash_count)
    }
}

#[async_trait]
impl Task for CoreDaoValidatorInfoScrapper {
    async fn run(&mut self, delay: Duration) {
        info!("(Core DAO Validator Info) Starting task");

        // Print the RPC endpoint(s) from the environment variable
        if let Ok(rpc_endpoints) = std::env::var("RPC_ENDPOINTS") {
            debug!(
                "(Core DAO Validator Info) Using RPC endpoints from env: {}",
                rpc_endpoints
            );
        } else {
            error!("(Core DAO Validator Info) RPC_ENDPOINTS environment variable not set");
        }

        loop {
            info!("(Core DAO Validator Info) Executing validator info collection");

            // Collect and update validator metrics
            self.collect_validator_metrics().await;

            info!(
                "(Core DAO Validator Info) Task iteration completed, sleeping for {:?}",
                delay
            );
            sleep(delay).await;
        }
    }
}

impl CoreDaoValidatorInfoScrapper {
    // Add this new method to encapsulate the validator metrics collection logic
    async fn collect_validator_metrics(&self) {
        // Get all active validators and normalize to lowercase
        let active_validators: Vec<String> = self
            .get_validators()
            .await
            .into_iter()
            .map(|addr| addr.to_lowercase())
            .collect();

        info!(
            "(Core DAO Validator Info) Found {} active validators",
            active_validators.len()
        );

        // Get all candidates (including inactive validators) and normalize to lowercase
        let all_candidates: Vec<String> = self
            .get_all_candidates()
            .await
            .into_iter()
            .map(|addr| addr.to_lowercase())
            .collect();

        info!(
            "(Core DAO Validator Info) Found {} total candidates",
            all_candidates.len()
        );

        // Create a deduplicated set of all validators
        let mut all_validators = all_candidates.clone();

        // Add active validators that might not be in the candidates list
        for validator in &active_validators {
            if !all_validators.contains(validator) {
                all_validators.push(validator.clone());
            }
        }

        for validator in &self.validator_alert_addresses {
            if !all_validators.contains(validator) {
                all_validators.push(validator.clone());
            }
        }

        // Set the validator metric for all validators (active and inactive)
        for validator in &all_validators {
            let fires_alerts = self
                .validator_alert_addresses
                .contains(validator)
                .to_string();

            let is_active = active_validators.contains(validator);
            info!(
                "(Core DAO Validator Info) Setting validator metric for: {} (active: {})",
                validator, is_active
            );

            COREDAO_VALIDATORS
                .with_label_values(&[validator, &self.network.to_string(), &fires_alerts])
                .set(if is_active { 1 } else { 0 });

            // Always check metrics for the target validator
            // Check if the target validator is jailed
            let is_jailed = self.check_if_jailed(&validator).await;
            info!(
                "(Core DAO Validator Info) Target validator jailed status: {}",
                is_jailed
            );

            // Set the jailed metric for the target validator
            COREDAO_VALIDATOR_JAILED
                .with_label_values(&[validator, &self.network.to_string(), &fires_alerts])
                .set(if is_jailed { 1 } else { 0 });

            // Check the slash info for the target validator
            let (block_height, slash_count) = self.check_slash_info(&validator).await;
            info!("(Core DAO Validator Info) Target validator slash info - Block Height: {}, Slash Count: {}", 
                      block_height, slash_count);

            // Set the slash metrics for the target validator
            COREDAO_VALIDATOR_SLASH_BLOCK
                .with_label_values(&[validator, &self.network.to_string(), &fires_alerts])
                .set(block_height as i64);

            COREDAO_VALIDATOR_SLASH_COUNT
                .with_label_values(&[validator, &self.network.to_string(), &fires_alerts])
                .set(slash_count as i64);
        }
    }
}
