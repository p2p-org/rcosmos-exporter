use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tracing::{info, warn};
use futures::stream::{self, StreamExt};
use tokio::time::sleep;

use crate::blockchains::cometbft::types::BlockResponse;
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::blockchains::cometbft::types::Block as ChainBlock;
use crate::blockchains::cometbft::types::BlockSignature;
use crate::blockchains::cometbft::block::storage::ClickhouseSignatureStorage;
use crate::blockchains::cometbft::block::storage::SignatureStorage;

fn read_env_var(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("Environment variable {} must be set", key))
}

pub async fn run_backfill(app_context: Arc<AppContext>, start_height: usize, end_height: usize) -> Result<()> {
    let rpc = app_context
        .rpc
        .as_ref()
        .context("RPC pool not available for backfill")?
        .clone();

    // Read ClickHouse connection details
    let clickhouse_url = read_env_var("CLICKHOUSE_URL")?;
    let clickhouse_user = read_env_var("CLICKHOUSE_USER")?;
    let clickhouse_password = read_env_var("CLICKHOUSE_PASSWORD")?;
    let clickhouse_database = read_env_var("CLICKHOUSE_DATABASE")?;
    let chain_id = app_context.chain_id.clone();

    // Build ClickHouse storage for resume check
    let clickhouse_storage = ClickhouseSignatureStorage {
        clickhouse_client: clickhouse::Client::default()
            .with_url(&clickhouse_url)
            .with_user(&clickhouse_user)
            .with_password(&clickhouse_password)
            .with_database(&clickhouse_database),
        chain_id: chain_id.clone(),
    };

    let start = start_height.min(end_height);
    let end = end_height.max(start_height);

    if start >= end {
        bail!("Invalid height range: start ({}) must be less than end ({})", start, end);
    }

    // Check for existing data and resume from last processed height
    info!("Checking for existing backfill data in range {} to {}...", start, end);
    let resume_height = match find_resume_height(&clickhouse_storage, start, end).await {
        Ok(Some(h)) => {
            info!("Found existing data up to height {}. Resuming from height {}.", h, h + 1);
            h + 1
        }
        Ok(None) => {
            info!("No existing data found. Starting from height {}.", start);
            start
        }
        Err(e) => {
            warn!("Failed to check for existing data: {}. Starting from beginning.", e);
            start
        }
    };

    if resume_height > end {
        info!("Backfill already complete for range {} to {}.", start, end);
        return Ok(());
    }

    let total_blocks = end - resume_height + 1;
    info!("Backfilling {} blocks from {} to {} with concurrency 50", total_blocks, resume_height, end);

    // Process blocks with concurrency
    const CONCURRENCY: usize = 50;
    const RATE_LIMIT_MS: u64 = 10; // 10ms delay between batches = ~100 blocks/sec max
    
    let heights: Vec<usize> = (resume_height..=end).collect();
    let processed = Arc::new(AtomicUsize::new(0));

    stream::iter(heights)
        .map(|h| {
            let rpc = rpc.clone();
            let url = clickhouse_url.clone();
            let user = clickhouse_user.clone();
            let password = clickhouse_password.clone();
            let database = clickhouse_database.clone();
            let chain = chain_id.clone();
            
            async move {
                // Create storage instance for this task
                let mut storage = ClickhouseSignatureStorage {
                    clickhouse_client: clickhouse::Client::default()
                        .with_url(&url)
                        .with_user(&user)
                        .with_password(&password)
                        .with_database(&database),
                    chain_id: chain,
                };

                // Fetch block
                let block_path = Path::from(format!("/block?height={}", h));
                let res = rpc.get(block_path).await
                    .with_context(|| format!("Failed to fetch block {}", h))?;
                let block: ChainBlock = serde_json::from_str::<BlockResponse>(&res)
                    .with_context(|| format!("Could not deserialize block response for height {}", h))?
                    .result
                    .block;

                // Collect signatures
                let timestamp_naive = block.header.time;
                let signatures: Vec<String> = block
                    .last_commit
                    .signatures
                    .into_iter()
                    .map(|s: BlockSignature| s.validator_address)
                    .collect();

                // Save to ClickHouse
                storage.save_signatures(h, timestamp_naive, signatures)
                    .await
                    .with_context(|| format!("Failed to persist signatures for height {}", h))?;

                Ok::<usize, anyhow::Error>(h)
            }
        })
        .buffer_unordered(CONCURRENCY)
        .for_each(|result| {
            let processed = processed.clone();
            async move {
                match result {
                    Ok(_h) => {
                        let count = processed.fetch_add(1, Ordering::Relaxed) + 1;
                        
                        // Log progress every 1000 blocks
                        if count % 1000 == 0 || count == total_blocks {
                            let percent = (100.0 * count as f64 / total_blocks as f64).round();
                            info!(
                                "Backfill progress: {}/{} blocks ({:.1}% complete)",
                                count, total_blocks, percent
                            );
                        }
                        
                        // Rate limiting: small delay every CONCURRENCY blocks
                        if count % CONCURRENCY == 0 {
                            sleep(Duration::from_millis(RATE_LIMIT_MS)).await;
                        }
                    }
                    Err(e) => {
                        warn!("Error processing block: {:?}. Continuing with next blocks...", e);
                    }
                }
            }
        })
        .await;

    let final_count = processed.load(Ordering::Relaxed);

    info!("Backfill completed: processed {} blocks from {} to {}", final_count, resume_height, end);
    Ok(())
}

/// Find the highest height already processed in the given range
async fn find_resume_height(
    storage: &ClickhouseSignatureStorage,
    start: usize,
    end: usize,
) -> Result<Option<usize>> {
    // Query ClickHouse for max height in range
    let query = format!(
        "SELECT max(height) as max_h FROM validators_signatures WHERE chain_id = '{}' AND height >= {} AND height <= {}",
        storage.chain_id, start, end
    );
    
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct MaxHeight {
        max_h: Option<u64>,
    }
    
    let result: Option<MaxHeight> = storage
        .clickhouse_client
        .query(&query)
        .fetch_one()
        .await
        .ok();
    
    Ok(result.and_then(|r| r.max_h).map(|h| h as usize))
}
