use crate::core::chain_id::ChainIdFetcher;

use blockchains::{
    babylon::bls_scrapper::BabylonBlsScrapper,
    coredao::{
        block_scrapper::CoreDaoBlockScrapper, validator_info_scrapper::CoreDaoValidatorInfoScrapper,
    },
    lombard::ledger_scrapper::LombardLedgerScrapper,
    mezo::{block_scrapper::MezoBlockScrapper, validator_info_scrapper::MezoValidatorInfoScrapper},
    tendermint::{
        block_scrapper::TendermintBlockScrapper, chain_id::TendermintChainIdFetcher,
        node_status_scrapper::TendermintNodeStatusScrapper,
        proposal_scrapper::TendermintProposalScrapper,
        upgrade_plan_scrapper::TendermintUpgradePlanScrapper,
        validator_info_scrapper::TendermintValidatorInfoScrapper,
    },
};
use tokio_util::sync::CancellationToken;

use core::{
    blockchain::Blockchain,
    clients::{blockchain_client::BlockchainClientBuilder, http_client::HttpClient},
    exporter::{BlockchainExporter, ExporterTask, Mode},
    metrics::{
        exporter_metrics::{register_app_version_info, start_heartbeat},
        serve_metrics::serve_metrics,
    },
};
use dotenv::dotenv;
use std::{env, sync::Arc, time::Duration};
use tokio::{signal, sync::mpsc::unbounded_channel};
use tracing::{error, info};
mod blockchains;
mod core;

#[tokio::main]
async fn main() {
    let mut env_file = None;
    let mut args = std::env::args().peekable();
    while let Some(arg) = args.next() {
        if arg == "--env" {
            if let Some(file) = args.next() {
                env_file = Some(file);
            }
        }
    }
    if let Some(file) = env_file {
        dotenv::from_filename(file).ok();
    } else {
        dotenv().ok();
    }

    tracing_subscriber::fmt().with_target(false).init();

    let (sender, mut receiver) = unbounded_channel::<()>();
    let token = CancellationToken::new();

    let prometheus_ip = env::var("PROMETHEUS_IP").unwrap_or_else(|_| "0.0.0.0".to_string());

    let prometheus_port = env::var("PROMETHEUS_PORT").unwrap_or_else(|_| "9100".to_string());

    let block_window: usize = env::var("BLOCK_WINDOW")
        .unwrap_or_else(|_| "500".to_string())
        .parse()
        .unwrap();

    let validator_alert_addresses =
        env::var("VALIDATOR_ALERT_ADDRESSES").unwrap_or_else(|_| "".to_string());

    let blockchain = env::var("BLOCKCHAIN").expect("You must passs BLOCKCHAIN env var.");
    let mode = env::var("MODE").expect("You must pass MODE env var.");
    let network = env::var("NETWORK").expect("You must passs NETWORK env var.");

    println!("{}", ascii_art());

    let blockchain = match Blockchain::from_str(&blockchain) {
        Some(blockchain) => blockchain,
        None => panic!("Unsupported blockchain"),
    };

    let mode = match Mode::from_str(&mode) {
        Some(mode) => mode,
        None => panic!("Unsupported mode, must be either network or node"),
    };

    let exporter: BlockchainExporter = match mode {
        Mode::Network => {
            let rpc_endpoints = env::var("RPC_ENDPOINTS").expect("You must pass RPC_ENDPOINTS");
            let rest_endpoints = env::var("REST_ENDPOINTS").expect("You must pass REST_ENDPOINTS");

            info!("--------------------------------------------------------------------");
            info!("MODE: {}", mode);
            info!("BLOCKCHAIN: {}", blockchain);
            info!("NETWORK: {}", network);
            info!("VALIDATOR_ALERT_ADDRESES: {}", validator_alert_addresses);
            info!("PROMETHEUS_IP: {}", prometheus_ip);
            info!("PROMETHEUS_PORT: {}", prometheus_port);
            info!("BLOCK_WINDOW: {}", block_window);
            info!("RPC_ENDPOINTS: {}", rpc_endpoints);
            info!("REST_ENDPOINTS: {}", rest_endpoints);
            info!("--------------------------------------------------------------------");

            network_exporter(
                &blockchain,
                rpc_endpoints,
                rest_endpoints,
                block_window,
                network.clone(),
                validator_alert_addresses,
            )
            .await
        }
        Mode::Node => {
            let name = env::var("NODE_NAME").expect("You must pass NODE_NAME env var.");
            let rpc_endpoint =
                env::var("NODE_RPC_ENDPOINT").expect("You must pass NODE_RPC_ENDPOINT env var.");
            let rest_endpoint =
                env::var("NODE_REST_ENDPOINT").expect("You must pass NODE_REST_ENDPOINT env var.");

            info!("--------------------------------------------------------------------");
            info!("MODE: {}", mode);
            info!("NODE_NAME: {}", name);
            info!("NETWORK: {}", network);
            info!("NODE_RPC_ENDPOINT: {}", rpc_endpoint);
            info!("NODE_REST_ENDPOINT: {}", rest_endpoint);
            info!("--------------------------------------------------------------------");

            blockchains::tendermint::metrics::register_custom_metrics();

            let node_status_scrapper = ExporterTask::new(
                Box::new(TendermintNodeStatusScrapper::new(
                    name,
                    rpc_endpoint,
                    rest_endpoint,
                    network.clone(),
                )),
                Duration::from_secs(5),
            );

            BlockchainExporter::new().add_task(node_status_scrapper)
        }
    };

    register_app_version_info(network.clone());
    start_heartbeat(network.clone()).await;
    exporter.print_tasks().await;
    exporter.start(token.clone(), sender, network.clone());

    tokio::select! {
        _ = serve_metrics(
            prometheus_ip,
            prometheus_port,
            blockchain,
        ) => {
            error!("Hyper server exited.");
        },
        _ = listen_for_shutdown(token.clone()) => {

            let number_of_tasks = exporter.number_of_tasks();
            let mut finished_tasks = 0;

            while let Some(_) = receiver.recv().await {
                finished_tasks += 1;
                info!("Waiting for tasks: {}/{}", finished_tasks, number_of_tasks);
                if finished_tasks == number_of_tasks {
                    info!("All tasks finished...");
                    break;
                }
            }

            info!("Gracefuly shutted down server.")
        }
    }
}

