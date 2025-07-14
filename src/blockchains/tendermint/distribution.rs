use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use async_trait::async_trait;
use base64::engine::general_purpose;
use base64::Engine;
use serde_json::from_str;
use tracing::info;
use urlencoding::encode;

use crate::blockchains::tendermint::metrics::{
    TENDERMINT_VALIDATOR_COMMISSIONS, TENDERMINT_VALIDATOR_REWARDS, TENDERMINT_VALIDATOR_SLASHES,
};
use crate::blockchains::tendermint::types::{
    CommissionResponse, RewardsResponse, Validator, ValidatorSlashesResponse, ValidatorsResponse,
};
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;

pub struct Distribution {
    app_context: Arc<AppContext>,
}

impl Distribution {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn fetch_validators(&self, path: &str) -> anyhow::Result<Vec<Validator>> {
        info!("(Tendermint Distribution) Fetching REST validators");
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

    async fn get_validator_reward(
        &self,
        validator_address: &str,
    ) -> anyhow::Result<HashMap<String, f64>> {
        let url = format!(
            "/cosmos/distribution/v1beta1/validators/{}/outstanding_rewards",
            validator_address
        );
        let res = self
            .app_context
            .lcd
            .as_ref()
            .unwrap()
            .get(Path::from(url))
            .await
            .context("Could not fetch validator reward")?;

        let rewards = from_str::<RewardsResponse>(&res)
            .context("Could not deserialize validator reward")?
            .rewards
            .rewards;

        let mut rewards_map = HashMap::new();

        for reward in rewards {
            rewards_map.insert(
                reward.denom,
                reward
                    .amount
                    .parse::<f64>()
                    .context("Could not parse reward amount")?,
            );
        }
        Ok(rewards_map)
    }

    async fn get_validator_slashes_count(&self, validator_address: &str) -> anyhow::Result<usize> {
        let mut pagination_key: Option<String> = None;
        let mut slashes_count = 0;

        loop {
            let mut url = format!(
                "/cosmos/distribution/v1beta1/validators/{}/slashes",
                validator_address
            );
            if let Some(key) = &pagination_key {
                let encoded_key = encode(key);
                url = format!("{}?pagination.key={}", url, encoded_key);
            }

            let res = self
                .app_context
                .lcd
                .as_ref()
                .unwrap()
                .get(Path::from(url))
                .await
                .context("Could not fetch validator slashes")?;

            let res = from_str::<ValidatorSlashesResponse>(&res)
                .context("Could not deserialize validator slashes")?;

            pagination_key = res.pagination.next_key;
            slashes_count += res.slashes.len();
            if pagination_key.is_none() {
                break;
            }
        }

        Ok(slashes_count)
    }

    async fn get_validator_commission(
        &self,
        validator_address: &str,
    ) -> anyhow::Result<HashMap<String, f64>> {
        let url = format!(
            "/cosmos/distribution/v1beta1/validators/{}/commission",
            validator_address
        );
        let res = self
            .app_context
            .lcd
            .as_ref()
            .unwrap()
            .get(Path::from(url))
            .await
            .context("Could not fetch validator commission")?;

        let commissions = from_str::<CommissionResponse>(&res)
            .context("Could not deserialize validator commission")?
            .commission
            .commission;

        let mut commission_map = HashMap::new();

        for commission in commissions {
            commission_map.insert(
                commission.denom,
                commission
                    .amount
                    .parse::<f64>()
                    .context("Could not parse commission amount")?,
            );
        }
        Ok(commission_map)
    }

    async fn get_distribution(&self) -> anyhow::Result<()> {
        let validators = self
            .fetch_validators("/cosmos/staking/v1beta1/validators")
            .await?;
        let network = &self.app_context.config.general.network;
        for validator in &validators {
            let moniker = &validator.description.moniker;
            let operator_address = &validator.operator_address;
            // Calculate consensus address (hex, uppercase)
            let bytes = general_purpose::STANDARD
                .decode(&validator.consensus_pubkey.key)
                .context("Could not decode validator pub key")?;

            let address = {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(bytes);
                let hash = hasher.finalize();
                let hash = &hash[..20];
                hash.iter()
                    .map(|byte| format!("{:02x}", byte))
                    .collect::<String>()
                    .to_uppercase()
            };

            // Rewards
            info!("(Tendermint Distribution) Getting {} rewards", address);
            let rewards = self.get_validator_reward(operator_address).await?;
            for (denom, amount) in rewards {
                TENDERMINT_VALIDATOR_REWARDS
                    .with_label_values(&[
                        moniker,
                        &address,
                        &denom,
                        &self.app_context.chain_id,
                        network,
                    ])
                    .set(amount);
            }

            // Commission
            info!("(Tendermint Distribution) Getting {} commissions", address);
            let commission = self.get_validator_commission(operator_address).await?;
            for (denom, amount) in commission {
                TENDERMINT_VALIDATOR_COMMISSIONS
                    .with_label_values(&[
                        moniker,
                        &address,
                        &denom,
                        &self.app_context.chain_id,
                        network,
                    ])
                    .set(amount);
            }

            // Slashes
            info!("(Tendermint Distribution) Getting {} slashes", address);
            let slashes_count = self.get_validator_slashes_count(operator_address).await?;
            TENDERMINT_VALIDATOR_SLASHES
                .with_label_values(&[moniker, &address, &self.app_context.chain_id, network])
                .set(slashes_count as f64);
        }
        Ok(())
    }
}

#[async_trait]
impl RunnableModule for Distribution {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.get_distribution()
            .await
            .context("Could not get distribution")
    }
    fn name(&self) -> &'static str {
        "Tendermint Distribution"
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context
                .config
                .network
                .tendermint
                .distribution
                .interval as u64,
        )
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.lcd.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    Ok(Box::new(Distribution::new(app_context)))
}
