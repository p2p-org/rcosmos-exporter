use std::env;

use anyhow::Context;
use async_trait::async_trait;
use tracing::info;

use crate::blockchains::tendermint::types::NodeInfoResponse;
use crate::core::clients::path::Path;
use crate::{
    blockchains::tendermint::metrics::{
        TENDERMINT_NODE_APP_COMMIT, TENDERMINT_NODE_APP_NAME, TENDERMINT_NODE_APP_VERSION,
        TENDERMINT_NODE_COSMOS_SDK_VERSION, TENDERMINT_NODE_MONIKER,
    },
    core::{app_context::AppContext, exporter::RunnableModule},
};

pub struct NodeInfo {
    app_context: std::sync::Arc<AppContext>,
    name: String,
    app_name: Option<String>,
    app_version: Option<String>,
    app_commit: Option<String>,
    cosmos_sdk_version: Option<String>,
    node_moniker: Option<String>,
}

impl NodeInfo {
    pub fn new(app_context: std::sync::Arc<AppContext>, name: String) -> Self {
        Self {
            app_context,
            name,
            app_name: None,
            app_version: None,
            app_commit: None,
            cosmos_sdk_version: None,
            node_moniker: None,
        }
    }

    async fn get_node_info(&self) -> anyhow::Result<NodeInfoResponse> {
        let client = self.app_context.lcd.as_ref().unwrap();
        let response = client
            .get(Path::from("/cosmos/base/tendermint/v1beta1/node_info"))
            .await
            .context("Could not fetch node info from node api")?;
        let node_info: NodeInfoResponse =
            serde_json::from_str(&response).context("Could not deserialize node info response")?;
        Ok(node_info)
    }

    async fn process_node_info(&mut self, node_info: &NodeInfoResponse) -> anyhow::Result<()> {
        info!("(Tendermint Node Info) Processing node info");
        let chain_id = &self.app_context.chain_id;
        let name = &self.name;
        let network = &self.app_context.config.general.network;
        let id = &self.app_context.config.node.id;
        // Helper macro to DRY the code
        macro_rules! update_metric {
            ($field:ident, $value:expr, $id:expr, $metric:ident) => {{
                let new_value = $value.clone();
                if self.$field.as_ref() != Some(&new_value) {
                    if let Some(ref old_value) = self.$field {
                        // Remove old label
                        let _ =
                            $metric.remove_label_values(&[name, chain_id, network, $id, old_value]);
                    }
                    // Set new value
                    $metric
                        .with_label_values(&[name, chain_id, network, $id, &new_value])
                        .set(1.0);
                    // Update stored field
                    self.$field = Some(new_value);
                }
            }};
        }
        update_metric!(
            app_name,
            node_info.application_version.app_name,
            id,
            TENDERMINT_NODE_APP_NAME
        );
        update_metric!(
            app_version,
            node_info.application_version.version,
            id,
            TENDERMINT_NODE_APP_VERSION
        );
        update_metric!(
            app_commit,
            node_info.application_version.git_commit,
            id,
            TENDERMINT_NODE_APP_COMMIT
        );
        update_metric!(
            cosmos_sdk_version,
            node_info.application_version.cosmos_sdk_version,
            id,
            TENDERMINT_NODE_COSMOS_SDK_VERSION
        );
        update_metric!(
            node_moniker,
            node_info.default_node_info.moniker,
            id,
            TENDERMINT_NODE_MONIKER
        );
        Ok(())
    }
}

pub fn factory(app_context: std::sync::Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.rpc.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    let name =
        env::var("NODE_NAME").unwrap_or_else(|_| panic!("NODE_NAME env variable should be set"));
    Ok(Box::new(NodeInfo::new(app_context, name)))
}

#[async_trait]
impl RunnableModule for NodeInfo {
    async fn run(&mut self) -> anyhow::Result<()> {
        let node_info = self
            .get_node_info()
            .await
            .context("Could not obtain node info")?;
        self.process_node_info(&node_info)
            .await
            .context("Failed to process node info")
    }
    fn name(&self) -> &'static str {
        "Tendermint Node Info"
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.app_context.config.node.tendermint.node_info.interval)
    }
}