pub async fn network_exporter(
    blockchain: &Blockchain,
    rpc_endpoints: String,
    rest_endpoints: String,
    block_window: usize,
    network: String,
    validator_alert_addresses: String,
) -> BlockchainExporter {
    let rpc = HttpClient::new(split_urls(rpc_endpoints), None, network.clone());
    let rest = HttpClient::new(split_urls(rest_endpoints), None, network.clone());
    let validator_alert_addresses = split_validator_addresses(validator_alert_addresses);

    match blockchain {
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
                    network.clone(),
                    validator_alert_addresses.clone(),
                )),
                Duration::from_secs(30),
            );

            let validator_info_scrapper = ExporterTask::new(
                Box::new(TendermintValidatorInfoScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                    validator_alert_addresses.clone(),
                )),
                Duration::from_secs(300),
            );

            let proposal_scrapper = ExporterTask::new(
                Box::new(TendermintProposalScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                )),
                Duration::from_secs(300),
            );

            let upgrade_plan_scrapper = ExporterTask::new(
                Box::new(TendermintUpgradePlanScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                )),
                Duration::from_secs(300),
            );

            blockchains::tendermint::metrics::register_custom_metrics();

            BlockchainExporter::new()
                .add_task(block_scrapper)
                .add_task(validator_info_scrapper)
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
                Box::new(MezoBlockScrapper::new(
                    Arc::clone(&client),
                    block_window,
                    chain_id.clone(),
                    network.clone(),
                    validator_alert_addresses.clone(),
                )),
                Duration::from_secs(30),
            );

            let validator_info_scrapper: ExporterTask = ExporterTask::new(
                Box::new(MezoValidatorInfoScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                    validator_alert_addresses.clone(),
                )),
                Duration::from_secs(300),
            );

            let upgrade_plan_scrapper = ExporterTask::new(
                Box::new(TendermintUpgradePlanScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                )),
                Duration::from_secs(300),
            );

            blockchains::tendermint::metrics::register_custom_metrics();

            BlockchainExporter::new()
                .add_task(block_scrapper)
                .add_task(validator_info_scrapper)
                .add_task(upgrade_plan_scrapper)
        }
        Blockchain::Babylon => {
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
                    network.clone(),
                    validator_alert_addresses.clone(),
                )),
                Duration::from_secs(30),
            );

            let consensus_scrapper: ExporterTask = ExporterTask::new(
                Box::new(TendermintValidatorInfoScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                    validator_alert_addresses.clone(),
                )),
                Duration::from_secs(300),
            );

            let upgrade_plan_scrapper = ExporterTask::new(
                Box::new(TendermintUpgradePlanScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                )),
                Duration::from_secs(300),
            );

            let proposal_scrapper = ExporterTask::new(
                Box::new(TendermintProposalScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                )),
                Duration::from_secs(300),
            );

            let bls_scrapper = ExporterTask::new(
                Box::new(BabylonBlsScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                    validator_alert_addresses.clone(),
                )),
                Duration::from_secs(300),
            );

            // let secret_id = env::var("BABYLON_CUBIST_SESSION_SECRET_ID").expect("You must pass BABYLON_CUBIST_SESSION_SECRET_ID env var");

            // let cubist_client = CubistClient::new(secret_id)
            //     .await
            //     .expect("Could not initialize Cubist client");

            // let cubist_metrics_exporter = ExporterTask::graceful(
            //     Box::new(BabylonCubistMetricScrapper::new(
            //         cubist_client,
            //         chain_id.clone(),
            //     )),
            //     Duration::from_secs(300),
            //     sender.clone(),
            // );

            blockchains::tendermint::metrics::register_custom_metrics();
            blockchains::babylon::metrics::register_custom_metrics();

            BlockchainExporter::new()
                .add_task(block_scrapper)
                .add_task(consensus_scrapper)
                .add_task(upgrade_plan_scrapper)
                .add_task(bls_scrapper)
                .add_task(proposal_scrapper)
            // .add_task(cubist_metrics_exporter)
        }
        Blockchain::CoreDao => {
            let client = BlockchainClientBuilder::new().with_rpc(rpc).build().await;

            let client = Arc::new(client);

            // Register CoreDao metrics
            blockchains::coredao::metrics::register_custom_metrics();

            let block_scrapper = ExporterTask::new(
                Box::new(CoreDaoBlockScrapper::new(
                    Arc::clone(&client),
                    validator_alert_addresses.clone(),
                    network.clone(),
                )),
                Duration::from_secs(15),
            );

            let validator_info_scrapper = ExporterTask::new(
                Box::new(CoreDaoValidatorInfoScrapper::new(
                    Arc::clone(&client),
                    validator_alert_addresses.clone(),
                    network.clone(),
                )),
                Duration::from_secs(60),
            );

            BlockchainExporter::new()
                .add_task(block_scrapper)
                .add_task(validator_info_scrapper)
        }
        Blockchain::Lombard => {
            let validator_operator_addresses =
                env::var("VALIDATOR_OPERATOR_ADDRESSES").unwrap_or_else(|_| "".to_string());
            let validator_operator_addresses =
                split_validator_addresses(validator_operator_addresses);

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

            // Register Lombard custom metrics
            blockchains::lombard::metrics::register_ledger_metrics();
            blockchains::tendermint::metrics::register_custom_metrics();

            let block_scrapper = ExporterTask::new(
                Box::new(TendermintBlockScrapper::new(
                    Arc::clone(&client),
                    block_window,
                    chain_id.clone(),
                    network.clone(),
                    validator_alert_addresses.clone(),
                )),
                Duration::from_secs(30),
            );

            let validator_info_scrapper = ExporterTask::new(
                Box::new(TendermintValidatorInfoScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                    validator_alert_addresses.clone(),
                )),
                Duration::from_secs(300),
            );

            let proposal_scrapper = ExporterTask::new(
                Box::new(TendermintProposalScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                )),
                Duration::from_secs(300),
            );

            let upgrade_plan_scrapper = ExporterTask::new(
                Box::new(TendermintUpgradePlanScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
                    network.clone(),
                )),
                Duration::from_secs(300),
            );

            let ledger_scrapper = ExporterTask::new(
                Box::new(LombardLedgerScrapper::new(
                    Arc::clone(&client),
                    validator_operator_addresses.clone(),
                    network.clone(),
                )),
                Duration::from_secs(1800),
            );

            BlockchainExporter::new()
                .add_task(block_scrapper)
                .add_task(validator_info_scrapper)
                .add_task(proposal_scrapper)
                .add_task(upgrade_plan_scrapper)
                .add_task(ledger_scrapper)
        }
    }
}

