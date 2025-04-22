use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use tokio::time::sleep;
use tracing::{error, info};

use crate::{
    blockchains::tendermint::metrics::{
        TENDERMINT_NODE_CATCHING_UP, TENDERMINT_NODE_EARLIEST_BLOCK_HASH,
        TENDERMINT_NODE_EARLIEST_BLOCK_HEIGHT, TENDERMINT_NODE_EARLIEST_BLOCK_TIME,
        TENDERMINT_NODE_ID, TENDERMINT_NODE_LATEST_BLOCK_HASH, TENDERMINT_NODE_LATEST_BLOCK_HEIGHT,
        TENDERMINT_NODE_LATEST_BLOCK_TIME,
    },
    core::{exporter::Task, network::Network},
};

use super::types::TendermintStatusResponse;

pub struct TendermintNodeStatusScrapper {
    client: Client,
    endpoint: String,
    name: String,
    network: Network,
}

impl TendermintNodeStatusScrapper {
    pub fn new(name: String, endpoint: String, network: Network) -> Self {
        Self {
            client: Client::new(),
            name,
            endpoint,
            network,
        }
    }

    async fn get_status(&self) -> anyhow::Result<TendermintStatusResponse> {
        let response = match self
            .client
            .get(format!("{}/status", self.endpoint))
            .send()
            .await
        {
            Ok(res) => res,
            Err(e) => return Err(e.into()),
        };

        let status: TendermintStatusResponse = response.json().await?;
        Ok(status)
    }

    async fn process_status(&self) {
        info!("(Tendermint Node Status) Obtaining node status");

        let status = match self.get_status().await {
            Ok(status) => status,
            Err(e) => {
                error!("(Tendermint Node Status) Could not obtain status");
                error!("(Tendermint Node Status) Error: {}", e);
                return;
            }
        };

        let chain_id = &status.result.node_info.network;

        TENDERMINT_NODE_ID
            .with_label_values(&[
                &self.name,
                &chain_id,
                &status.result.node_info.id,
                &self.network.to_string(),
            ])
            .set(0);

        TENDERMINT_NODE_CATCHING_UP
            .with_label_values(&[&self.name, &chain_id, &self.network.to_string()])
            .set(if status.result.sync_info.catching_up {
                1
            } else {
                0
            });
        TENDERMINT_NODE_LATEST_BLOCK_HASH
            .with_label_values(&[
                &self.name,
                &chain_id,
                &status.result.sync_info.latest_block_hash,
                &self.network.to_string(),
            ])
            .set(0);
        TENDERMINT_NODE_LATEST_BLOCK_HEIGHT
            .with_label_values(&[&self.name, &chain_id, &self.network.to_string()])
            .set(
                status
                    .result
                    .sync_info
                    .latest_block_height
                    .parse::<i64>()
                    .expect("Could not parse latest block height"),
            );
        TENDERMINT_NODE_LATEST_BLOCK_TIME
            .with_label_values(&[&self.name, &chain_id, &self.network.to_string()])
            .set(
                status
                    .result
                    .sync_info
                    .latest_block_time
                    .and_utc()
                    .timestamp() as f64,
            );

        TENDERMINT_NODE_EARLIEST_BLOCK_HASH
            .with_label_values(&[
                &self.name,
                &chain_id,
                &status.result.sync_info.earliest_block_hash,
                &self.network.to_string(),
            ])
            .set(0);
        TENDERMINT_NODE_EARLIEST_BLOCK_HEIGHT
            .with_label_values(&[&self.name, &chain_id, &self.network.to_string()])
            .set(
                status
                    .result
                    .sync_info
                    .earliest_block_height
                    .parse::<i64>()
                    .expect("Could not parse earliest block height"),
            );
        TENDERMINT_NODE_EARLIEST_BLOCK_TIME
            .with_label_values(&[&self.name, &chain_id, &self.network.to_string()])
            .set(
                status
                    .result
                    .sync_info
                    .earliest_block_time
                    .and_utc()
                    .timestamp() as f64,
            );
    }
}

#[async_trait]
impl Task for TendermintNodeStatusScrapper {
    async fn run(&mut self, delay: Duration) {
        info!("(Running Tendermint Node Status Scrapper");

        loop {
            self.process_status().await;

            sleep(delay).await
        }
    }
}
