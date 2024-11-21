extern crate dotenv;

mod config;

use std::process;

use rcosmos_exporter::MessageLog;
use crate::config::Settings;
use rcosmos_exporter::tendermint::{
    rpc::initialize_rpc_client,
    rest::initialize_rest_client,
    watcher::{
        initialize_watcher_client,
        spawn_watcher
    },
    metrics::{
        register_custom_metrics,
        serve_metrics
    }
};

#[tokio::main]
async fn main() {
    let mut rest_initialized = false;
    let mut rpc_initialized = false;
    let config = match config::Settings::new() {
        Ok(settings) => settings,
        Err(err) => {
            MessageLog!("Failed to load configuration: {}", err);
            process::exit(1);
        }
    };

    if !config.rest_endpoints.is_empty() {
        match initialize_rest_client().await {
            Ok(_) => {
                rest_initialized = true;
            }
            Err(err) => MessageLog!("ERROR", "Failed to initialize REST client: {:?}", err),
        }
    } else {
        MessageLog!("INFO", "Skipping REST initialization: Missing or invalid `rest_endpoints` in config");
    }

    if !config.rpc_endpoints.is_empty() {
        match initialize_rpc_client().await {
            Ok(_) => {
                rpc_initialized = true;
            }
            Err(err) => MessageLog!("ERROR", "Failed to initialize RPC client: {:?}", err),
        }
    } else {
        MessageLog!("INFO", "Skipping RPC initialization: Missing or invalid `rpc_endpoints` in config");
    }

    if !rest_initialized && !rpc_initialized {
        MessageLog!("ERROR", "Failed to initialize both REST and RPC clients. Exiting...");
        process::exit(1);
    }

    let watcher_client = initialize_watcher_client()
        .await
        .expect("Failed to initialize watcher client");
    register_custom_metrics();
    spawn_watcher(watcher_client.clone());
    serve_metrics().await;
}
