use crate::core::chain_id::ChainIdFetcher;
use blockchains::{
    mezo::validator_info_scrapper::MezoValidatorInfoScrapper,
    tendermint::{
        block_scrapper::TendermintBlockScrapper, chain_id::TendermintChainIdFetcher,
        proposal_scrapper::TendermintProposalScrapper,
        upgrade_plan_scrapper::TendermintUpgradePlanScrapper,
        validator_info_scrapper::TendermintValidatorInfoScrapper,
    },
};
use core::{
    blockchain::Blockchain,
    clients::{blockchain_client::BlockchainClientBuilder, http_client::HttpClient},
    exporter::{BlockchainExporter, ExporterTask},
    metrics::serve_metrics::serve_metrics,
};
use dotenv::dotenv;
use std::{env, sync::Arc, time::Duration};
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

    let block_window: usize = env::var("BLOCK_WINDOW")
        .unwrap_or_else(|_| "500".to_string())
        .parse()
        .unwrap();

    let rpc_endpoints = env::var("RPC_ENDPOINTS").unwrap();
    let rest_endpoints = env::var("REST_ENDPOINTS").unwrap();

    let blockchain = env::var("BLOCKCHAIN").unwrap();

    info!("RCosmos Exporter");
    info!(
        prometheus_ip,
        prometheus_port, block_window, rpc_endpoints, rest_endpoints, blockchain
    );

    let blockchain = match Blockchain::from_str(&blockchain) {
        Some(blockchain) => blockchain,
        None => panic!("Unsupported blockchain"),
    };

    let rpc = HttpClient::new(split_urls(rpc_endpoints), None);
    let rest = HttpClient::new(split_urls(rest_endpoints), None);

    let exporter = match blockchain {
        Blockchain::Tendermint => {
            let client = BlockchainClientBuilder::new()
                .with_rest(rest)
                .with_rpc(rpc)
                .build()
                .await;

            let client = Arc::new(client);

            let chain_id = TendermintChainIdFetcher::new(Arc::clone(&client))
                .get_chain_id()
                .await
                .unwrap();

            let block_scrapper = ExporterTask::new(
                Box::new(TendermintBlockScrapper::new(
                    Arc::clone(&client),
                    block_window,
                    chain_id.clone(),
                )),
                Duration::from_secs(30),
            );

            let consensus_scrapper = ExporterTask::new(
                Box::new(TendermintValidatorInfoScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                )),
                Duration::from_secs(300),
            );

            let proposal_scrapper = ExporterTask::new(
                Box::new(TendermintProposalScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                )),
                Duration::from_secs(300),
            );

            let upgrade_plan_scrapper = ExporterTask::new(
                Box::new(TendermintUpgradePlanScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                )),
                Duration::from_secs(300),
            );

            blockchains::tendermint::metrics::register_custom_metrics();

            BlockchainExporter::new()
                .add_task(block_scrapper)
                .add_task(consensus_scrapper)
                .add_task(proposal_scrapper)
                .add_task(upgrade_plan_scrapper)
        }
        Blockchain::Mezo => {
            let client = BlockchainClientBuilder::new()
                .with_rest(rest)
                .with_rpc(rpc)
                .build()
                .await;

            let client = Arc::new(client);

            let chain_id = TendermintChainIdFetcher::new(Arc::clone(&client))
                .get_chain_id()
                .await
                .unwrap();

            let block_scrapper = ExporterTask::new(
                Box::new(TendermintBlockScrapper::new(
                    Arc::clone(&client),
                    block_window,
                    chain_id.clone(),
                )),
                Duration::from_secs(30),
            );

            let consensus_scrapper: ExporterTask = ExporterTask::new(
                Box::new(MezoValidatorInfoScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                )),
                Duration::from_secs(300),
            );

            let upgrade_plan_scrapper = ExporterTask::new(
                Box::new(TendermintUpgradePlanScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                )),
                Duration::from_secs(300),
            );

            blockchains::tendermint::metrics::register_custom_metrics();

            BlockchainExporter::new()
                .add_task(block_scrapper)
                .add_task(consensus_scrapper)
                .add_task(upgrade_plan_scrapper)
        }
    };

    exporter.start();

    serve_metrics(prometheus_ip, prometheus_port, blockchain, block_window).await;
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
