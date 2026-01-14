#![recursion_limit = "256"]

use crate::blockchains::coredao::metrics::coredao_custom_metrics;
use crate::blockchains::lombard::metrics::lombard_custom_metrics;
use crate::blockchains::sei::metrics::sei_custom_metrics;
use crate::blockchains::tendermint::metrics::tendermint_custom_metrics;
use crate::core::config::Mode;
use crate::core::exporter::network_mode_modules;
use crate::core::exporter::node_mode_modules;

use tokio_util::sync::CancellationToken;

use core::exporter::BlockchainExporter;

use std::sync::Arc;
use tokio::{signal, sync::mpsc::unbounded_channel};
use tracing::{error, info};
use tracing_subscriber;
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
    // Initialize tracing subscriber for logging (no spans, just log messages)
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_ansi(true)
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::NONE)
        .init();

    // Load environment variables from .env file if it exists
    if let Err(_) = dotenv::dotenv() {
        // It's okay if .env doesn't exist
    }

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

    info!("[main] Config loaded successfully");

    // Build node pools and HTTP clients
    info!("[main] Building node pools...");
    info!("[main] Building node pools from config...");
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

    // Use configurable timeout: prefer block-specific timeout if set, otherwise use general timeout
    // This allows large blocks (like Celestia) to have longer timeouts without affecting other modules
    let rpc_timeout_seconds = config
        .network
        .cometbft
        .block
        .timeout_seconds
        .unwrap_or(config.general.rpc_timeout_seconds);
    if let Some(block_timeout) = config.network.cometbft.block.timeout_seconds {
        info!("[main] Using block-specific RPC timeout: {}s (general timeout: {}s)", block_timeout, config.general.rpc_timeout_seconds);
    } else {
        info!("[main] Using general RPC timeout: {}s", rpc_timeout_seconds);
    }
    let rpc_timeout = std::time::Duration::from_secs(rpc_timeout_seconds);
    info!("[main] Creating RPC node pool...");
    let rpc_pool =
        NodePool::new(rpc_nodes, None, config.general.network.clone(), Some(rpc_timeout)).map(|np| Arc::new(np));
    info!("[main] Creating LCD node pool...");
    let lcd_pool =
        NodePool::new(lcd_nodes, None, config.general.network.clone(), Some(rpc_timeout)).map(|np| Arc::new(np));

    // Start health checks for node pools in separate threads
    info!("[main] Starting health checks...");
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
    info!("[main] Determining chain_id...");
    let chain_id = if config.general.chain_id == "cometbft" {
        info!("[main] chain_id is 'cometbft', fetching from RPC node...");
        match rpc_pool.as_ref() {
            Some(rpc) => {
                info!("[main] RPC pool available, fetching chain_id...");
                match fetch_chain_id(&**rpc).await {
                    Ok(cid) => {
                        info!("[main] 🚀 Automatically obtained chain_id: {}", cid);
                        cid
                    }
                    Err(e) => {
                        error!("Failed to fetch chain_id from CometBFT node: {}", e);
                        std::process::exit(1);
                    }
                }
            },
            None => {
                error!("No RPC pool available to fetch chain_id for CometBFT");
                std::process::exit(1);
            }
        }
    } else {
        info!("[main] Using configured chain_id: {}", config.general.chain_id);
        config.general.chain_id.clone()
    };

    info!("[main] Creating AppContext...");
    let app_context = Arc::new(AppContext::new(config, rpc_pool, lcd_pool, chain_id));

    // Create cancellation token and channel for shutdown
    info!("[main] Setting up shutdown handlers...");
    let token = CancellationToken::new();
    let (tx, mut rx) = unbounded_channel();

    // Start Prometheus metrics server
    info!("[main] Starting Prometheus metrics server...");
    let prometheus_port = app_context.config.general.metrics.port.to_string();
    let prometheus_ip = app_context.config.general.metrics.address.clone();
    let prometheus_path = app_context.config.general.metrics.path.clone();

    // Start exporter metrics
    info!("[main] Registering exporter metrics...");
    let network = app_context.config.general.network.clone();
    register_app_version_info(network.clone());
    info!("[main] Starting heartbeat...");
    start_heartbeat(network.clone()).await;
    info!("[main] Heartbeat started");

    // Register all module custom metrics
    info!("[main] Registering module custom metrics...");
    cometbft_custom_metrics();
    tendermint_custom_metrics();
    babylon_custom_metrics();
    lombard_custom_metrics();
    coredao_custom_metrics();
    sei_custom_metrics();

    info!("[main] Creating modules for mode: {:?}...", app_context.config.general.mode);
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
    info!("[main] Modules created, starting exporter...");

    // Create BlockchainExporter
    info!("[main] Creating BlockchainExporter...");
    let exporter = BlockchainExporter::new(app_context.clone(), modules);
    info!("[main] Starting exporter...");
    exporter.start(token.clone(), tx);
    info!("[main] Exporter started, entering main event loop...");

    info!("[main] Starting tokio::select! with serve_metrics...");
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
                info!("[main] Waiting for modules: {}/{}", finished_modules, number_of_modules);
                if finished_modules == number_of_modules {
                    info!("[main] All modules finished...");
                    break;
                }
            }

            info!("Gracefully shut down server.")
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
