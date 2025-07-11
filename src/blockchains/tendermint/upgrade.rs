use std::sync::Arc;

use crate::core::clients::path::Path;
use async_trait::async_trait;
use serde_json::from_str;
use tracing::info;

use crate::core::app_context::AppContext;
use crate::{
    blockchains::tendermint::{metrics::TENDERMINT_UPGRADE_PLAN, types::UpgradePlanResponse},
    core::exporter::RunnableModule,
};
use anyhow::Context;

pub struct Upgrade {
    app_context: Arc<AppContext>,
}

impl Upgrade {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn get_upgrade_plan(&self) -> anyhow::Result<UpgradePlanResponse> {
        info!("(Tendermint Upgrade) Fetching upgrade plan");

        let client = self.app_context.lcd.as_ref().unwrap();

        let res = client
            .get(Path::from("/cosmos/upgrade/v1beta1/current_plan"))
            .await
            .map_err(|e| anyhow::anyhow!(format!("NodePool error: {e}")))?;

        from_str::<UpgradePlanResponse>(&res).context("Could not deserialize upgrade plan response")
    }

    async fn process_upgrade_plan(&mut self) -> anyhow::Result<()> {
        info!("(Tendermint Upgrade) Searching for upgrade plan");

        let plan_response = self
            .get_upgrade_plan()
            .await
            .context("Could not obtain upgrade plan")?;

        if let Some(plan) = plan_response.plan {
            info!("(Tendermint Upgrade) Found upgrade plan");
            let height = plan
                .height
                .parse::<i64>()
                .context("Could not parse plan height")?;
            TENDERMINT_UPGRADE_PLAN
                .with_label_values(&[&plan.name, &self.app_context.config.general.network])
                .set(height);
        }

        Ok(())
    }
}

#[async_trait]
impl RunnableModule for Upgrade {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_upgrade_plan()
            .await
            .context("Failed to process upgrade plan")
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.tendermint.upgrade.interval as u64,
        )
    }

    fn name(&self) -> &'static str {
        "Tendermint Upgrade"
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.lcd.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    Ok(Box::new(Upgrade::new(app_context)))
}
