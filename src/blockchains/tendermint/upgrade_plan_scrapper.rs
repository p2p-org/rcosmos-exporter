use std::sync::Arc;

use crate::core::clients::path::Path;
use async_trait::async_trait;
use serde_json::from_str;
use tracing::info;

use crate::{
    blockchains::tendermint::{
        metrics::TENDERMINT_UPGRADE_PLAN, types::TendermintUpgradePlanResponse,
    },
    core::{chain_id::ChainId, clients::blockchain_client::BlockchainClient, exporter::Task},
};
use anyhow::Context;

pub struct TendermintUpgradePlanScrapper {
    client: Arc<BlockchainClient>,
    chain_id: ChainId,
    network: String,
}

impl TendermintUpgradePlanScrapper {
    pub fn new(client: Arc<BlockchainClient>, chain_id: ChainId, network: String) -> Self {
        Self {
            client,
            chain_id,
            network,
        }
    }

    async fn get_upgrade_plan(&self) -> anyhow::Result<TendermintUpgradePlanResponse> {
        info!("(Tendermint Upgrade Plan Scrapper) Fetching upgrade plan");

        let res = self
            .client
            .with_rest()
            .get(Path::from("/cosmos/upgrade/v1beta1/current_plan"))
            .await
            .context("Could not fetch upgrade plan")?;

        from_str::<TendermintUpgradePlanResponse>(&res)
            .context("Could not deserialize upgrade plan response")
    }

    async fn process_upgrade_plan(&mut self) -> anyhow::Result<()> {
        info!("(Tendermint Upgrade Plan Scrapper) Searching for upgrade plan");

        let plan_response = self
            .get_upgrade_plan()
            .await
            .context("Could not obtain upgrade plan")?;

        if let Some(plan) = plan_response.plan {
            info!("(Tendermint Upgrade Plan Scrapper) Found upgrade plan");
            let height = plan
                .height
                .parse::<i64>()
                .context("Could not parse plan height")?;
            TENDERMINT_UPGRADE_PLAN
                .with_label_values(&[&plan.name, &self.chain_id.to_string(), &self.network])
                .set(height);
        }

        Ok(())
    }
}

#[async_trait]
impl Task for TendermintUpgradePlanScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_upgrade_plan()
            .await
            .context("Failed to process upgrade plan")
    }

    fn name(&self) -> &'static str {
        "Tendermint Upgrade Plan Scrapper"
    }
}
