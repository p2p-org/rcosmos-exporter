use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use tracing::info;

use crate::{
    blockchains::tendermint::metrics::{
        TENDERMINT_NODE_APP_COMMIT, TENDERMINT_NODE_APP_NAME, TENDERMINT_NODE_APP_VERSION,
        TENDERMINT_NODE_CATCHING_UP, TENDERMINT_NODE_COSMOS_SDK_VERSION,
        TENDERMINT_NODE_EARLIEST_BLOCK_HEIGHT, TENDERMINT_NODE_EARLIEST_BLOCK_TIME,
        TENDERMINT_NODE_ID, TENDERMINT_NODE_LATEST_BLOCK_HEIGHT, TENDERMINT_NODE_LATEST_BLOCK_TIME,
        TENDERMINT_NODE_MONIKER,
    },
    core::exporter::Task,
};

use super::types::{TendermintNodeInfoResponse, TendermintStatusResponse};

pub struct TendermintNodeStatusScrapper {
    client: Client,
    rpc_endpoint: String,
    rest_endpoint: String,
    name: String,
    network: String,

    app_name: Option<String>,
    app_version: Option<String>,
    app_commit: Option<String>,
    cosmos_sdk_version: Option<String>,
    node_moniker: Option<String>,
}

impl TendermintNodeStatusScrapper {
    pub fn new(name: String, rpc_endpoint: String, rest_endpoint: String, network: String) -> Self {
        Self {
            client: Client::new(),
            name,
            rpc_endpoint,
            rest_endpoint,
            network,
            app_name: None,
            app_version: None,
            app_commit: None,
            cosmos_sdk_version: None,
            node_moniker: None,
        }
    }

    async fn get_status(&self) -> anyhow::Result<TendermintStatusResponse> {
        let response = self
            .client
            .get(format!("{}/status", self.rpc_endpoint))
            .send()
            .await
            .context("Could not fetch status from node")?;

        let status: TendermintStatusResponse = response
            .json()
            .await
            .context("Could not deserialize status response")?;
        Ok(status)
    }

    async fn get_node_info(&self) -> anyhow::Result<TendermintNodeInfoResponse> {
        let response = self
            .client
            .get(format!(
                "{}/cosmos/base/tendermint/v1beta1/node_info",
                self.rest_endpoint
            ))
            .send()
            .await
            .context("Could not fetch node info from node api")?;

        let node_info: TendermintNodeInfoResponse = response
            .json()
            .await
            .context("Could not deserialize node info response")?;
        Ok(node_info)
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

    async fn process_node_info(&mut self) -> anyhow::Result<()> {
        info!("(Tendermint Node Status) Obtaining node info");

        let node_info = self
            .get_node_info()
            .await
            .context("Could not obtain node info")?;

        let chain_id = &node_info.default_node_info.network;

        // Helper macro to DRY the code
        macro_rules! update_metric {
            ($field:ident, $value:expr, $metric:ident) => {{
                let new_value = $value.clone();
                if self.$field.as_ref() != Some(&new_value) {
                    if let Some(ref old_value) = self.$field {
                        // Remove old label
                        let _ = $metric.remove_label_values(&[
                            &self.name,
                            chain_id,
                            &self.network,
                            old_value,
                        ]);
                    }

                    // Set new value
                    $metric
                        .with_label_values(&[&self.name, chain_id, &self.network, &new_value])
                        .set(1.0);

                    // Update stored field
                    self.$field = Some(new_value);
                }
            }};
        }

        // Now apply it to each metric
        update_metric!(
            app_name,
            node_info.application_version.name,
            TENDERMINT_NODE_APP_NAME
        );
        update_metric!(
            app_version,
            node_info.application_version.version,
            TENDERMINT_NODE_APP_VERSION
        );
        update_metric!(
            app_commit,
            node_info.application_version.git_commit,
            TENDERMINT_NODE_APP_COMMIT
        );
        update_metric!(
            cosmos_sdk_version,
            node_info.application_version.cosmos_sdk_version,
            TENDERMINT_NODE_COSMOS_SDK_VERSION
        );
        update_metric!(
            node_moniker,
            node_info.default_node_info.moniker,
            TENDERMINT_NODE_MONIKER
        );

        Ok(())
    }
}

#[async_trait]
impl Task for TendermintNodeStatusScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_status()
            .await
            .context("Failed to process node status")?;
        self.process_node_info()
            .await
            .context("Failed to process node info")
    }

    fn name(&self) -> &'static str {
        "Tendermint Node Status Scrapper"
    }
}
