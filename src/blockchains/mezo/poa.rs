use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;

use serde_json::from_str;
use sha2::{Digest, Sha256};
use tracing::info;

use crate::{
    blockchains::{mezo::types::MezoRESTResponse, tendermint::metrics::TENDERMINT_VALIDATOR},
    core::{app_context::AppContext, clients::path::Path, exporter::RunnableModule},
};

use super::types::MezoRESTValidator;

pub struct Poa {
    app_context: Arc<AppContext>,
}

impl Poa {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn get_rest_validators(&self, path: &str) -> anyhow::Result<Vec<MezoRESTValidator>> {
        info!("(Mezo POA) Fetching REST validators");

        let client = self.app_context.lcd.as_ref().unwrap();

        let res = client
            .get(Path::from(path))
            .await
            .context("Could not fetch REST validators")?;
        let rest_validator_response = from_str::<MezoRESTResponse>(&res)
            .context("Could not deserialize validators REST response")?;

        Ok(rest_validator_response.validators)
    }

    async fn process_validators(&mut self) -> anyhow::Result<()> {
        let rest_validators = self
            .get_rest_validators("/mezo/poa/v1/validators")
            .await
            .context("Could not obtain REST validators")?;

        info!("(Mezo POA) Processing REST validators");
        for validator in rest_validators {
            let (_, hash) = bech32::decode(&validator.cons_pub_key_bech32)
                .context("Could not decode validator address into bech32")?;

            let mut hasher = Sha256::new();

            // Process the input data
            hasher.update(&hash[5..]);

            let hash = hasher.finalize();
            let hash = &hash[0..20];

            let address: String = hash.iter().map(|byte| format!("{:02x}", byte)).collect();
            let address = address.to_uppercase();

            let moniker = &validator.description.moniker;

            TENDERMINT_VALIDATOR
                .with_label_values(&[
                    moniker,
                    &address,
                    &self.app_context.config.general.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(0);
        }

        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.lcd.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    Ok(Box::new(Poa::new(app_context)))
}

#[async_trait]
impl RunnableModule for Poa {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_validators()
            .await
            .context("Could not process validators")
    }

    fn name(&self) -> &'static str {
        "Mezo POA"
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.app_context.config.network.mezo.poa.interval as u64)
    }
}
