#![recursion_limit = "256"]

use crate::blockchains::coredao::metrics::coredao_custom_metrics;

use crate::blockchains::lombard::metrics::lombard_custom_metrics;
use crate::blockchains::tendermint::metrics::tendermint_custom_metrics;
use crate::core::config::Mode;
use crate::core::exporter::network_mode_modules;
use crate::core::exporter::node_mode_modules;

use tokio_util::sync::CancellationToken;

use core::exporter::BlockchainExporter;

use std::sync::Arc;
use tokio::{signal, sync::mpsc::unbounded_channel};
use tracing::{error, info};
mod blockchains;
mod core;
use crate::core::app_context::AppContext;
use crate::core::clients::http_client::NodePool;
use crate::core::config::AppConfig;
use blockchains::babylon::metrics::babylon_custom_metrics;
use blockchains::cometbft::chain_id::fetch_chain_id;
use blockchains::cometbft::metrics::cometbft_custom_metrics;
use core::metrics::exporter_metrics::{register_app_version_info, start_heartbeat};
use core::metrics::serve_metrics::serve_metrics;
use serde_yaml;
use std::fs;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    println!("{}", ascii_art());

    // Parse --config flag
    let mut args = std::env::args().skip(1);
    let mut config_path = "config.yaml".to_string();
    while let Some(arg) = args.next() {
        if arg == "--config" {
            if let Some(path) = args.next() {
                config_path = path;
            } else {
                error!("--config flag provided but no file specified");
                std::process::exit(1);
            }
        }
    }

    // Read and parse config file
    let config_str = match fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to read {}: {}", config_path, e);
            std::process::exit(1);
        }
    };
    let config: AppConfig = match serde_yaml::from_str(&config_str) {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to parse {}: {}", config_path, e);
            std::process::exit(1);
        }
    };

    // Build node pools from config
    let rpc_nodes: Vec<(String, String, String)> = config
        .general
        .nodes
        .rpc
        .iter()
        .map(|n| (n.name.clone(), n.url.clone(), n.health_endpoint.clone()))
        .collect();
    let lcd_nodes: Vec<(String, String, String)> = config
        .general
        .nodes
        .lcd
        .iter()
        .map(|n| (n.name.clone(), n.url.clone(), n.health_endpoint.clone()))
        .collect();

    let rpc_pool =
        NodePool::new(rpc_nodes, None, config.general.network.clone()).map(|np| Arc::new(np));
    let lcd_pool =
        NodePool::new(lcd_nodes, None, config.general.network.clone()).map(|np| Arc::new(np));

    // Start health checks for node pools in separate threads
    if let Some(ref rpc) = rpc_pool {
        let rpc_clone = rpc.clone();
        tokio::spawn(async move {
            rpc_clone.start_health_checks();
        });
    }
    if let Some(ref lcd) = lcd_pool {
        let lcd_clone = lcd.clone();
        tokio::spawn(async move {
            lcd_clone.start_health_checks();
        });
    }

    // Automatically obtain chain_id for cometbft or allow user to set it
    let chain_id = if config.general.chain_id == "cometbft" {
        match rpc_pool.as_ref() {
            Some(rpc) => match fetch_chain_id(&**rpc).await {
                Ok(cid) => {
                    info!("🚀 Automatically obtained chain_id: {}", cid);
                    cid
                }
                Err(e) => {
                    error!("Failed to fetch chain_id from CometBFT node: {}", e);
                    std::process::exit(1);
                }
            },
            None => {
                error!("No RPC pool available to fetch chain_id for CometBFT");
                std::process::exit(1);
            }
        }
    } else {
        config.general.chain_id.clone()
    };

    let app_context = Arc::new(AppContext::new(config, rpc_pool, lcd_pool, chain_id));

    // Create cancellation token and channel for shutdown
    let token = CancellationToken::new();
    let (tx, mut rx) = unbounded_channel();

    // Start Prometheus metrics server
    let prometheus_port = app_context.config.general.metrics.port.to_string();
    let prometheus_ip = app_context.config.general.metrics.address.clone();
    let prometheus_path = app_context.config.general.metrics.path.clone();

    // Start exporter metrics
    let network = app_context.config.general.network.clone();
    register_app_version_info(network.clone());
    start_heartbeat(network.clone()).await;

    // Register all module custom metrics
    cometbft_custom_metrics();
    tendermint_custom_metrics();
    babylon_custom_metrics();
    lombard_custom_metrics();
    coredao_custom_metrics();

    let modules = match app_context.config.general.mode {
        Mode::Node => {
            let modules = match node_mode_modules(app_context.clone()) {
                Ok(m) => m,
                Err(err) => {
                    error!("Startup failed:");
                    for (i, cause) in err.chain().enumerate() {
                        error!("  {}: {}", i, cause);
                    }
                    std::process::exit(1);
                }
            };
            modules
        }
        Mode::Network => {
            let modules = match network_mode_modules(app_context.clone()) {
                Ok(m) => m,
                Err(err) => {
                    error!("Startup failed:");
                    for (i, cause) in err.chain().enumerate() {
                        error!("  {}: {}", i, cause);
                    }
                    std::process::exit(1);
                }
            };
            modules
        }
    };

    // Create BlockchainExporter
    let exporter = BlockchainExporter::new(app_context.clone(), modules);

    exporter.start(token.clone(), tx);

    tokio::select! {
        _ = serve_metrics(
            prometheus_ip,
            prometheus_port,
            prometheus_path,
        ) => {
            error!("Hyper server exited.");
        },
        _ = listen_for_shutdown(token.clone()) => {

            let number_of_modules = exporter.number_of_modules();
            let mut finished_modules = 0;

            while let Some(_) = rx.recv().await {
                finished_modules += 1;
                info!("Waiting for modules: {}/{}", finished_modules, number_of_modules);
                if finished_modules == number_of_modules {
                    info!("All modules finished...");
                    break;
                }
            }

            info!("Gracefuly shutted down server.")
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
