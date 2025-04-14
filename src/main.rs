use crate::core::chain_id::ChainIdFetcher;
use blockchains::{
    babylon::{
        bls_scrapper::BabylonBlsScrapper,
        // cubist::{
        //     client::Client as CubistClient, cubist_metrics_scrapper::BabylonCubistMetricScrapper,
        // },
    },
    mezo::validator_info_scrapper::MezoValidatorInfoScrapper,
    tendermint::{
        block_scrapper::TendermintBlockScrapper, chain_id::TendermintChainIdFetcher,
        node_status_scrapper::TendermintNodeStatusScrapper,
        proposal_scrapper::TendermintProposalScrapper,
        upgrade_plan_scrapper::TendermintUpgradePlanScrapper,
        validator_info_scrapper::TendermintValidatorInfoScrapper,
    },
};
use core::{
    blockchain::Blockchain,
    clients::{blockchain_client::BlockchainClientBuilder, http_client::HttpClient},
    exporter::{BlockchainExporter, ExporterTask, Mode},
    metrics::serve_metrics::serve_metrics,
};
use dotenv::dotenv;
use std::{env, sync::Arc, time::Duration};
use tokio::{
    signal,
    sync::{
        mpsc::{unbounded_channel, UnboundedSender},
        Notify,
    },
};
use tracing::{error, info};
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

    let shutdown_notify = Arc::new(Notify::new());
    let (sender, mut receiver) = unbounded_channel::<()>();

    let prometheus_ip = env::var("PROMETHEUS_IP").unwrap_or_else(|_| "0.0.0.0".to_string());

    let prometheus_port = env::var("PROMETHEUS_PORT").unwrap_or_else(|_| "9100".to_string());

    let block_window: usize = env::var("BLOCK_WINDOW")
        .unwrap_or_else(|_| "500".to_string())
        .parse()
        .unwrap();

    let blockchain = env::var("BLOCKCHAIN").expect("You must passs BLOCKCHAIN env var.");
    let mode = env::var("MODE").expect("You must pass MODE env var.");

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
                sender,
            )
            .await
        }
        Mode::Node => {
            let name = env::var("NODE_NAME").expect("You must pass NODE_NAME env var.");
            let endpoint = env::var("NODE_ENDPOINT").expect("You must pass NODE_ENDPOINT env var.");

            info!("--------------------------------------------------------------------");
            info!("MODE: {}", mode);
            info!("NODE_NAME: {}", name);
            info!("NODE_ENDPOINT: {}", endpoint);
            info!("--------------------------------------------------------------------");

            blockchains::tendermint::metrics::register_custom_metrics();

            let node_status_scrapper = ExporterTask::new(
                Box::new(TendermintNodeStatusScrapper::new(name, endpoint)),
                Duration::from_secs(5),
            );

            BlockchainExporter::new().add_task(node_status_scrapper)
        }
    };

    exporter.start(Arc::clone(&shutdown_notify));

    tokio::select! {
        _ = serve_metrics(
            prometheus_ip,
            prometheus_port,
            blockchain,
            block_window,
        ) => {
            error!("Hyper server exited.");
        },
        _ = listen_for_shutdown(Arc::clone(&shutdown_notify)) => {

            let graceful_tasks = exporter.graceful_task_count();

            if graceful_tasks != 0 {
                info!("Waiting for graceful tasks");

                let mut finished_tasks = 0;
                while let Some(_) = receiver.recv().await {
                    finished_tasks += 1;
                    info!("Waiting for graceful tasks: {}/{}", finished_tasks, graceful_tasks);
                    if finished_tasks == graceful_tasks {
                        info!("All graceful tasks finished...");
                        break;
                    }
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
    sender: UnboundedSender<()>,
) -> BlockchainExporter {
    let rpc = HttpClient::new(split_urls(rpc_endpoints), None);
    let rest = HttpClient::new(split_urls(rest_endpoints), None);

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
                )),
                Duration::from_secs(30),
            );

            let validator_info_scrapper = ExporterTask::new(
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
                Box::new(TendermintBlockScrapper::new(
                    Arc::clone(&client),
                    block_window,
                    chain_id.clone(),
                )),
                Duration::from_secs(30),
            );

            let validator_info_scrapper: ExporterTask = ExporterTask::new(
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
                )),
                Duration::from_secs(30),
            );

            let consensus_scrapper: ExporterTask = ExporterTask::new(
                Box::new(TendermintValidatorInfoScrapper::new(
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

            let bls_scrapper = ExporterTask::new(
                Box::new(BabylonBlsScrapper::new(
                    Arc::clone(&client),
                    chain_id.clone(),
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
            // .add_task(cubist_metrics_exporter)
        }
    }
}

pub async fn listen_for_shutdown(shutdown_notify: Arc<Notify>) {
    let sigint = signal::ctrl_c();
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate()).unwrap();

    tokio::select! {
        _ = sigint => info!("Received SIGINT, shutting down graceful tasks"),
        _ = sigterm.recv() => info!("Received SIGTERM, shutting down graceful tasks"),
    }

    shutdown_notify.notify_waiters();
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
