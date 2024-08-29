extern crate dotenv;

use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    pub prometheus_ip: String,
    pub prometheus_port: u16,
    pub rpc_endpoints: String,
    pub validator_address: String,
    pub block_window: u16,
}

#[derive(Debug)]
pub enum ConfigError {
    EnvVarError(String),
}

impl std::error::Error for ConfigError {}
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ConfigError::EnvVarError(err) => write!(f, "Environment variable error: {}", err),
        }
    }
}

impl Settings {
    #[allow(dead_code)]
    pub(crate) fn new() -> Result<Self, ConfigError> {
        dotenv::dotenv().ok();
        // Prometheus config
        let prometheus_ip = env::var("PROMETHEUS_IP")
            .unwrap_or_else(|_| "127.0.0.1".to_string());
        let prometheus_port = env::var("PROMETHEUS_PORT")
            .unwrap_or_else(|_| "9100".to_string())
            .parse::<u16>()
            .map_err(|err| ConfigError::EnvVarError(format!("Invalid format for PROMETHEUS_PORT: {}", err)))?;

        // Tendermint config
        let rpc_endpoints = env::var("RPC_ENDPOINTS")
            .map_err(|err| ConfigError::EnvVarError(format!("Missing or invalid RPC_ENDPOINTS: {}", err)))?;
        let validator_address = env::var("VALIDATOR_ADDRESS")
            .map_err(|err| ConfigError::EnvVarError(format!("Missing or invalid VALIDATOR_ADDRESS: {}", err)))?;
        let block_window = env::var("BLOCK_WINDOW")
            .unwrap_or_else(|_| "500".to_string())
            .parse::<u16>()
            .map_err(|err| ConfigError::EnvVarError(format!("Invalid format for BLOCK_WINDOW: {}", err)))?;

        Ok(Settings {
            prometheus_ip,
            prometheus_port,
            rpc_endpoints,
            validator_address,
            block_window,
        })
    }
}
