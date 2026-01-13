#![recursion_limit = "256"]

use std::sync::Arc;
use tracing::{error, info};

use rcosmos_exporter::core::app_context::AppContext;
use rcosmos_exporter::core::backfill::run_backfill;
use rcosmos_exporter::core::clients::http_client::NodePool;
use rcosmos_exporter::core::config::AppConfig;
use rcosmos_exporter::blockchains::cometbft::chain_id::fetch_chain_id;

#[tokio::main]
async fn main() {
    if let Err(e) = dotenv::dotenv() {
        tracing::debug!("No .env file found: {}", e);
    }
    tracing_subscriber::fmt().with_target(false).init();

    // Parse flags: --config, --start-height, --end-height
    let mut args = std::env::args().skip(1);
    let mut config_path = "config.yaml".to_string();
    let mut start_height: Option<usize> = None;
    let mut end_height: Option<usize> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                if let Some(path) = args.next() {
                    config_path = path;
                } else {
                    error!("--config flag provided but no file specified");
                    std::process::exit(1);
                }
            }
            "--start-height" => {
                if let Some(v) = args.next() {
                    match v.parse::<usize>() {
                        Ok(n) => start_height = Some(n),
                        Err(_) => {
                            error!("--start-height must be a positive integer");
                            std::process::exit(1);
                        }
                    }
                } else {
                    error!("--start-height flag provided but no value specified");
                    std::process::exit(1);
                }
            }
            "--end-height" => {
                if let Some(v) = args.next() {
                    match v.parse::<usize>() {
                        Ok(n) => end_height = Some(n),
                        Err(_) => {
                            error!("--end-height must be a positive integer");
                            std::process::exit(1);
                        }
                    }
                } else {
                    error!("--end-height flag provided but no value specified");
                    std::process::exit(1);
                }
            }
            other => {
                error!("Unknown argument: {}", other);
                std::process::exit(1);
            }
        }
    }

    // Read and parse config
    let config_str = match std::fs::read_to_string(&config_path) {
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

    // Build node pools
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
    let rpc_timeout = std::time::Duration::from_secs(config.general.rpc_timeout_seconds);
    let rpc_pool = NodePool::new(rpc_nodes, None, config.general.network.clone(), Some(rpc_timeout)).map(|np| Arc::new(np));
    let lcd_pool = NodePool::new(lcd_nodes, None, config.general.network.clone(), Some(rpc_timeout)).map(|np| Arc::new(np));

    if let Some(ref rpc) = rpc_pool {
        let rpc_clone = rpc.clone();
        tokio::spawn(async move { rpc_clone.start_health_checks(); });
    }
    if let Some(ref lcd) = lcd_pool {
        let lcd_clone = lcd.clone();
        tokio::spawn(async move { lcd_clone.start_health_checks(); });
    }

    // Determine chain_id same way as main
    let chain_id = if config.general.chain_id == "cometbft" {
        match rpc_pool.as_ref() {
            Some(rpc) => match fetch_chain_id(&**rpc).await {
                Ok(cid) => cid,
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

    // Validate args
    let sh = match start_height {
        Some(v) => v,
        None => {
            error!("--start-height is required");
            std::process::exit(1);
        }
    };
    let eh = match end_height {
        Some(v) => v,
        None => {
            error!("--end-height is required");
            std::process::exit(1);
        }
    };

    info!("Starting backfill from {} to {}", sh, eh);
    if let Err(err) = run_backfill(app_context.clone(), sh, eh).await {
        error!("Backfill failed:");
        for (i, cause) in err.chain().enumerate() {
            error!("  {}: {}", i, cause);
        }
        std::process::exit(1);
    }
    info!("Backfill completed.");
}
