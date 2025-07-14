use crate::blockchains::tendermint::{
    metrics::TENDERMINT_ADDRESS_BALANCE, types::BankBalancesResponse,
};
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;

pub struct Bank {
    app_context: Arc<AppContext>,
}

impl Bank {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn update_address_balance(&self, address: &str) -> Result<()> {
        info!("(Tendermint Bank) Getting {} balance", address);
        let client = self.app_context.lcd.as_ref().unwrap();
        let network = &self.app_context.config.general.network;
        let chain_id = &self.app_context.chain_id;
        let response = client
            .get(Path::from(format!(
                "/cosmos/bank/v1beta1/balances/{}",
                address
            )))
            .await
            .context(format!("Failed to fetch balance for address: {}", address))?;

        let response: BankBalancesResponse = serde_json::from_str(&response).context(format!(
            "Failed to parse balance response for address: {}",
            address
        ))?;

        for balance in response.balances {
            let amount = balance
                .amount
                .parse::<f64>()
                .context(format!("Failed to parse amount for address: {}", address))?;
            let denom = &balance.denom;
            TENDERMINT_ADDRESS_BALANCE
                .with_label_values(&[address, denom, chain_id, network])
                .set(amount);
        }
        Ok(())
    }

    async fn get_bank_balances(&self) -> Result<()> {
        let addresses = self
            .app_context
            .config
            .network
            .tendermint
            .bank
            .addresses
            .clone();

        for address in &addresses {
            self.update_address_balance(address).await?;
        }
        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.lcd.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    if app_context
        .config
        .network
        .tendermint
        .bank
        .addresses
        .is_empty()
    {
        anyhow::bail!("No addresses configured for bank balance monitoring but module is enabled");
    }
    Ok(Box::new(Bank::new(app_context)))
}

#[async_trait]
impl RunnableModule for Bank {
    async fn run(&mut self) -> Result<()> {
        self.get_bank_balances()
            .await
            .context("Could not get bank balances")
    }

    fn name(&self) -> &'static str {
        "Tendermint Bank Balance"
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.tendermint.bank.interval as u64,
        )
    }
}
