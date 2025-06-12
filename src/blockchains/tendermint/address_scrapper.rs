use crate::blockchains::tendermint::metrics::ADDRESS_BALANCE;
use crate::core::clients::blockchain_client::BlockchainClient;
use crate::core::clients::path::Path;
use crate::core::exporter::Task;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::env;
use std::sync::Arc;
use tracing::{debug, info};

pub struct AddressScrapper {
    client: Arc<BlockchainClient>,
    addresses: Vec<String>,
}

impl AddressScrapper {
    pub fn new(client: Arc<BlockchainClient>) -> Self {
        // Parse addresses during initialization
        let addresses = env::var("ADDRESS_MONITORS")
            .unwrap_or_default()
            .split(';')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        Self { client, addresses }
    }

    async fn update_address_balance(&self, address: &str) -> Result<()> {
        let response = self
            .client
            .with_rest()
            .get(Path::from(format!(
                "/cosmos/bank/v1beta1/balances/{}",
                address
            )))
            .await
            .context("Failed to fetch balance")?;

        let response: Value =
            serde_json::from_str(&response).context("Failed to parse balance response")?;

        let balances = response["balances"]
            .as_array()
            .context("Balances field not found or not an array")?;

        for balance in balances {
            let amount = balance["amount"]
                .as_str()
                .context("Amount field not found or not a string")?
                .parse::<f64>()
                .context("Failed to parse amount")?;

            ADDRESS_BALANCE.with_label_values(&[address]).set(amount);
        }

        Ok(())
    }
}

#[async_trait]
impl Task for AddressScrapper {
    async fn run(&mut self) -> Result<()> {
        if self.addresses.is_empty() {
            debug!("No addresses configured for monitoring, skipping");
            return Ok(());
        }

        info!("Updating balances for {} addresses", self.addresses.len());

        for address in &self.addresses {
            if let Err(e) = self.update_address_balance(address).await {
                debug!("Failed to update balance for {}: {}", address, e);
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Address Scrapper"
    }
}
