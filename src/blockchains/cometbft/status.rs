use std::env;

use anyhow::Context;
use async_trait::async_trait;
use tracing::info;


use crate::{
    blockchains::{
        cometbft::metrics::{
            COMETBFT_NODE_CATCHING_UP, COMETBFT_NODE_EARLIEST_BLOCK_HEIGHT,
            COMETBFT_NODE_EARLIEST_BLOCK_TIME, COMETBFT_NODE_ID, COMETBFT_NODE_LATEST_BLOCK_HEIGHT,
            COMETBFT_NODE_LATEST_BLOCK_TIME,
        },
        cometbft::types::{StatusResponse, StatusResult},
    },
    core::{app_context::AppContext, clients::path::Path, exporter::RunnableModule},
};

// Use shared utility from core::utils instead of local implementation
use crate::core::utils::find_nested_value;

pub struct Status {
    app_context: std::sync::Arc<AppContext>,
    name: String,
}

impl Status {
    pub fn new(app_context: std::sync::Arc<AppContext>, name: String) -> Self {
        Self { app_context, name }
    }

    async fn get_status(&self) -> anyhow::Result<StatusResponse> {
        let client = self.app_context.rpc.as_ref().unwrap();
        let response = client
            .get(Path::from("/status"))
            .await
            .context("Could not fetch status from node")?;

        // Parse as JSON to dynamically find sync_info and node_info
        let json: serde_json::Value = serde_json::from_str(&response)
            .context("Could not parse status response as JSON")?;

        // Dynamically find all required fields regardless of nesting
        let sync_info = find_nested_value(&json, "sync_info")
            .context("Could not find sync_info in response")?;
        let node_info = find_nested_value(&json, "node_info")
            .context("Could not find node_info in response")?;
        let validator_info = find_nested_value(&json, "validator_info");

        // Create StatusResponse with dynamically found values
        let status_result = StatusResult {
            node_info: serde_json::from_value(node_info.clone())
                .context("Could not deserialize node_info")?,
            sync_info: serde_json::from_value(sync_info.clone())
                .context("Could not deserialize sync_info")?,
            validator_info: if let Some(vi) = validator_info {
                Some(serde_json::from_value(vi.clone())
                    .context("Could not deserialize validator_info")?)
            } else {
                None
            },
        };

        Ok(StatusResponse { result: status_result })
    }

    async fn process_status_metrics(&self, status: &StatusResponse) -> anyhow::Result<()> {
        info!("(CometBFT Node Status) Processing status metrics");
        let chain_id = &self.app_context.chain_id;
        let network = &self.app_context.config.general.network;
        let client = &self.app_context.config.node.client;
        COMETBFT_NODE_ID
            .with_label_values(&[
                &self.name,
                &chain_id,
                &status.result.node_info.id,
                &network,
                client,
            ])
            .set(0);
        COMETBFT_NODE_CATCHING_UP
            .with_label_values(&[&self.name, &chain_id, &network, client])
            .set(if status.result.sync_info.catching_up {
                1
            } else {
                0
            });
        COMETBFT_NODE_LATEST_BLOCK_HEIGHT
            .with_label_values(&[&self.name, &chain_id, &network, client])
            .set(
                status
                    .result
                    .sync_info
                    .latest_block_height
                    .parse::<i64>()
                    .context("Could not parse latest block height")?,
            );
        COMETBFT_NODE_LATEST_BLOCK_TIME
            .with_label_values(&[&self.name, &chain_id, &network, client])
            .set(
                status
                    .result
                    .sync_info
                    .latest_block_time
                    .and_utc()
                    .timestamp() as f64,
            );
        COMETBFT_NODE_EARLIEST_BLOCK_HEIGHT
            .with_label_values(&[&self.name, &chain_id, &network, client])
            .set(
                status
                    .result
                    .sync_info
                    .earliest_block_height
                    .parse::<i64>()
                    .context("Could not parse earliest block height")?,
            );
        COMETBFT_NODE_EARLIEST_BLOCK_TIME
            .with_label_values(&[&self.name, &chain_id, &network, client])
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

pub fn factory(app_context: std::sync::Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.rpc.is_none() {
        anyhow::bail!("Config is missing RPC node pool");
    }
    let name =
        env::var("NODE_NAME").unwrap_or_else(|_| panic!("NODE_NAME env variable should be set"));
    Ok(Box::new(Status::new(app_context, name)))
}

#[async_trait]
impl RunnableModule for Status {
    async fn run(&mut self) -> anyhow::Result<()> {
        let status = self
            .get_status()
            .await
            .context("Could not obtain node status")?;
        self.process_status_metrics(&status)
            .await
            .context("Failed to process node status")
    }
    fn name(&self) -> &'static str {
        "CometBFT Status"
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.app_context.config.node.cometbft.status.interval)
    }
}
