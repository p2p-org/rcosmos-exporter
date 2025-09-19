use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::core::{app_context::AppContext, clients::path::Path};

#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    pub address: String,
    pub name: String,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    name: String,
    source: CacheSource,
    timestamp: Instant,
}

#[derive(Debug, Clone, PartialEq)]
enum CacheSource {
    Api,
    Rpc,
}

pub struct SharedValidatorCache {
    app_context: Arc<AppContext>,
    // Address -> CacheEntry mapping
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    // Last successful API fetch time
    last_api_success: Arc<RwLock<Option<Instant>>>,
    // Cache duration from config
    cache_duration: Duration,
}

impl SharedValidatorCache {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        let cache_duration = Duration::from_secs(
            app_context
                .config
                .network
                .coredao
                .validator
                .api
                .cache_duration_seconds,
        );

        Self {
            app_context,
            cache: Arc::new(RwLock::new(HashMap::new())),
            last_api_success: Arc::new(RwLock::new(None)),
            cache_duration,
        }
    }

    /// Get validator name with consistent fallback behavior
    pub async fn get_validator_name(&self, address: &str) -> String {
        let normalized_address = address.to_lowercase();

        // First, try to get from cache
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&normalized_address) {
                // Only use cached entry if it's not expired or if it's from API
                if entry.source == CacheSource::Api || !self.is_entry_expired(entry) {
                    return entry.name.clone();
                }
            }
        }

        // Cache miss or expired - try to refresh
        if let Err(e) = self.refresh_cache().await {
            warn!("Failed to refresh validator cache: {}", e);
        }

        // Try again after refresh
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&normalized_address) {
                return entry.name.clone();
            }
        }

        // Final fallback - use address but don't cache it to avoid overwriting good data
        warn!(
            "No cached name found for validator {}, using address as fallback",
            address
        );
        address.to_string()
    }

    /// Get all cached validators
    pub async fn get_all_validators(&self) -> Vec<ValidatorInfo> {
        let cache = self.cache.read().await;
        cache
            .iter()
            .map(|(address, entry)| ValidatorInfo {
                address: address.clone(),
                name: entry.name.clone(),
            })
            .collect()
    }

    /// Force refresh the cache from API/RPC
    pub async fn refresh_cache(&self) -> anyhow::Result<()> {
        let api_config = &self.app_context.config.network.coredao.validator.api;

        // Try API first if enabled
        if api_config.enabled {
            match self.fetch_from_api().await {
                Ok(validators) => {
                    self.update_cache(validators, CacheSource::Api).await;
                    let mut last_success = self.last_api_success.write().await;
                    *last_success = Some(Instant::now());
                    info!("✅ Successfully refreshed validator cache from API");
                    return Ok(());
                }
                Err(e) => {
                    error!("❌ API fetch failed during cache refresh: {}", e);
                }
            }
        }

        // Fallback to RPC, but preserve existing API names
        match self.fetch_from_rpc().await {
            Ok(validators) => {
                // Only update entries that don't exist or are from RPC/Fallback sources
                self.update_cache_selective(validators, CacheSource::Rpc)
                    .await;
                info!("✅ Refreshed validator cache from RPC (preserving API names)");
                Ok(())
            }
            Err(e) => {
                error!("❌ Both API and RPC failed during cache refresh");
                Err(e.context("Failed to refresh cache from both API and RPC"))
            }
        }
    }

    /// Check if we should attempt cache refresh
    pub async fn should_refresh(&self) -> bool {
        let cache = self.cache.read().await;

        // Refresh if cache is empty
        if cache.is_empty() {
            return true;
        }

        // Check if we have any recent API data
        let last_api = self.last_api_success.read().await;
        if let Some(last_success) = *last_api {
            // If API data is recent, don't refresh
            if last_success.elapsed() < self.cache_duration {
                return false;
            }
        }

        // Refresh if most entries are expired
        let expired_count = cache
            .values()
            .filter(|entry| self.is_entry_expired(entry))
            .count();

        expired_count > cache.len() / 2
    }

    fn is_entry_expired(&self, entry: &CacheEntry) -> bool {
        match entry.source {
            CacheSource::Api => entry.timestamp.elapsed() > self.cache_duration * 2, // Keep API data longer
            CacheSource::Rpc => entry.timestamp.elapsed() > self.cache_duration,
        }
    }

    async fn update_cache(&self, validators: Vec<ValidatorInfo>, source: CacheSource) {
        let mut cache = self.cache.write().await;
        let now = Instant::now();

        for validator in validators {
            let key = validator.address.to_lowercase();
            cache.insert(
                key,
                CacheEntry {
                    name: validator.name,
                    source: source.clone(),
                    timestamp: now,
                },
            );
        }

        info!(
            "Updated cache with {} validators from {:?}",
            cache.len(),
            source
        );
    }

    async fn update_cache_selective(&self, validators: Vec<ValidatorInfo>, source: CacheSource) {
        let mut cache = self.cache.write().await;
        let now = Instant::now();
        let mut updated_count = 0;

        for validator in validators {
            let key = validator.address.to_lowercase();
            let should_update = match cache.get(&key) {
                None => true,
                Some(existing) => {
                    existing.source != CacheSource::Api || self.is_entry_expired(existing)
                }
            };

            if should_update {
                cache.insert(
                    key,
                    CacheEntry {
                        name: validator.name,
                        source: source.clone(),
                        timestamp: now,
                    },
                );
                updated_count += 1;
            }
        }

        info!(
            "Selectively updated {} cache entries from {:?} (preserving {} API entries)",
            updated_count,
            source,
            cache
                .values()
                .filter(|e| e.source == CacheSource::Api)
                .count()
        );
    }

    async fn fetch_from_api(&self) -> anyhow::Result<Vec<ValidatorInfo>> {
        let api_config = &self.app_context.config.network.coredao.validator.api;

        if !api_config.enabled {
            bail!("API fetching is not enabled");
        }

        // Determine base URL
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

            selected
                .ok_or_else(|| anyhow::anyhow!("No LCD nodes configured to derive API base URL"))?
        };

        const ENDPOINT: &str = "/api/stats/list_of_validators";
        let trimmed = base_url.trim_end_matches('/');
        let final_url = if trimmed.ends_with(ENDPOINT) || trimmed.contains(ENDPOINT) {
            trimmed.to_string()
        } else {
            format!("{}{}", trimmed, ENDPOINT)
        };

        info!("Fetching validators from API: {}", final_url);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        let mut request = client.get(&final_url);

        if let Some(api_key) = api_config.get_api_key() {
            request = request.query(&[("apikey", api_key)]);
        }

        let response = request.send().await.context("Failed to send API request")?;

        if !response.status().is_success() {
            bail!("API request failed with status: {}", response.status());
        }

        let body = response
            .text()
            .await
            .context("Failed to read API response body")?;

        let json_value: Value =
            serde_json::from_str(&body).context("Failed to parse API response as JSON")?;

        let validators: Vec<ValidatorInfo> =
            if let Some(result_array) = json_value.get("result").and_then(|v| v.as_array()) {
                result_array
                    .iter()
                    .filter_map(|validator_obj| {
                        let address = validator_obj
                            .get("operatorAddress")
                            .and_then(|addr| addr.as_str())?;

                        let status = validator_obj
                            .get("validatorStatus")
                            .and_then(|s| s.as_str())
                            .unwrap_or("0");

                        if status != "1" {
                            return None;
                        }

                        let name = validator_obj
                            .get("validatorName")
                            .and_then(|name| name.as_str())
                            .filter(|n| !n.is_empty())
                            .unwrap_or(address);

                        Some(ValidatorInfo {
                            address: address.to_string(),
                            name: name.to_string(),
                        })
                    })
                    .collect()
            } else {
                bail!("API response does not contain a valid 'result' array");
            };

        Ok(validators)
    }

    async fn fetch_from_rpc(&self) -> anyhow::Result<Vec<ValidatorInfo>> {
        info!("Fetching validators from RPC");

        let client = self
            .app_context
            .rpc
            .as_ref()
            .context("RPC client not available")?;

        let data = "0xb7ab4db5";
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": "0x0000000000000000000000000000000000001000",
                "data": data
            }, "latest"],
            "id": 1
        });

        let res = client
            .post(Path::from(""), &payload)
            .await
            .context("Error fetching validators from RPC")?;

        let result: Value = serde_json::from_str(&res).context("Error parsing JSON response")?;

        let hex_data = result
            .get("result")
            .and_then(Value::as_str)
            .context("Invalid result format")?
            .trim_start_matches("0x")
            .to_string();

        let length_hex = hex_data.get(64..128).context("Could not get length hex")?;
        let length = u64::from_str_radix(length_hex, 16).unwrap_or(0) as usize;

        let mut validators = Vec::with_capacity(length);

        for i in 0..length {
            let start = 128 + i * 64;
            if start + 64 <= hex_data.len() {
                let address = format!("0x{}", &hex_data[start + 24..start + 64]);

                // CRITICAL FIX: Don't use address as name for RPC data
                // Instead, preserve any existing cached name or use a placeholder
                let name = {
                    let cache = self.cache.read().await;
                    if let Some(existing) = cache.get(&address.to_lowercase()) {
                        existing.name.clone()
                    } else {
                        format!("Validator-{}", &address[2..8]) // Use short prefix instead of full address
                    }
                };

                validators.push(ValidatorInfo {
                    address: address.clone(),
                    name,
                });
            }
        }

        Ok(validators)
    }
}
