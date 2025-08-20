use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::info;
use tokio::sync::RwLock;

use crate::{
    blockchains::coredao::metrics::{
        COREDAO_VALIDATORS, COREDAO_VALIDATOR_JAILED, COREDAO_VALIDATOR_SLASH_BLOCK,
        COREDAO_VALIDATOR_SLASH_COUNT,
    },
    core::{app_context::AppContext, clients::path::Path, exporter::RunnableModule},
};

#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    pub address: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct CachedValidators {
    pub validators: Vec<ValidatorInfo>,
    timestamp: Instant,
}

impl CachedValidators {
    fn new(validators: Vec<ValidatorInfo>) -> Self {
        Self {
            validators,
            timestamp: Instant::now(),
        }
    }

    fn is_expired(&self, cache_duration: Duration) -> bool {
        self.timestamp.elapsed() > cache_duration
    }
}

pub struct ValidatorFetcher {
    app_context: Arc<AppContext>,
    pub cache: Arc<RwLock<Option<CachedValidators>>>,
}

impl ValidatorFetcher {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self {
            app_context,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn fetch_validators_from_api(&self) -> anyhow::Result<Vec<ValidatorInfo>> {
        let api_config = &self.app_context.config.network.coredao.validator.api;
        
        if !api_config.enabled {
            bail!("API fetching is not enabled");
        }

        // Determine base URL:
        // 1) Use explicit validator.api.url if present
        // 2) Otherwise derive from general.nodes.lcd (prefer node.client match, else first)
        let base_url = if let Some(url) = api_config.get_url() {
            url
        } else {
            let client_name = &self.app_context.config.node.client;
            let lcd_nodes = &self.app_context.config.general.nodes.lcd;

            let selected = lcd_nodes
                .iter()
                .find(|n| &n.name == client_name)
                .or_else(|| lcd_nodes.first())
                .map(|n| n.url.clone());

            selected.ok_or_else(|| anyhow::anyhow!(
                "No LCD nodes configured to derive API base URL"
            ))?
        };

        // Ensure the endpoint path is present
        const ENDPOINT: &str = "/api/stats/list_of_validators";
        let trimmed = base_url.trim_end_matches('/');
        let final_url = if trimmed.ends_with(ENDPOINT) || trimmed.contains(ENDPOINT) {
            trimmed.to_string()
        } else {
            format!("{}{}", trimmed, ENDPOINT)
        };

        info!(
            "(Core DAO Validator) Fetching validators from API: {}",
            final_url
        );

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        let mut request = client.get(&final_url);

        // API expects the key as a query parameter: ?apikey=...
        if let Some(api_key) = api_config.get_api_key() {
            info!("(Core DAO Validator) Using API key as query parameter (length: {})", api_key.len());
            request = request.query(&[("apikey", api_key)]);
        } else {
            info!("(Core DAO Validator) No API key found, making unauthenticated request");
        }

        let response = request.send().await
            .context("Failed to send API request")?;

        if !response.status().is_success() {
            bail!("API request failed with status: {}", response.status());
        }

        let body = response.text().await
            .context("Failed to read API response body")?;

        // Parse the API response - handle the actual format with result array containing validator objects
        let json_value: Value = serde_json::from_str(&body)
            .context("Failed to parse API response as JSON")?;

        let validators: Vec<ValidatorInfo> = if let Some(result_array) = json_value.get("result").and_then(|v| v.as_array()) {
            result_array
                .iter()
                .filter_map(|validator_obj| {
                    // Extract operatorAddress and validatorName from each validator object
                    let address = validator_obj
                        .get("operatorAddress")
                        .and_then(|addr| addr.as_str())?;
                    
                    let name = validator_obj
                        .get("validatorName")
                        .and_then(|name| name.as_str())
                        .filter(|n| !n.is_empty()) // Filter out empty strings
                        .unwrap_or(address); // Fallback to address if name is not available or empty
                    
                    Some(ValidatorInfo {
                        address: address.to_string(),
                        name: name.to_string(),
                    })
                })
                .collect()
        } else {
            bail!("API response does not contain a valid 'result' array with validator objects");
        };

        info!("(Core DAO Validator) Successfully fetched {} validators from API", validators.len());
        Ok(validators)
    }

