use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use base64::engine::general_purpose;
use base64::Engine;
use serde_json::from_str;
use sha2::{Digest, Sha256};
use tracing::info;
use urlencoding::encode;

use crate::blockchains::tendermint::metrics::{
    TENDERMINT_STAKING_PARAM_BOND_DENOM, TENDERMINT_STAKING_PARAM_HISTORICAL_ENTRIES,
    TENDERMINT_STAKING_PARAM_MAX_ENTRIES, TENDERMINT_STAKING_PARAM_MAX_VALIDATORS,
    TENDERMINT_STAKING_PARAM_MIN_COMMISSION_RATE, TENDERMINT_STAKING_PARAM_UNBONDING_TIME,
    TENDERMINT_STAKING_POOL_BONDED_TOKENS, TENDERMINT_STAKING_POOL_NOT_BONDED_TOKENS,
    TENDERMINT_VALIDATOR, TENDERMINT_VALIDATOR_COMMISSION_MAX_CHANGE_RATE,
    TENDERMINT_VALIDATOR_COMMISSION_MAX_RATE, TENDERMINT_VALIDATOR_COMMISSION_RATE,
    TENDERMINT_VALIDATOR_DELEGATIONS, TENDERMINT_VALIDATOR_DELEGATOR_SHARES,
    TENDERMINT_VALIDATOR_JAILED, TENDERMINT_VALIDATOR_TOKENS,
    TENDERMINT_VALIDATOR_UNBONDING_DELEGATIONS,
};
use crate::blockchains::tendermint::types::{
    Delegation, DelegationResponse, PoolResponse, StakingParamsResponse, UnbondingDelegation,
    UnbondingDelegationResponse, Validator, ValidatorsResponse,
};
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;

pub struct Staking {
    app_context: Arc<AppContext>,
    monikers: HashMap<String, String>,
}

