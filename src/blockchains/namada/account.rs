use crate::blockchains::tendermint::metrics::TENDERMINT_ADDRESS_BALANCE;
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;
use anyhow::Context;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub struct Account {
    pub app_context: Arc<AppContext>,
}

impl Account {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn process_accounts(&self) -> anyhow::Result<()> {
        let client = self.app_context.lcd.as_ref().unwrap();
        for address in self
            .app_context
            .config
            .network
            .namada
            .account
            .addresses
            .clone()
        {
            let response = client
                .get(Path::from(format!("/api/v1/account/{}", address)))
                .await
                .context(format!("Failed to fetch balance for address: {}", address))?;

            let response: Value = serde_json::from_str(&response).context(format!(
                "Failed to parse balance response for address: {}",
                address
            ))?;

            let balances = response.as_array().context(format!(
                "Account response not an array for address: {}",
                address
            ))?;

            for balance in balances {
                let amount = balance["minDenomAmount"]
                    .as_str()
                    .context(format!(
                        "minDenomAmount field not found or not a string for address: {}",
                        address
                    ))?
                    .parse::<f64>()
                    .context(format!("Failed to parse amount for address: {}", address))?;

                let denom = balance["tokenAddress"].as_str().context(format!(
                    "tokenAddress field not found or not a string for address: {}",
                    address
                ))?;

                TENDERMINT_ADDRESS_BALANCE
                    .with_label_values(&[
                        &address,
                        denom,
                        &self.app_context.chain_id.to_string(),
                        &self.app_context.config.general.network,
                    ])
                    .set(amount);
            }
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
        .namada
        .account
        .addresses
        .is_empty()
    {
        anyhow::bail!(
            "No addresses configured for account balance monitoring but module is enabled"
        );
    }
    Ok(Box::new(Account::new(app_context)))
}

#[async_trait]
impl RunnableModule for Account {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_accounts()
            .await
            .context("Failed to process accounts")
    }
    fn name(&self) -> &'static str {
        "Namada Account"
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.app_context.config.network.namada.account.interval)
    }
}
