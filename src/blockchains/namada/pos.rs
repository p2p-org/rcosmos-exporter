use crate::blockchains::cometbft::metrics::COMETBFT_VALIDATOR_VOTING_POWER;
use crate::blockchains::namada::types::RestValidator;
use crate::blockchains::tendermint::metrics::{
    TENDERMINT_VALIDATOR, TENDERMINT_VALIDATOR_COMMISSION_MAX_RATE,
    TENDERMINT_VALIDATOR_COMMISSION_RATE, TENDERMINT_VALIDATOR_DELEGATOR_SHARES,
    TENDERMINT_VALIDATOR_JAILED, TENDERMINT_VALIDATOR_REWARDS, TENDERMINT_VALIDATOR_TOKENS,
    TENDERMINT_VALIDATOR_UNBONDING_DELEGATIONS,
};
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;
use anyhow::Context;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;

pub struct Pos {
    pub app_context: Arc<AppContext>,
}

impl Pos {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn get_validators(&self) -> anyhow::Result<Vec<RestValidator>> {
        let client = self.app_context.lcd.as_ref().unwrap();
        let res = client
            .get(Path::from("/api/v1/pos/validator/all"))
            .await
            .context("Failed to fetch validators")?;
        let validators: Vec<RestValidator> =
            serde_json::from_str(&res).context("Failed to parse validators")?;
        Ok(validators)
    }

    async fn process_validators(&mut self) -> anyhow::Result<()> {
        let validators = self.get_validators().await?;
        let client = self.app_context.lcd.as_ref().unwrap();
        info!("(Namada Pos) Processing validators");
        for validator in validators {
            let name = &validator.address;
            let address = &validator.address;
            info!("(Namada Pos) Validator: {}", address);

            // Set TENDERMINT_VALIDATORS per-validator
            TENDERMINT_VALIDATOR
                .with_label_values(&[
                    name,
                    address,
                    &self.app_context.chain_id.to_string(),
                    &self.app_context.config.general.network,
                    &self
                        .app_context
                        .config
                        .general
                        .alerting
                        .validators
                        .contains(address)
                        .to_string(),
                ])
                .set(0);
            // Voting power
            if let Some(voting_power_str) = validator.voting_power.as_ref() {
                if let Ok(voting_power) = voting_power_str.parse::<f64>() {
                    COMETBFT_VALIDATOR_VOTING_POWER
                        .with_label_values(&[
                            address,
                            &self.app_context.chain_id.to_string(),
                            &self.app_context.config.general.network,
                            &self
                                .app_context
                                .config
                                .general
                                .alerting
                                .validators
                                .contains(address)
                                .to_string(),
                        ])
                        .set(voting_power as i64);
                }
            }
            // Jailed status
            let is_jailed = validator.state.as_deref() == Some("jailed");
            TENDERMINT_VALIDATOR_JAILED
                .with_label_values(&[
                    name,
                    address,
                    &self.app_context.chain_id.to_string(),
                    &self.app_context.config.general.network,
                    &self
                        .app_context
                        .config
                        .general
                        .alerting
                        .validators
                        .contains(address)
                        .to_string(),
                ])
                .set(if is_jailed { 1 } else { 0 });
            // Tokens (if available)
            if let Some(tokens) = validator.voting_power.as_ref() {
                if let Ok(tokens_val) = tokens.parse::<f64>() {
                    TENDERMINT_VALIDATOR_TOKENS
                        .with_label_values(&[
                            name,
                            address,
                            &self.app_context.chain_id.to_string(),
                            &self.app_context.config.general.network,
                        ])
                        .set(tokens_val);
                }
            }
            // Commission rate
            if let Some(commission) = validator.commission.as_ref() {
                if let Ok(commission_val) = commission.parse::<f64>() {
                    TENDERMINT_VALIDATOR_COMMISSION_RATE
                        .with_label_values(&[
                            name,
                            address,
                            &self.app_context.chain_id.to_string(),
                            &self.app_context.config.general.network,
                        ])
                        .set(commission_val);
                }
            }
            // Max commission rate
            if let Some(max_commission) = validator.max_commission.as_ref() {
                if let Ok(max_commission_val) = max_commission.parse::<f64>() {
                    TENDERMINT_VALIDATOR_COMMISSION_MAX_RATE
                        .with_label_values(&[
                            name,
                            address,
                            &self.app_context.chain_id.to_string(),
                            &self.app_context.config.general.network,
                        ])
                        .set(max_commission_val);
                }
            }
            // Delegator shares (simulate as voting_power for now if not available)
            if let Some(voting_power_str) = validator.voting_power.as_ref() {
                if let Ok(shares) = voting_power_str.parse::<f64>() {
                    TENDERMINT_VALIDATOR_DELEGATOR_SHARES
                        .with_label_values(&[
                            name,
                            address,
                            &self.app_context.chain_id.to_string(),
                            &self.app_context.config.general.network,
                        ])
                        .set(shares);
                }
            }
            // Delegations (bonds)
            let bonds_url = format!("/api/v1/pos/bond/{}", address);
            if let Ok(bonds_res) = client.get(Path::from(bonds_url)).await {
                if let Ok(bonds_json) = serde_json::from_str::<serde_json::Value>(&bonds_res) {
                    let delegations = bonds_json["results"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    TENDERMINT_VALIDATOR_TOKENS
                        .with_label_values(&[
                            name,
                            address,
                            &self.app_context.chain_id.to_string(),
                            &self.app_context.config.general.network,
                        ])
                        .set(delegations as f64);
                }
            }
            // Unbonding delegations
            let unbonds_url = format!("/api/v1/pos/unbond/{}", address);
            if let Ok(unbonds_res) = client.get(Path::from(unbonds_url)).await {
                if let Ok(unbonds_json) = serde_json::from_str::<serde_json::Value>(&unbonds_res) {
                    let unbondings = unbonds_json["results"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    TENDERMINT_VALIDATOR_UNBONDING_DELEGATIONS
                        .with_label_values(&[
                            name,
                            address,
                            &self.app_context.chain_id.to_string(),
                            &self.app_context.config.general.network,
                        ])
                        .set(unbondings as f64);
                }
            }
            // Rewards
            let rewards_url = format!("/api/v1/pos/reward/{}", address);
            if let Ok(rewards_res) = client.get(Path::from(rewards_url)).await {
                if let Ok(rewards_json) = serde_json::from_str::<serde_json::Value>(&rewards_res) {
                    if let Some(rewards) = rewards_json.as_array() {
                        for reward in rewards {
                            if let Some(min_denom_amount) =
                                reward.get("minDenomAmount").and_then(|v| v.as_str())
                            {
                                if let Ok(amount) = min_denom_amount.parse::<f64>() {
                                    TENDERMINT_VALIDATOR_REWARDS
                                        .with_label_values(&[
                                            name,
                                            address,
                                            "NAM", // denom, adjust as needed
                                            &self.app_context.chain_id.to_string(),
                                            &self.app_context.config.general.network,
                                        ])
                                        .set(amount);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.lcd.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    Ok(Box::new(Pos::new(app_context)))
}

#[async_trait]
impl RunnableModule for Pos {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_validators().await
    }
    fn name(&self) -> &'static str {
        "Namada Pos"
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.app_context.config.network.namada.pos.interval)
    }
}