impl Staking {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self {
            app_context,
            monikers: HashMap::new(),
        }
    }

    async fn fetch_validators(&self, path: &str) -> anyhow::Result<Vec<Validator>> {
        info!("(Tendermint Staking) Fetching REST validators");
        let mut pagination_key: Option<String> = None;
        let mut validators: Vec<Validator> = Vec::new();

        let client = self.app_context.lcd.as_ref().unwrap();

        loop {
            let mut url = path.to_string();
            if let Some(key) = &pagination_key {
                url = format!("{}?pagination.key={}", path, encode(key));
            }
            let res = client
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

    async fn get_validator_delegations_count(
        &self,
        validator_address: &str,
    ) -> anyhow::Result<usize> {
        let mut pagination_key: Option<String> = None;
        let mut delegations: Vec<Delegation> = Vec::new();

        let client = self.app_context.lcd.as_ref().unwrap();
        loop {
            let mut url = format!(
                "/cosmos/staking/v1beta1/validators/{}/delegations?pagination.limit=100000",
                validator_address
            );
            if let Some(key) = &pagination_key {
                let encoded_key = general_purpose::STANDARD.encode(key);
                url = format!("{}?pagination.key={}", url, encoded_key);
            }

            let res = client
                .get(Path::from(url))
                .await
                .context("Could not fetch validator delegation")?;

            let res = from_str::<DelegationResponse>(&res)
                .context("Could not deserialize delegations response")?;

            pagination_key = res.pagination.next_key;

            delegations.extend(res.delegation_responses);
            if pagination_key.is_none() {
                break;
            }
        }

        Ok(delegations.len())
    }

    async fn get_validator_unbonding_delegations_count(
        &self,
        validator_address: &str,
    ) -> anyhow::Result<usize> {
        let mut pagination_key: Option<String> = None;
        let mut delegations: Vec<UnbondingDelegation> = Vec::new();

        let client = self.app_context.lcd.as_ref().unwrap();
        loop {
            let mut url = format!(
                "/cosmos/staking/v1beta1/validators/{}/unbonding_delegations",
                validator_address
            );
            if let Some(key) = &pagination_key {
                let encoded_key = general_purpose::STANDARD.encode(key);
                url = format!("{}?pagination.key={}", url, encoded_key);
            }

            let res = client
                .get(Path::from(url))
                .await
                .context("Could not fetch validator delegation")?;

            let res = from_str::<UnbondingDelegationResponse>(&res)
                .context("Could not deserialize delegations response")?;

            pagination_key = res.pagination.next_key;

            delegations.extend(res.unbonding_responses);
            if pagination_key.is_none() {
                break;
            }
        }

        Ok(delegations.len())
    }

    async fn get_params(&self) -> anyhow::Result<()> {
        info!("(Tendermint Staking) Getting staking params");
        let client = self.app_context.lcd.as_ref().unwrap();
        let network = &self.app_context.config.general.network;

        let res = client
            .get(Path::from("/cosmos/staking/v1beta1/params"))
            .await
            .map_err(|e| anyhow::anyhow!(format!("NodePool error: {e}")))?;
        let params_response: StakingParamsResponse =
            from_str(&res).context("Could not deserialize staking params response")?;
        let params = params_response.params;

        // Parse unbonding_time as seconds (e.g., "1814400s" -> 1814400)
        let unbonding_time_secs = params
            .unbonding_time
            .trim_end_matches('s')
            .parse::<f64>()
            .unwrap_or(0.0);
        TENDERMINT_STAKING_PARAM_UNBONDING_TIME
            .with_label_values(&[&self.app_context.chain_id, network])
            .set(unbonding_time_secs);
        TENDERMINT_STAKING_PARAM_MAX_VALIDATORS
            .with_label_values(&[&self.app_context.chain_id, network])
            .set(params.max_validators as f64);
        TENDERMINT_STAKING_PARAM_MAX_ENTRIES
            .with_label_values(&[&self.app_context.chain_id, network])
            .set(params.max_entries as f64);
        TENDERMINT_STAKING_PARAM_HISTORICAL_ENTRIES
            .with_label_values(&[&self.app_context.chain_id, network])
            .set(params.historical_entries as f64);
        TENDERMINT_STAKING_PARAM_BOND_DENOM
            .with_label_values(&[&self.app_context.chain_id, network, &params.bond_denom])
            .set(0.0);
        let min_commission_rate = params.min_commission_rate.parse::<f64>().unwrap_or(0.0);
        TENDERMINT_STAKING_PARAM_MIN_COMMISSION_RATE
            .with_label_values(&[&self.app_context.chain_id, network])
            .set(min_commission_rate);
        Ok(())
    }

    async fn get_validators(&mut self) -> anyhow::Result<()> {
        let rest_validators = self
            .fetch_validators("/cosmos/staking/v1beta1/validators")
            .await
            .context("Could not obtain REST validators")?;

        let alerts = self.app_context.config.general.alerting.validators.clone();
        info!("(Tendermint Staking) Processing REST validators");
        for validator in rest_validators {
            let bytes = general_purpose::STANDARD
                .decode(&validator.consensus_pubkey.key)
                .context("Could not validator pub key")?;

            let mut hasher = Sha256::new();
            hasher.update(bytes);
            let hash = hasher.finalize();
            let hash = &hash[..20];

            let address: String = hash.iter().map(|byte| format!("{:02x}", byte)).collect();
            let address = address.to_uppercase();
            let moniker = &validator.description.moniker;
            let chain_id = &self.app_context.chain_id;
            let network = &self.app_context.config.general.network;

            // Moniker tracking and cleanup
            if let Some(old_moniker) = self.monikers.get(&address) {
                if old_moniker != moniker {
                    // Remove all metrics for the old moniker/address/chain_id/network
                    let _ = TENDERMINT_VALIDATOR.remove_label_values(&[
                        old_moniker,
                        &address,
                        chain_id,
                        network,
                        &alerts.contains(&address).to_string(),
                    ]);
                    let _ = TENDERMINT_VALIDATOR_DELEGATOR_SHARES.remove_label_values(&[
                        old_moniker,
                        &address,
                        chain_id,
                        network,
                    ]);
                    let _ = TENDERMINT_VALIDATOR_TOKENS.remove_label_values(&[
                        old_moniker,
                        &address,
                        chain_id,
                        network,
                    ]);
                    let _ = TENDERMINT_VALIDATOR_JAILED.remove_label_values(&[
                        old_moniker,
                        &address,
                        chain_id,
                        network,
                        &alerts.contains(&address).to_string(),
                    ]);
                    let _ = TENDERMINT_VALIDATOR_DELEGATIONS.remove_label_values(&[
                        old_moniker,
                        &address,
                        chain_id,
                        network,
                    ]);
                    let _ = TENDERMINT_VALIDATOR_UNBONDING_DELEGATIONS.remove_label_values(&[
                        old_moniker,
                        &address,
                        chain_id,
                        network,
                    ]);
                    let _ = TENDERMINT_VALIDATOR_COMMISSION_RATE.remove_label_values(&[
                        old_moniker,
                        &address,
                        chain_id,
                        network,
                    ]);
                    let _ = TENDERMINT_VALIDATOR_COMMISSION_MAX_RATE.remove_label_values(&[
                        old_moniker,
                        &address,
                        chain_id,
                        network,
                    ]);
                    let _ = TENDERMINT_VALIDATOR_COMMISSION_MAX_CHANGE_RATE.remove_label_values(&[
                        old_moniker,
                        &address,
                        chain_id,
                        network,
                    ]);
                }
            }
            // Update the moniker map
            self.monikers.insert(address.clone(), moniker.clone());

            info!(
                "(Tendermint Staking) Getting {} delegations",
                validator.operator_address
            );
            let delegations_count = self
                .get_validator_delegations_count(&validator.operator_address)
                .await
                .context("Could not get validator delegations count")?;
            info!(
                "(Tendermint Staking) Getting {} unbonding delegations",
                validator.operator_address
            );
            let unbonding_delegations_count = self
                .get_validator_unbonding_delegations_count(&validator.operator_address)
                .await
                .context("Could not get validator unbonding delegations count")?;
            let tokens: f64 = validator
                .tokens
                .parse()
                .context("Could not parse validator tokens")?;
            let delegator_shares: f64 = validator
                .delegator_shares
                .parse()
                .context("Could not parse validator shares")?;
            let jailed = validator.jailed;

            TENDERMINT_VALIDATOR
                .with_label_values(&[
                    moniker,
                    &address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &alerts.contains(&address).to_string(),
                ])
                .set(0);
            TENDERMINT_VALIDATOR_DELEGATOR_SHARES
                .with_label_values(&[
                    moniker,
                    &address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(delegator_shares);
            TENDERMINT_VALIDATOR_TOKENS
                .with_label_values(&[
                    moniker,
                    &address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(tokens);
            TENDERMINT_VALIDATOR_JAILED
                .with_label_values(&[
                    moniker,
                    &address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &alerts.contains(&address).to_string(),
                ])
                .set(if jailed { 1 } else { 0 });
            TENDERMINT_VALIDATOR_DELEGATIONS
                .with_label_values(&[
                    moniker,
                    &address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(delegations_count as f64);
            TENDERMINT_VALIDATOR_UNBONDING_DELEGATIONS
                .with_label_values(&[
                    moniker,
                    &address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(unbonding_delegations_count as f64);
            TENDERMINT_VALIDATOR_COMMISSION_RATE
                .with_label_values(&[
                    moniker,
                    &address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(
                    validator
                        .commission
                        .commission_rates
                        .rate
                        .parse::<f64>()
                        .unwrap_or(0.0),
                );
            TENDERMINT_VALIDATOR_COMMISSION_MAX_RATE
                .with_label_values(&[
                    moniker,
                    &address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(
                    validator
                        .commission
                        .commission_rates
                        .max_rate
                        .parse::<f64>()
                        .unwrap_or(0.0),
                );
            TENDERMINT_VALIDATOR_COMMISSION_MAX_CHANGE_RATE
                .with_label_values(&[
                    moniker,
                    &address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(
                    validator
                        .commission
                        .commission_rates
                        .max_change_rate
                        .parse::<f64>()
                        .unwrap_or(0.0),
                );
        }
        Ok(())
    }

    async fn get_pool(&self) -> anyhow::Result<()> {
        info!("(Tendermint Staking) Getting pool");
        let client = self.app_context.lcd.as_ref().unwrap();
        let network = &self.app_context.config.general.network;
        let chain_id = &self.app_context.chain_id;

        let res = client
            .get(Path::from("/cosmos/staking/v1beta1/pool"))
            .await
            .map_err(|e| anyhow::anyhow!(format!("NodePool error: {e}")))?;
        let pool_response: PoolResponse =
            from_str(&res).context("Could not deserialize staking pool response")?;
        let pool = pool_response.pool;

        let bonded_tokens = pool.bonded_tokens.parse::<f64>().unwrap_or(0.0);
        let not_bonded_tokens = pool.not_bonded_tokens.parse::<f64>().unwrap_or(0.0);
        TENDERMINT_STAKING_POOL_BONDED_TOKENS
            .with_label_values(&[chain_id, network])
            .set(bonded_tokens);
        TENDERMINT_STAKING_POOL_NOT_BONDED_TOKENS
            .with_label_values(&[chain_id, network])
            .set(not_bonded_tokens);
        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.lcd.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    Ok(Box::new(Staking::new(app_context)))
}

#[async_trait]
impl RunnableModule for Staking {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.get_params()
            .await
            .context("Failed to obtain staking params")?;
        self.get_validators()
            .await
            .context("Failed to process validators")?;
        self.get_pool()
            .await
            .context("Failed to obtain and set staking pool metrics")
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.tendermint.staking.interval as u64,
        )
    }
    fn name(&self) -> &'static str {
        "Tendermint Staking"
    }
}
