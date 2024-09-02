extern crate dotenv;

mod config;

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
    initialize_rpc_client().await;
    initialize_rest_client().await;
    let watcher_client = initialize_watcher_client()
        .await
        .expect("Failed to initialize watcher client");
    register_custom_metrics();
    spawn_watcher(watcher_client.clone());
    serve_metrics().await;
}
