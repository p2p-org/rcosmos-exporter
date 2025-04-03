use std::sync::Arc;

use crate::{
    blockchains::tendermint::types::TendermintStatusResponse,
    core::{
        chain_id::{ChainId, ChainIdFetcher, ChainIdFetcherErrors},
        clients::blockchain_client::BlockchainClient,
    },
};

pub struct TendermintChainIdFetcher {
    client: Arc<BlockchainClient>,
}

impl TendermintChainIdFetcher {
    pub fn new(client: Arc<BlockchainClient>) -> Self {
        Self { client }
    }
}

impl ChainIdFetcher for TendermintChainIdFetcher {
    async fn get_chain_id(&self) -> Result<ChainId, ChainIdFetcherErrors> {
        let res = match self.client.with_rpc().get("/status").await {
            Ok(res) => res,
            Err(e) => return Err(ChainIdFetcherErrors::HttpClientError(e)),
        };

        match serde_json::from_str::<TendermintStatusResponse>(&res) {
            Ok(res) => Ok(ChainId::new(res.result.node_info.network)),
            Err(e) => Err(ChainIdFetcherErrors::DeserializeError(e)),
        }
    }
}
