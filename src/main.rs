extern crate dotenv;

mod config;

use tokio::time::Duration;
use tokio::time::sleep;
use rcosmos_exporter::tendermint::rpc::initialize_rpc_client;
use rcosmos_exporter::tendermint::watcher::initialize_watcher_client;
use rcosmos_exporter::tendermint::metrics::{register_custom_metrics, serve_metrics};

#[tokio::main]
async fn main() {
    initialize_rpc_client().await;
    initialize_watcher_client().await.expect("Failed to initialize watcher client");
    register_custom_metrics();
    serve_metrics().await;
}
