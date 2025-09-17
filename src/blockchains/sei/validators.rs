use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use serde_json::from_str;
use tracing::{debug, info};

use crate::blockchains::sei::metrics::{
    COMETBFT_VALIDATOR, COMETBFT_VALIDATOR_PROPOSER_PRIORITY, COMETBFT_VALIDATOR_VOTING_POWER,
};
use crate::blockchains::sei::types::SeiValidatorsResponse;
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

        for validator in validators {
            debug!("Sei Validator: {:?}", validator);
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
