use crate::blockchains::tendermint::metrics::TENDERMINT_ADDRESS_BALANCE;
use crate::core::chain_id::ChainId;
use crate::core::clients::blockchain_client::BlockchainClient;
use crate::core::clients::path::Path;
use crate::core::exporter::Task;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::env;
use std::sync::Arc;
use tracing::{info, warn};

pub struct TendermintAddressScrapper {
    client: Arc<BlockchainClient>,
    addresses: Vec<String>,
    chain_id: ChainId,
    network: String,
}

impl TendermintAddressScrapper {
    pub fn new(client: Arc<BlockchainClient>, chain_id: ChainId, network: String) -> Self {
        // Parse addresses during initialization
        let addresses = env::var("ADDRESS_MONITORS")
            .unwrap_or_default()
            .split(';')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        Self {
            client,
            addresses,
            chain_id,
            network,
        }
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
            .context(format!("Failed to fetch balance for address: {}", address))?;

        let response: Value = serde_json::from_str(&response).context(format!(
            "Failed to parse balance response for address: {}",
            address
        ))?;

        let balances = response["balances"].as_array().context(format!(
            "Balances field not found or not an array for address: {}",
            address
        ))?;

        for balance in balances {
            let amount = balance["amount"]
                .as_str()
                .context(format!(
                    "Amount field not found or not a string for address: {}",
                    address
                ))?
                .parse::<f64>()
                .context(format!("Failed to parse amount for address: {}", address))?;

            let denom = balance["denom"].as_str().context(format!(
                "Denom field not found or not a string for address: {}",
                address
            ))?;

            TENDERMINT_ADDRESS_BALANCE
                .with_label_values(&[address, denom, &self.chain_id.to_string(), &self.network])
                .set(amount);
        }

        Ok(())
    }
}

#[async_trait]
impl Task for TendermintAddressScrapper {
    async fn run(&mut self) -> Result<()> {
        if self.addresses.is_empty() {
            warn!("No addresses configured for monitoring in ADDRESS_MONITORS environment variable, skipping");
            return Ok(());
        }

        info!("Updating balances for {} addresses", self.addresses.len());

        for address in &self.addresses {
            self.update_address_balance(address).await?;
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Tendermint Address Scrapper"
    }
}
