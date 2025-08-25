use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use serde_json::from_str;
use tracing::{debug, info};

use crate::blockchains::cometbft::metrics::{
    COMETBFT_VALIDATOR, COMETBFT_VALIDATOR_PROPOSER_PRIORITY, COMETBFT_VALIDATOR_VOTING_POWER,
};
use crate::blockchains::cometbft::types::{Validator, ValidatorsResponse};
use crate::blockchains::tendermint::metrics::{TENDERMINT_VALIDATOR, TENDERMINT_VALIDATOR_TOKENS};
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;

pub struct Validators {
    pub app_context: Arc<AppContext>,
}

impl Validators {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn fetch_cometbft_validators(&self) -> anyhow::Result<()> {
        let mut validators: Vec<Validator> = Vec::new();
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

            let validators_response =
                from_str::<ValidatorsResponse>(&res).context("Could not decode JSON response")?;

            if let Some(res) = validators_response.result {
                let count = res.count.parse::<usize>().context(
                    "Could not parse the count of obtained validators when fetching validators",
                )?;
                let total = res
                    .total
                    .parse::<usize>()
                    .context("Could not parse the total of validators when fetching validators")?;
                if count + validators.len() == total {
                    all_fetched = true;
                } else {
                    page += 1;
                }
                validators.extend(res.validators)
            } else {
                anyhow::bail!("Result key not present at validators rpc endpoint response");
            }
        }

        let alert_addresses = self.app_context.config.general.alerting.validators.clone();

        for validator in validators {
            debug!("Validator: {:?}", validator);
            COMETBFT_VALIDATOR
                .with_label_values(&[
                    &validator.address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &alert_addresses.contains(&validator.address).to_string(),
                ])
                .set(0);
            COMETBFT_VALIDATOR_VOTING_POWER
                .with_label_values(&[
                    &validator.address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &alert_addresses.contains(&validator.address).to_string(),
                ])
                .set(validator.voting_power.parse::<i64>().unwrap_or(0));
            COMETBFT_VALIDATOR_PROPOSER_PRIORITY
                .with_label_values(&[
                    &validator.address,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &alert_addresses.contains(&validator.address).to_string(),
                ])
                .set(validator.proposer_priority.parse::<i64>().unwrap_or(0));

            // Only set Tendermint metrics if CometBFT validators is enabled AND Tendermint staking is disabled
            // (to avoid conflicts when both modules are enabled)
            if self.app_context.config.network.cometbft.validators.enabled
                && !self.app_context.config.network.tendermint.staking.enabled
            {
                // Set Tendermint metrics for compatibility (address as moniker)
                TENDERMINT_VALIDATOR
                    .with_label_values(&[
                        &validator.address,
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
                            &validator.address,
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
        info!("(CometBFT Validators) Fetching validators");
        self.fetch_cometbft_validators().await?;
        Ok(())
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.cometbft.validators.interval as u64,
        )
    }

    fn name(&self) -> &'static str {
        "CometBFT Validators"
    }
}
