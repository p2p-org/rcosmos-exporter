use std::ops::Deref;

use thiserror::Error;

use super::clients::http_client::HTTPClientErrors;

#[derive(Clone)]
pub struct ChainId(pub String); 

impl ChainId {
    pub fn new(chain_id: String) -> Self {
        Self(chain_id)
    }
}

impl Deref for ChainId {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Error)]
pub enum ChainIdFetcherErrors {
    #[error("Error at the http call: {0}")]
    HttpClientError(#[from] HTTPClientErrors),

    #[error("Error deserializing response: {0}")]
    DeserializeError(#[from] serde_json::Error),
}

pub trait ChainIdFetcher {
    async fn get_chain_id(&self) -> Result<ChainId, ChainIdFetcherErrors>;
}
