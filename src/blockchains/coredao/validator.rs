use std::sync::Arc;

use anyhow::{bail, Context, Ok};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::info;

use crate::{
    blockchains::coredao::metrics::{
        COREDAO_VALIDATORS, COREDAO_VALIDATOR_JAILED, COREDAO_VALIDATOR_SLASH_BLOCK,
        COREDAO_VALIDATOR_SLASH_COUNT,
    },
    core::{app_context::AppContext, clients::path::Path, exporter::RunnableModule},
};

pub struct Validator {
    pub app_context: Arc<AppContext>,
}

impl Validator {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn get_validators(&self) -> anyhow::Result<Vec<String>> {
        info!("(Core DAO Validator) Fetching validators");

        let client = self.app_context.rpc.as_ref().unwrap();
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
            "(Core DAO Validator) Sending request to RPC endpoint with payload: {}",
            payload
        );

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching validators")?;

        let result: Value =
            serde_json::from_str(&res).context("Error parsing json of the validators response")?;

        let hex_data = result
            .get("result")
            .and_then(Value::as_str)
            .context("Invalid result format for validators response")?
            .trim_start_matches("0x")
            .to_string();

        // Parse the ABI-encoded array of addresses
        // Skip first 64 hex chars (32 bytes) for the offset
        // Next 64 hex chars (32 bytes) contain the array length
        let length_hex = &hex_data.get(64..128).context("Could not get length hex")?;

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

