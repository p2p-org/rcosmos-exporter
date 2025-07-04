use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use tracing::{error, info};

use crate::{
    blockchains::namada::metrics::{
        TENDERMINT_NODE_CATCHING_UP, TENDERMINT_NODE_LATEST_BLOCK_HEIGHT,
        TENDERMINT_NODE_LATEST_BLOCK_TIME,
    },
    core::exporter::Task,
};

pub struct NamadaNodeStatusScrapper {
    client: Client,
    rpc_endpoint: String,
    name: String,
    network: String,
}

impl NamadaNodeStatusScrapper {
    pub fn new(
        name: String,
        rpc_endpoint: String,
        _rest_endpoint: String,
        network: String,
    ) -> Self {
        Self {
            client: Client::new(),
            name,
            rpc_endpoint,
            network,
        }
    }

    async fn get_node_status(&self) -> anyhow::Result<NamadaNodeStatus> {
        // For Namada, we'll use the /block endpoint to get latest block info
        // since Namada doesn't have a /status endpoint like Tendermint
        let response = self
            .client
            .get(format!("{}/block", self.rpc_endpoint))
            .send()
            .await
            .context("Could not fetch latest block")?;

        let block_response: crate::blockchains::namada::types::BlockResponse = response
            .json()
            .await
            .context("Could not deserialize block response")?;

        // Extract status info from the block response
        let block = block_response.result.block;
        let height = block
            .header
            .height
            .parse::<u64>()
            .context("Could not parse block height")?;
        let time = block.header.time.clone();
        let chain_id = block.header.chain_id.clone();

        Ok(NamadaNodeStatus {
            height,
            time,
            chain_id,
            catching_up: false, // We'll assume it's not catching up if we can get blocks
        })
    }
}

#[derive(Debug)]
struct NamadaNodeStatus {
    height: u64,
    time: String,
    chain_id: String,
    catching_up: bool,
}

#[async_trait]
impl Task for NamadaNodeStatusScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        match self.get_node_status().await {
            Ok(status) => {
                // Set metrics
                TENDERMINT_NODE_LATEST_BLOCK_HEIGHT
                    .with_label_values(&[&self.name, &status.chain_id, &self.network])
                    .set(status.height as i64);

                // Parse time string to timestamp
                if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(&status.time) {
                    TENDERMINT_NODE_LATEST_BLOCK_TIME
                        .with_label_values(&[&self.name, &status.chain_id, &self.network])
                        .set(timestamp.timestamp() as f64);
                }

                TENDERMINT_NODE_CATCHING_UP
                    .with_label_values(&[&self.name, &status.chain_id, &self.network])
                    .set(if status.catching_up { 1 } else { 0 });

                info!(
                    "(Namada Node Status) Node: {}, Network: {}, Height: {}, Time: {}, Catching up: {}",
                    self.name, self.network, status.height, status.time, status.catching_up
                );
            }
            Err(e) => {
                error!("(Namada Node Status) Error: {}", e);
                return Err(e);
            }
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Namada Node Status Scrapper"
    }
}
