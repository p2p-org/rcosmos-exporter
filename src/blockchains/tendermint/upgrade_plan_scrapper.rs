use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde_json::from_str;
use tokio::time::sleep;
use tracing::{error, info};

use crate::{
    blockchains::tendermint::{
        metrics::TENDERMINT_UPGRADE_PLAN, types::TendermintUpgradePlanResponse,
    },
    core::{chain_id::ChainId, clients::blockchain_client::BlockchainClient, exporter::Task},
};

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
            .get("/cosmos/upgrade/v1beta1/current_plan")
            .await?;

        match from_str::<TendermintUpgradePlanResponse>(&res) {
            Ok(res) => Ok(res),
            Err(e) => Err(e.into()),
        }
    }

    async fn process_upgrade_plan(&mut self) {
        info!("(Tendermint Upgrade Plan Scrapper) Searching for upgrade plan");

        match self.get_upgrade_plan().await {
            Ok(res) => {
                if let Some(plan) = res.plan {
                    info!("(Tendermint Upgrade Plan Scrapper) Found upgrade plan");
                    let height = match plan.height.parse::<i64>() {
                        Ok(h) => h,
                        Err(e) => {
                            error!("(Tendermint Upgrade Plan Scrapper) Could not parse upgrade plan height");
                            error!("{:?}", e);
                            return;
                        }
                    };
                    TENDERMINT_UPGRADE_PLAN
                        .with_label_values(&[&plan.name, &self.chain_id.to_string(), &self.network])
                        .set(height);
                }
            }
            Err(e) => {
                error!("(Tendermint Upgrade Plan Scrapper) Failed to obtain upgrade plan");
                error!("{:?}", e);
                return;
            }
        };
    }
}

#[async_trait]
impl Task for TendermintUpgradePlanScrapper {
    async fn run(&mut self, delay: Duration) {
        info!("Running Tendermint Plan Scrapper");

        loop {
            self.process_upgrade_plan().await;

            sleep(delay).await
        }
    }
}
