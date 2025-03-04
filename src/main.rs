use blockchains::tendermint::tendermint::Tendermint;
use core::{
    blockchain::Blockchain, blockchain_client::BlockchainClientBuilder,
    blockchains::BlockchainType, http_client::HttpClient, metrics::serve_metrics,
};
use dotenv::dotenv;
use std::{env, sync::Arc};
use tokio::sync::Mutex;
use tracing::info;

mod blockchains;
mod core;

#[tokio::main]
async fn main() {
    dotenv().ok();

    tracing_subscriber::fmt()
        .json()
        .with_target(false)
        .flatten_event(true)
        .init();

    let prometheus_ip = env::var("PROMETHEUS_IP").unwrap_or_else(|_| "0.0.0.0".to_string());

    let prometheus_port = env::var("PROMETHEUS_PORT").unwrap_or_else(|_| "9100".to_string());

    let block_window: i64 = env::var("BLOCK_WINDOW")
        .unwrap_or_else(|_| "500".to_string())
        .parse()
        .unwrap();

    let validator_address = env::var("VALIDATOR_ADDRESS").unwrap();

    let rpc_endpoints = env::var("RPC_ENDPOINTS").unwrap();
    let rest_endpoints = env::var("REST_ENDPOINTS").unwrap();

    let blockchain_type = env::var("BLOCKCHAIN").unwrap();

    info!("RCosmos Exporter");
    info!(
        prometheus_ip,
        prometheus_port,
        block_window,
        validator_address,
        rpc_endpoints,
        rest_endpoints,
        blockchain_type
    );

    let blockchain_type = match BlockchainType::from_str(&blockchain_type) {
        Some(blockchain) => blockchain,
        None => panic!("Unsupported blockchain"),
    };

    let rpc = HttpClient::new(split_urls(rpc_endpoints), None);
    let rest = HttpClient::new(split_urls(rest_endpoints), None);

    let blockchain = match blockchain_type {
        BlockchainType::Tendermint => {
            let client = BlockchainClientBuilder::new(validator_address, block_window)
                .with_rest(rest)
                .with_rpc(rpc)
                .build()
                .await;

            blockchains::tendermint::metrics::register_custom_metrics();
            Blockchain::Tendermint(Tendermint {
                client: Arc::new(Mutex::new(client)),
            })
        }
    };

    blockchain.start_monitoring().await;
    serve_metrics(prometheus_ip, prometheus_port, blockchain_type).await;
}

fn split_urls(urls: String) -> Vec<(String, String)> {
    urls.split(';') // Split on semicolons to get pairs
        .filter_map(|pair| {
            let mut parts = pair.split(','); // Split each pair by the comma
            let url = parts.next().map(|s| s.to_string());
            let health_url = parts.next().map(|s| s.to_string());
            match (url, health_url) {
                (Some(url), Some(health_url)) => Some((url, health_url)),
                _ => None,
            }
        })
        .collect()
}
