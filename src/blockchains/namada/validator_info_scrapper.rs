use anyhow::Context;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;

use crate::{
    blockchains::namada::metrics::{
        TENDERMINT_VALIDATORS, TENDERMINT_VALIDATOR_COMMISSION_MAX_RATE,
        TENDERMINT_VALIDATOR_COMMISSION_RATE, TENDERMINT_VALIDATOR_DELEGATIONS,
        TENDERMINT_VALIDATOR_DELEGATOR_SHARES, TENDERMINT_VALIDATOR_JAILED,
        TENDERMINT_VALIDATOR_REWARDS, TENDERMINT_VALIDATOR_TOKENS,
        TENDERMINT_VALIDATOR_UNBONDING_DELEGATIONS, TENDERMINT_VALIDATOR_VOTING_POWER,
    },
    blockchains::namada::types::RestValidator,
    core::{clients::blockchain_client::BlockchainClient, clients::path::Path, exporter::Task},
};

pub struct NamadaValidatorInfoScrapper {
    client: Arc<BlockchainClient>,
    chain_id: crate::core::chain_id::ChainId,
    network: String,
    validator_alert_addresses: Vec<String>,
}

impl NamadaValidatorInfoScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        chain_id: crate::core::chain_id::ChainId,
        network: String,
        validator_alert_addresses: Vec<String>,
    ) -> Self {
        Self {
            client,
            chain_id,
            network,
            validator_alert_addresses,
        }
    }

    async fn get_validators(&self) -> anyhow::Result<Vec<RestValidator>> {
        let res = self
            .client
            .with_rest()
            .get(Path::from(format!("/api/v1/pos/validator/all")))
            .await
            .context("Could not fetch validators")?;
        Ok(serde_json::from_str(&res)?)
    }

    async fn process_validators(&mut self) -> anyhow::Result<()> {
        let validators = self.get_validators().await?;
        info!("(Namada Validator Info Scrapper) Processing validators");
        for validator in validators {
            let fires_alerts = self
                .validator_alert_addresses
                .contains(&validator.address)
                .to_string();
            let name = validator.name.as_deref().unwrap_or("");
            let address = &validator.address;
            info!("Validator: {}", address);

            // Set TENDERMINT_VALIDATORS per-validator
            TENDERMINT_VALIDATORS
                .with_label_values(&[
                    name,
                    address,
                    &self.chain_id.to_string(),
                    &self.network,
                    &fires_alerts,
                ])
                .set(0);
            // Voting power
            if let Some(voting_power_str) = validator.voting_power.as_ref() {
                if let Ok(voting_power) = voting_power_str.parse::<f64>() {
                    TENDERMINT_VALIDATOR_VOTING_POWER
                        .with_label_values(&[address, &self.chain_id.to_string(), &self.network])
                        .set(voting_power as i64);
                }
            }
            // Jailed status
            let is_jailed = validator.state.as_deref() == Some("jailed");
            TENDERMINT_VALIDATOR_JAILED
                .with_label_values(&[
                    name,
                    address,
                    &self.chain_id.to_string(),
                    &self.network,
                    &fires_alerts,
                ])
                .set(if is_jailed { 1 } else { 0 });
            // Tokens (if available)
            if let Some(tokens) = validator.voting_power.as_ref() {
                if let Ok(tokens_val) = tokens.parse::<f64>() {
                    TENDERMINT_VALIDATOR_TOKENS
                        .with_label_values(&[
                            name,
                            address,
                            &self.chain_id.to_string(),
                            &self.network,
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
                            &self.chain_id.to_string(),
                            &self.network,
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
                            &self.chain_id.to_string(),
                            &self.network,
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
                            &self.chain_id.to_string(),
                            &self.network,
                        ])
                        .set(shares);
                }
            }
            // Delegations (bonds)
            let bonds_url = format!("/api/v1/pos/bond/{}", address);
            if let Ok(bonds_res) = self.client.with_rest().get(Path::from(bonds_url)).await {
                if let Ok(bonds_json) = serde_json::from_str::<serde_json::Value>(&bonds_res) {
                    let delegations = bonds_json["results"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    TENDERMINT_VALIDATOR_DELEGATIONS
                        .with_label_values(&[
                            name,
                            address,
                            &self.chain_id.to_string(),
                            &self.network,
                        ])
                        .set(delegations as f64);
                }
            }
            // Unbonding delegations
            let unbonds_url = format!("/api/v1/pos/unbond/{}", address);
            if let Ok(unbonds_res) = self.client.with_rest().get(Path::from(unbonds_url)).await {
                if let Ok(unbonds_json) = serde_json::from_str::<serde_json::Value>(&unbonds_res) {
                    let unbondings = unbonds_json["results"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    TENDERMINT_VALIDATOR_UNBONDING_DELEGATIONS
                        .with_label_values(&[
                            name,
                            address,
                            &self.chain_id.to_string(),
                            &self.network,
                        ])
                        .set(unbondings as f64);
                }
            }
            // Rewards
            let rewards_url = format!("/api/v1/pos/reward/{}", address);
            if let Ok(rewards_res) = self.client.with_rest().get(Path::from(rewards_url)).await {
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
                                            &self.chain_id.to_string(),
                                            &self.network,
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

#[async_trait]
impl Task for NamadaValidatorInfoScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_validators().await
    }
    fn name(&self) -> &'static str {
        "Namada Validator Info Scrapper"
    }
}
