extern crate dotenv;

use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    discord_token: String,
    pub rpc_endpoints: String,
    pub indexer_endpoints: String,
    pub postgres_host: String,
    pub postgres_db: String,
    pub postgres_username: String,
    postgres_password: String,
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
        let indexer_endpoints = env::var("INDEXER_ENDPOINTS")
        .map_err(|err| ConfigError::EnvVarError(format!("INDEXER_ENDPOINTS: {}", err)))?;

        // Postgres config
        let postgres_host = env::var("POSTGRES_HOST")
        .map_err(|err| ConfigError::EnvVarError(format!("POSTGRES_HOST: {}", err)))?;
        let postgres_db = env::var("POSTGRES_DB")
        .map_err(|err| ConfigError::EnvVarError(format!("POSTGRES_DB: {}", err)))?;
        let postgres_username = env::var("POSTGRES_USERNAME")
        .map_err(|err| ConfigError::EnvVarError(format!("POSTGRES_USERNAME: {}", err)))?;
        let postgres_password = env::var("POSTGRES_PASSWORD")
        .map_err(|err| ConfigError::EnvVarError(format!("POSTGRES_PASSWORD: {}", err)))?;

        Ok(Settings {
            discord_token,
            rpc_endpoints,
            indexer_endpoints,
            postgres_host,
            postgres_db,
            postgres_username,
            postgres_password
        })
    }
    #[allow(dead_code)]
    pub(crate) fn discord_token(&self) -> &str {
        &self.discord_token
    }
}