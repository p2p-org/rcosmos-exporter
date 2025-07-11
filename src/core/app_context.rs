use crate::core::clients::http_client::NodePool;
use crate::core::config::AppConfig;
use std::sync::Arc;

pub struct AppContext {
    pub config: AppConfig,
    pub rpc: Option<Arc<NodePool>>,
    pub lcd: Option<Arc<NodePool>>,
    pub chain_id: String,
}

impl AppContext {
    pub fn new(
        config: AppConfig,
        rpc: Option<Arc<NodePool>>,
        lcd: Option<Arc<NodePool>>,
        chain_id: String,
    ) -> Self {
        Self {
            config,
            rpc,
            lcd,
            chain_id: chain_id,
        }
    }
}
