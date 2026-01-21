use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use serde_json::from_str;
use tracing::{debug, info};

use crate::blockchains::sei::metrics::{
    COMETBFT_VALIDATOR, COMETBFT_VALIDATOR_PROPOSER_PRIORITY, COMETBFT_VALIDATOR_VOTING_POWER,
};
use crate::blockchains::sei::types::SeiValidatorsResponse;
use crate::blockchains::tendermint::metrics::{TENDERMINT_VALIDATOR, TENDERMINT_VALIDATOR_TOKENS};
use crate::blockchains::tendermint::types::{Validator, ValidatorsResponse};
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;
use base64::engine::general_purpose;
use base64::Engine;
use sha2::{Digest, Sha256};
use urlencoding::encode;

pub struct Validators {
    pub app_context: Arc<AppContext>,
}

impl Validators {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn fetch_sei_validators(&self) -> anyhow::Result<()> {
        let mut validators: Vec<crate::blockchains::sei::types::SeiValidator> = Vec::new();
        let mut all_fetched = false;
        let path = "/validators".to_string();
        let mut page = 1;
        let client = self.app_context.rpc.as_ref().unwrap();

        while !all_fetched {
            let url = format!("{}?page={}", path, page);
            let res = client
                .get(Path::from(url))
                .await
                .map_err(|e| anyhow::anyhow!(format!("NodePool error: {e}")))?;

            let validators_response: SeiValidatorsResponse = from_str(&res)
                .context("Could not decode Sei validators JSON response")?;

            let count = validators_response.count.parse::<usize>().context(
                "Could not parse the count of obtained validators when fetching validators",
            )?;
            let total = validators_response
                .total
                .parse::<usize>()
                .context("Could not parse the total of validators when fetching validators")?;

            if count + validators.len() == total {
                all_fetched = true;
            } else {
                page += 1;
            }
            validators.extend(validators_response.validators);
        }

        let alert_addresses = self.app_context.config.general.alerting.validators.clone();

        // Fetch monikers from Tendermint staking endpoint if LCD is available
        // This provides human-readable monikers instead of just addresses
        let mut moniker_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if let Some(lcd_client) = &self.app_context.lcd {
            if let Ok(rest_validators) = self.fetch_tendermint_validators(lcd_client).await {
                for validator in rest_validators {
                    // Calculate consensus address from pubkey (same as Tendermint staking does)
                    let bytes = general_purpose::STANDARD
                        .decode(&validator.consensus_pubkey.key)
                        .ok();
                    if let Some(bytes) = bytes {
                        let mut hasher = Sha256::new();
                        hasher.update(bytes);
                        let hash = hasher.finalize();
                        let hash = &hash[..20];
                        let address: String = hash.iter().map(|byte| format!("{:02x}", byte)).collect();
                        let address = address.to_uppercase();
                        let moniker = &validator.description.moniker;
                        moniker_map.insert(address, moniker.clone());
                    }
                }
            }
        }

        for validator in validators {
            debug!("Sei Validator: {:?}", validator);
            let moniker = moniker_map
                .get(&validator.address)
                .map(|m| m.as_str())
                .unwrap_or(&validator.address); // Fallback to address if moniker not found

            COMETBFT_VALIDATOR
                .with_label_values(&[
                    &validator.address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &alert_addresses.contains(&validator.address).to_string(),
                ])
                .set(0.0);
            COMETBFT_VALIDATOR_VOTING_POWER
                .with_label_values(&[
                    &validator.address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &alert_addresses.contains(&validator.address).to_string(),
                ])
                .set(validator.voting_power.parse::<i64>().unwrap_or(0) as f64);
            COMETBFT_VALIDATOR_PROPOSER_PRIORITY
                .with_label_values(&[
                    &validator.address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &alert_addresses.contains(&validator.address).to_string(),
                ])
                .set(validator.proposer_priority.parse::<i64>().unwrap_or(0) as f64);

            // Set Tendermint metrics with monikers (same as CometBFT does when Tendermint staking is disabled)
            // Only set if Tendermint staking is disabled to avoid conflicts
            if !self.app_context.config.network.tendermint.staking.enabled {
                TENDERMINT_VALIDATOR
                    .with_label_values(&[
                        moniker,
                        &validator.address,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        &alert_addresses.contains(&validator.address).to_string(),
                    ])
                    .set(0);

                // Tokens (voting power)
                if let Ok(tokens_val) = validator.voting_power.parse::<f64>() {
                    TENDERMINT_VALIDATOR_TOKENS
                        .with_label_values(&[
                            moniker,
                            &validator.address,
                            &self.app_context.chain_id,
                            &self.app_context.config.general.network,
                        ])
                        .set(tokens_val);
                }
            }
        }

        Ok(())
    }

    /// Fetch validators from Tendermint staking endpoint to get monikers
    async fn fetch_tendermint_validators(
        &self,
        lcd_client: &crate::core::clients::http_client::NodePool,
    ) -> anyhow::Result<Vec<Validator>> {
        let mut pagination_key: Option<String> = None;
        let mut validators: Vec<Validator> = Vec::new();

        loop {
            let mut url = "/cosmos/staking/v1beta1/validators".to_string();
            if let Some(key) = &pagination_key {
                url = format!("{}?pagination.key={}", url, encode(key));
            }
            let res = lcd_client
                .get(Path::from(url))
                .await
                .map_err(|e| anyhow::anyhow!(format!("NodePool error: {e}")))?;
            let rest_validator_response = from_str::<ValidatorsResponse>(&res)
                .context("Could not deserialize REST validators response")?;
            pagination_key = rest_validator_response.pagination.next_key;
            validators.extend(rest_validator_response.validators);
            if pagination_key.is_none() {
                break;
            }
        }
        Ok(validators)
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.rpc.is_none() {
        anyhow::bail!("Config is missing RPC node pool");
    }
    Ok(Box::new(Validators::new(app_context)))
}

#[async_trait]
impl RunnableModule for Validators {
    async fn run(&mut self) -> anyhow::Result<()> {
        info!("(Sei Validators) Fetching validators");
        self.fetch_sei_validators().await?;
        Ok(())
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.sei.validators.interval as u64,
        )
    }

    fn name(&self) -> &'static str {
        "Sei Validators"
    }
}
