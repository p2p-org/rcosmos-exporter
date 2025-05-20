use std::sync::Arc;
use anyhow::Context;
use async_trait::async_trait;
use tracing::info;

use crate::{
    blockchains::namada::types::{Validator},
    core::{
        clients::{blockchain_client::BlockchainClient, path::Path},
        exporter::Task
    },
};

pub struct NamadaValidatorInfoScrapper {
    client: Arc<BlockchainClient>,
    validator_alert_addresses: Vec<String>,
}

impl NamadaValidatorInfoScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        _chain_id: crate::core::chain_id::ChainId,
        _network: String,
        validator_alert_addresses: Vec<String>,
    ) -> Self {
        Self {
            client,
            validator_alert_addresses,
        }
    }

    async fn get_validators(&self) -> anyhow::Result<Vec<Validator>> {
        let res = self
            .client
            .with_rest()
            .get(Path::ensure_leading_slash("/api/v1/pos/validator/all"))
            .await
            .context("Could not fetch validators")?;
        Ok(serde_json::from_str(&res)?)
    }

    async fn process_validators(&mut self) -> anyhow::Result<()> {
        let validators = self.get_validators().await?;
        info!("(Namada Validator Info Scrapper) Processing validators");
        for validator in &validators {
            let _fires_alerts = self
                .validator_alert_addresses
                .contains(&validator.address)
                .to_string();
            info!("Validator: {}", validator.address);
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
