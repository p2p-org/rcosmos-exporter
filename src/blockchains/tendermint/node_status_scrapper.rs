use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use tracing::info;

use crate::{
    blockchains::tendermint::metrics::{
        TENDERMINT_NODE_CATCHING_UP, TENDERMINT_NODE_EARLIEST_BLOCK_HEIGHT,
        TENDERMINT_NODE_EARLIEST_BLOCK_TIME, TENDERMINT_NODE_ID,
        TENDERMINT_NODE_LATEST_BLOCK_HEIGHT, TENDERMINT_NODE_LATEST_BLOCK_TIME,
    },
    core::exporter::Task,
};

use super::types::TendermintStatusResponse;

pub struct TendermintNodeStatusScrapper {
    client: Client,
    endpoint: String,
    name: String,
    network: String,
}

impl TendermintNodeStatusScrapper {
    pub fn new(name: String, endpoint: String, network: String) -> Self {
        Self {
            client: Client::new(),
            name,
            endpoint,
            network,
        }
    }

    async fn get_status(&self) -> anyhow::Result<TendermintStatusResponse> {
        let response = self
            .client
            .get(format!("{}/status", self.endpoint))
            .send()
            .await
            .context("Could not fetch status from node")?;

        let status: TendermintStatusResponse = response
            .json()
            .await
            .context("Could not deserialize status response")?;
        Ok(status)
    }

    async fn process_status(&self) -> anyhow::Result<()> {
        info!("(Tendermint Node Status) Obtaining node status");

        let status = self
            .get_status()
            .await
            .context("Could not obtain node status")?;

        let chain_id = &status.result.node_info.network;

        TENDERMINT_NODE_ID
            .with_label_values(&[
                &self.name,
                &chain_id,
                &status.result.node_info.id,
                &self.network,
            ])
            .set(0);

        TENDERMINT_NODE_CATCHING_UP
            .with_label_values(&[&self.name, &chain_id, &self.network])
            .set(if status.result.sync_info.catching_up {
                1
            } else {
                0
            });
        TENDERMINT_NODE_LATEST_BLOCK_HEIGHT
            .with_label_values(&[&self.name, &chain_id, &self.network])
            .set(
                status
                    .result
                    .sync_info
                    .latest_block_height
                    .parse::<i64>()
                    .context("Could not parse latest block height")?,
            );
        TENDERMINT_NODE_LATEST_BLOCK_TIME
            .with_label_values(&[&self.name, &chain_id, &self.network])
            .set(
                status
                    .result
                    .sync_info
                    .latest_block_time
                    .and_utc()
                    .timestamp() as f64,
            );
        TENDERMINT_NODE_EARLIEST_BLOCK_HEIGHT
            .with_label_values(&[&self.name, &chain_id, &self.network])
            .set(
                status
                    .result
                    .sync_info
                    .earliest_block_height
                    .parse::<i64>()
                    .context("Could not parse earliest block height")?,
            );
        TENDERMINT_NODE_EARLIEST_BLOCK_TIME
            .with_label_values(&[&self.name, &chain_id, &self.network])
            .set(
                status
                    .result
                    .sync_info
                    .earliest_block_time
                    .and_utc()
                    .timestamp() as f64,
            );
        Ok(())
    }
}

#[async_trait]
impl Task for TendermintNodeStatusScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_status()
            .await
            .context("Failed to process node status")
    }

    fn name(&self) -> &'static str {
        "Tendermint Node Status Scrapper"
    }
}