pub async fn listen_for_shutdown(cancel_token: CancellationToken) {
    let sigint = signal::ctrl_c();
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate()).unwrap();

    tokio::select! {
        _ = sigint => info!("Received SIGINT"),
        _ = sigterm.recv() => info!("Received SIGTERM"),
    }

    if cfg!(debug_assertions) {
        info!("Received Ctrl+C in dev mode. Shutting down immediately.");
        std::process::exit(0);
    }

    cancel_token.cancel();
}

fn split_validator_addresses(addresses: String) -> Vec<String> {
    addresses.split(';').map(|s| s.trim().to_string()).collect()
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

fn ascii_art() -> &'static str {
    r#"

██████╗  ██████╗ ██████╗ ███████╗███╗   ███╗ ██████╗ ███████╗    
██╔══██╗██╔════╝██╔═══██╗██╔════╝████╗ ████║██╔═══██╗██╔════╝    
██████╔╝██║     ██║   ██║███████╗██╔████╔██║██║   ██║███████╗    
██╔══██╗██║     ██║   ██║╚════██║██║╚██╔╝██║██║   ██║╚════██║    
██║  ██║╚██████╗╚██████╔╝███████║██║ ╚═╝ ██║╚██████╔╝███████║    
╚═╝  ╚═╝ ╚═════╝ ╚═════╝ ╚══════╝╚═╝     ╚═╝ ╚═════╝ ╚══════╝    

███████╗██╗  ██╗██████╗  ██████╗ ██████╗ ████████╗███████╗██████╗ 
██╔════╝╚██╗██╔╝██╔══██╗██╔═══██╗██╔══██╗╚══██╔══╝██╔════╝██╔══██╗
█████╗   ╚███╔╝ ██████╔╝██║   ██║██████╔╝   ██║   █████╗  ██████╔╝
██╔══╝   ██╔██╗ ██╔═══╝ ██║   ██║██╔══██╗   ██║   ██╔══╝  ██╔══██╗
███████╗██╔╝ ██╗██║     ╚██████╔╝██║  ██║   ██║   ███████╗██║  ██║
╚══════╝╚═╝  ╚═╝╚═╝      ╚═════╝ ╚═╝  ╚═╝   ╚═╝   ╚══════╝╚═╝  ╚═╝                                                                                                                             
    "#
}
