use crate::blockchains::cometbft::types::StatusResponse;
use crate::core::clients::http_client::NodePool;
use crate::core::clients::path::Path;
use anyhow::{Context, Result};

pub async fn fetch_chain_id(rpc: &NodePool) -> Result<String> {
    let response = rpc
        .get(Path::from("/status"))
        .await
        .context("Could not fetch status from node")?;
    let status: StatusResponse = serde_json::from_str(&response).context(
        "Could not deserialize status response while fetching chain_id for automated setup",
    )?;
    Ok(status.result.node_info.network)
}