        info!("(Core DAO Validator) Found {} validators", validators.len());
        Ok(validators)
    }

    async fn get_all_candidates(&self) -> anyhow::Result<Vec<String>> {
        info!("(Core DAO Validator) Fetching all candidates (including inactive validators)");

        let client = self.app_context.rpc.as_ref().unwrap();

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
            "(Core DAO Validator) Sending request to get all candidates with payload: {}",
            payload
        );

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching all candidates")?;

        let result: Value =
            serde_json::from_str(&res).context("Error parsing json from all candidates")?;

        let hex_data = result
            .get("result")
            .and_then(Value::as_str)
            .context("Invalid result format for all candidates response")?
            .trim_start_matches("0x")
            .to_string();

        // Parse the ABI-encoded array of addresses
        // Skip first 64 hex chars (32 bytes) for the offset
        // Next 64 hex chars (32 bytes) contain the array length
        let length_hex = &hex_data.get(64..128).context("Could not get length hex")?;

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

        info!("(Core DAO Validator) Found {} candidates", candidates.len());
        Ok(candidates)
    }

    async fn check_if_jailed(&self, validator_address: &str) -> anyhow::Result<bool> {
        info!(
            "(Core DAO Validator) Checking if validator {} is jailed",
            validator_address
        );

        let client = self.app_context.rpc.as_ref().unwrap();
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

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching if validator is jailed")?;

        info!(
            "(Core DAO Validator) Jail check response for target validator: {}",
            res
        );

        let result: Value = serde_json::from_str(&res)
            .context("Could not deserialize response to check if validator is jailed")?;

        let result_hex = result.get("result").and_then(Value::as_str).context(
            "(Core DAO Validator) Invalid jail check result format for target validator",
        )?;

        // The result is 1 if jailed, 0 if not jailed (expression check)
        Ok(result_hex == "0x1")
    }

    async fn check_slash_info(&self, validator_address: &str) -> anyhow::Result<(i64, i64)> {
        info!(
            "(Core DAO Validator) Checking slash info for validator {}",
            validator_address
        );

        let client = self.app_context.rpc.as_ref().unwrap();

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
            "(Core DAO Validator) Sending slash info request with payload: {}",
            payload
        );

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Could not fetch slashing info")?;

        let result: Value =
            serde_json::from_str(&res).context("Could not deserialize slashing info response")?;

        // Check if there's an error in the response
        if let Some(error) = result.get("error") {
            bail!(format!("RPC error in slash info response: {}", error))
        };

        let result_hex = match result.get("result") {
            Some(Value::String(hex)) => {
                let trimmed = hex.trim_start_matches("0x");
                info!("(Core DAO Validator) Extracted hex result: {}", trimmed);
                trimmed
            }
            Some(other) => {
                bail!(format!(
                    "Unexpected result type for slash info: {:?}",
                    other
                ));
            }
            None => {
                bail!(format!("No result field in slash info response"));
            }
        };

        // Check if the result is empty
        if result_hex.is_empty() {
            bail!(format!(
                "Empty slash info result for {} - likely not slashed",
                validator_address
            ));
        }

        // Parse the result - for empty or error responses, return -1
        // This handles the case where the validator has no slash info
        if result_hex == "0x" || result_hex.len() < 128 {
            bail!(format!("No slash info for validator {}", validator_address));
        }

        // Parse the two uint256 values
        // Each uint256 is 32 bytes (64 hex chars)
        // The first 64 characters (32 bytes) represent the block height
        let block_height_hex = &result_hex[0..64];
        // The next 64 characters represent the slashing count
        let slash_count_hex = &result_hex[64..128];

        let block_height = i64::from_str_radix(block_height_hex, 16).unwrap_or(-1);

        let slash_count = i64::from_str_radix(slash_count_hex, 16).unwrap_or(-1);

        info!(
            "(Core DAO Validator) Parsed slash info - Block Height: {}, Slash Count: {}",
            block_height, slash_count
        );

        Ok((block_height, slash_count))
    }

    async fn collect_validator_metrics(&self) -> anyhow::Result<()> {
        // Get all active validators and normalize to lowercase
        let active_validators: Vec<String> = self
            .get_validators()
            .await
            .context("Could not obtain active validators")?
            .into_iter()
            .map(|addr| addr.to_lowercase())
            .collect();

        info!(
            "(Core DAO Validator) Found {} active validators",
            active_validators.len()
        );

        // Get all candidates (including inactive validators) and normalize to lowercase
        let all_candidates: Vec<String> = self
            .get_all_candidates()
            .await
            .context("Could not obtain all candidates")?
            .into_iter()
            .map(|addr| addr.to_lowercase())
            .collect();

        info!(
            "(Core DAO Validator) Found {} total candidates",
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

        let alert_addresses = self.app_context.config.general.alerting.validators.clone();

        for validator in &alert_addresses {
            if !all_validators.contains(validator) {
                all_validators.push(validator.clone());
            }
        }

        // Set the validator metric for all validators (active and inactive)
        for validator in &all_validators {
            let fires_alerts = alert_addresses.contains(validator).to_string();

            let is_active = active_validators.contains(validator);
            info!(
                "(Core DAO Validator) Setting validator metric for: {} (active: {})",
                validator, is_active
            );

            COREDAO_VALIDATORS
                .with_label_values(&[
                    validator,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(if is_active { 1 } else { 0 });

            // Always check metrics for the target validator
            // Check if the target validator is jailed
            let is_jailed = self
                .check_if_jailed(&validator)
                .await
                .context("Could not check if validator is jailed")?;

            info!(
                "(Core DAO Validator) Target validator jailed status: {}",
                is_jailed
            );

            // Set the jailed metric for the target validator
            COREDAO_VALIDATOR_JAILED
                .with_label_values(&[
                    validator,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(if is_jailed { 1 } else { 0 });

            // Check the slash info for the target validator
            let (block_height, slash_count) = self.check_slash_info(&validator).await.context(
                format!("Could not check slash info for validator: {}", validator),
            )?;
            info!("(Core DAO Validator) Target validator slash info - Block Height: {}, Slash Count: {}", 
                      block_height, slash_count);

            // Set the slash metrics for the target validator
            COREDAO_VALIDATOR_SLASH_BLOCK
                .with_label_values(&[
                    validator,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(block_height as i64);

            COREDAO_VALIDATOR_SLASH_COUNT
                .with_label_values(&[
                    validator,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(slash_count as i64);
        }
        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.rpc.is_none() {
        anyhow::bail!("Config is missing RPC node pool");
    }
    Ok(Box::new(Validator::new(app_context)))
}

#[async_trait]
impl RunnableModule for Validator {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.collect_validator_metrics()
            .await
            .context("Could not collect validator metrics")
    }

    fn name(&self) -> &'static str {
        "Core DAO Validator"
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.app_context.config.network.coredao.validator.interval)
    }
}