    pub async fn fetch_validators_from_rpc(&self) -> anyhow::Result<Vec<ValidatorInfo>> {
        info!("(Core DAO Validator) Fetching validators from RPC (fallback)");

        let client = self.app_context.rpc.as_ref()
            .context("RPC client not available")?;

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
            "(Core DAO Validator) Sending RPC request with payload: {}",
            payload
        );

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching validators from RPC")?;

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
                validators.push(ValidatorInfo {
                    address: address.clone(),
                    name: address, // Use address as name for RPC fallback
                });
            }
        }

        info!("(Core DAO Validator) Successfully fetched {} validators from RPC", validators.len());
        Ok(validators)
    }

    pub async fn get_validators(&self) -> anyhow::Result<Vec<ValidatorInfo>> {
        let api_config = &self.app_context.config.network.coredao.validator.api;
        let cache_duration = Duration::from_secs(api_config.cache_duration_seconds);

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.as_ref() {
                if !cached.is_expired(cache_duration) {
                    info!("✅ (Core DAO Validator) Using cached validators ({} validators)", cached.validators.len());
                    return Ok(cached.validators.clone());
                }
            }
        }

        // Try API first if enabled
        if api_config.enabled {
            info!("(Core DAO Validator) Attempting to fetch validators from API...");
            match self.fetch_validators_from_api().await {
                Ok(validators) => {
                    // Cache the result
                    let mut cache = self.cache.write().await;
                    *cache = Some(CachedValidators::new(validators.clone()));
                    info!("✅ (Core DAO Validator) Successfully fetched {} validators from API", validators.len());
                    return Ok(validators);
                }
                Err(e) => {
                    info!("❌ (Core DAO Validator) API fetch failed: {}. Falling back to RPC.", e);
                }
            }
        }

        // Fallback to RPC
        info!("(Core DAO Validator) Fetching validators from RPC (fallback)");
        let validators = self.fetch_validators_from_rpc().await?;
        
        // Cache the RPC result as well
        let mut cache = self.cache.write().await;
        *cache = Some(CachedValidators::new(validators.clone()));
        info!("✅ (Core DAO Validator) Successfully fetched {} validators from RPC", validators.len());
        
        Ok(validators)
    }
}

pub struct Validator {
    pub app_context: Arc<AppContext>,
    pub fetcher: ValidatorFetcher,
}

impl Validator {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { 
            app_context: app_context.clone(),
            fetcher: ValidatorFetcher::new(app_context),
        }
    }

    async fn get_validators(&self) -> anyhow::Result<Vec<ValidatorInfo>> {
        self.fetcher.get_validators().await
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
        // Get all active validators with their names
        let active_validators_info: Vec<ValidatorInfo> = self
            .get_validators()
            .await
            .context("Could not obtain active validators")?;
        
        let active_validators: Vec<String> = active_validators_info
            .iter()
            .map(|v| v.address.to_lowercase())
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
            
            // Find validator name from active_validators_info, or use address as fallback
            let validator_name = active_validators_info
                .iter()
                .find(|v| v.address.to_lowercase() == *validator)
                .map(|v| v.name.clone())
                .unwrap_or_else(|| validator.clone());
            
            info!(
                "(Core DAO Validator) Setting validator metric for: {} ({}) (active: {})",
                validator, validator_name, is_active
            );

            COREDAO_VALIDATORS
                .with_label_values(&[
                    validator,
                    &validator_name,
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
                    &validator_name,
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
                    &validator_name,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .set(block_height as i64);

            COREDAO_VALIDATOR_SLASH_COUNT
                .with_label_values(&[
                    validator,
                    &validator_name,
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
