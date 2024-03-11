extern crate dotenv;

use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    discord_token: String,
    pub rpc_endpoints: String,
}

#[derive(Debug)]
pub enum ConfigError {
    EnvVarError(String),
}

impl Settings {
    pub(crate) fn new() -> Result<Self, ConfigError> {
        dotenv::dotenv().ok();
        // Discord config
        let discord_token = env::var("DISCORD_TOKEN")
            .map_err(|err| ConfigError::EnvVarError(format!("DISCORD_TOKEN: {}", err)))?;

        // Tendermint config
        let rpc_endpoints = env::var("RPC_ENDPOINTS")
            .map_err(|err| ConfigError::EnvVarError(format!("RPC_ENDPOINTS: {}", err)))?;

        Ok(Settings {
            discord_token,
            rpc_endpoints,
        })
    }
    #[allow(dead_code)]
    pub(crate) fn discord_token(&self) -> &str {
        &self.discord_token
    }
}