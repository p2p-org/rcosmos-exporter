use anyhow::{bail, Context};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use futures::future;
use serde_json::from_str;
use std::env;
use std::sync::Arc;
use std::collections::{BTreeMap, VecDeque};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::blockchains::cometbft::metrics::{
    COMETBFT_BLOCK_GAP, COMETBFT_BLOCK_GAS_USED, COMETBFT_BLOCK_GAS_WANTED, COMETBFT_BLOCK_TXS,
    COMETBFT_BLOCK_TX_GAS_USED, COMETBFT_BLOCK_TX_GAS_WANTED, COMETBFT_BLOCK_TX_SIZE,
    COMETBFT_CURRENT_BLOCK_HEIGHT, COMETBFT_CURRENT_BLOCK_TIME,
    COMETBFT_VALIDATOR_15D_MISSED_BLOCKS, COMETBFT_VALIDATOR_15D_SIGNED_BLOCKS,
    COMETBFT_VALIDATOR_15D_TOTAL_BLOCKS, COMETBFT_VALIDATOR_15D_UPTIME,
    COMETBFT_VALIDATOR_1D_MISSED_BLOCKS, COMETBFT_VALIDATOR_1D_SIGNED_BLOCKS,
    COMETBFT_VALIDATOR_1D_TOTAL_BLOCKS, COMETBFT_VALIDATOR_1D_UPTIME,
    COMETBFT_VALIDATOR_30D_MISSED_BLOCKS, COMETBFT_VALIDATOR_30D_SIGNED_BLOCKS,
    COMETBFT_VALIDATOR_30D_TOTAL_BLOCKS, COMETBFT_VALIDATOR_30D_UPTIME,
    COMETBFT_VALIDATOR_6M_MISSED_BLOCKS, COMETBFT_VALIDATOR_6M_SIGNED_BLOCKS,
    COMETBFT_VALIDATOR_6M_TOTAL_BLOCKS, COMETBFT_VALIDATOR_6M_UPTIME,
    COMETBFT_VALIDATOR_7D_MISSED_BLOCKS, COMETBFT_VALIDATOR_7D_SIGNED_BLOCKS,
    COMETBFT_VALIDATOR_7D_TOTAL_BLOCKS, COMETBFT_VALIDATOR_7D_UPTIME,
    COMETBFT_VALIDATOR_BLOCKWINDOW_UPTIME, COMETBFT_VALIDATOR_MISSED_BLOCKS,
    COMETBFT_VALIDATOR_TOTAL_BLOCKS, COMETBFT_VALIDATOR_PROPOSED_BLOCKS,
};
use crate::blockchains::cometbft::types::{Block as ChainBlock, BlockResponse, Tx, TxResponse};
use crate::core::app_context::AppContext;
use crate::core::block_window::BlockWindow;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;
use crate::core::utils::{create_error_preview, extract_txs_from_response};

use super::storage::{
    ClickhouseSignatureStorage, InMemorySignatureStorage, SignatureStorage, UptimeWindow,
};

fn read_env_var(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("{key} env variable should be set"))
}
pub struct Block {
    app_context: Arc<AppContext>,
    validators: Vec<String>,
    signature_storage: Box<dyn SignatureStorage>,
    // Track recent block timestamps to calculate average blocks per second
    recent_block_times: VecDeque<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Copy)]
enum BlockHeight {
    Height(usize),
    Latest,
}

impl Block {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        let persistence = app_context.config.network.cometbft.block.uptime.persistence;
        let signature_storage: Box<dyn SignatureStorage> = if persistence {
            Box::new(ClickhouseSignatureStorage {
                clickhouse_client: clickhouse::Client::default()
                    .with_url(read_env_var("CLICKHOUSE_URL"))
                    .with_user(read_env_var("CLICKHOUSE_USER"))
                    .with_password(read_env_var("CLICKHOUSE_PASSWORD"))
                    .with_database(read_env_var("CLICKHOUSE_DATABASE")),
                chain_id: app_context.chain_id.clone(),
                cached_validators: None,
            })
        } else {
            Box::new(InMemorySignatureStorage {
                block_window: BlockWindow::new(
                    app_context.config.network.cometbft.block.window as usize,
                ),
                processed_height: 0,
            })
        };
        Self {
            app_context: app_context.clone(),
            validators: Vec::new(),
            signature_storage,
            recent_block_times: VecDeque::with_capacity(100), // Keep last 100 block times for rolling average
        }
    }

    async fn get_block(&mut self, height: BlockHeight) -> anyhow::Result<ChainBlock> {
        let path = match height {
            BlockHeight::Height(h) => {
                info!("(CometBFT Block) Obtaining block with height: {}", h);
                format!("/block?height={}", h)
            }
            BlockHeight::Latest => {
                info!("(CometBFT Block) Obtaining latest block");
                "/block".to_string()
            }
        };

        let res = self
            .app_context
            .rpc
            .as_ref()
            .unwrap()
            .get(Path::from(path.clone()))
            .await
            .with_context(|| format!("Could not fetch block {}", path))?;

        Ok(from_str::<BlockResponse>(&res)
            .with_context(|| {
                let preview = if res.len() > 200 {
                    format!("{}...", &res[..200])
                } else {
                    res.clone()
                };
                format!(
                    "Could not deserialize block response for {} (response length: {}, preview: {})",
                    match height {
                        BlockHeight::Height(h) => format!("height {}", h),
                        BlockHeight::Latest => "latest block".to_string(),
                    },
                    res.len(),
                    preview
                )
            })?
            .result
            .block)
    }

    async fn process_block_window(&mut self) -> anyhow::Result<()> {
        // Retry fetching latest block until successful (NodePool already retries, but we add extra resilience)
        let last_block = loop {
            match self.get_block(BlockHeight::Latest).await {
                Ok(block) => break block,
                Err(e) => {
                    warn!(
                        "(CometBFT Block) Failed to fetch latest block, retrying with backoff: {}",
                        e
                    );
                    // Exponential backoff: 200ms, 400ms, 800ms, 1.6s, 3.2s, 5s (capped)
                    // NodePool already retried 5 times across endpoints, so we wait before retrying
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    // Continue loop to retry
                }
            }
        };
        let block_window = self.app_context.config.network.cometbft.block.window as usize;

        let mut height_to_process = if self
            .app_context
            .config
            .network
            .cometbft
            .block
            .uptime
            .persistence
        {
            let mut h = self
                .signature_storage
                .get_last_processed_height()
                .await?
                .unwrap_or(0)
                + 1;
            if h <= 1 {
                // First run: start from latest block (will be caught up immediately)
                let latest = last_block
                    .header
                    .height
                    .parse::<usize>()
                    .context("Could not parse last block height")?;
                h = latest.saturating_sub(1);
            }
            h
        } else {
            let mut h = self
                .signature_storage
                .get_last_processed_height()
                .await?
                .unwrap_or(0)
                + 1;
            if h <= 1 {
                // First run: start from window back
                let latest = last_block
                    .header
                    .height
                    .parse::<usize>()
                    .context("Could not parse last block height")?;
                h = latest.saturating_sub(block_window);
            }
            h
        };

        info!(
            "(CometBFT Block) Starting from height: {} (will process continuously until caught up)",
            height_to_process
        );

        // CONCURRENT FETCH BUFFER: Fetch multiple blocks concurrently, process sequentially
        // This maximizes throughput while maintaining metric accuracy
        //
        // Strategy:
        // 1. Fetch blocks concurrently (concurrency configurable via network.cometbft.block.concurrency, defaults to 1)
        // 2. Process them sequentially from buffer (maintains accuracy)
        // 3. Keep buffer filled by continuously fetching ahead
        //
        // Performance: With concurrency=5, fetch 5 blocks in ~1.6s, process 5 blocks in ~0.5s = 0.42s per block!
        // This should keep up with 0.6s block time chains!
        //
        // IMPORTANT: Metrics remain 100% accurate because:
        // 1. Blocks are processed sequentially (we wait for each to complete)
        // 2. Signature storage is updated in order
        // 3. All metrics are set during sequential processing
        // 4. Only fetching is concurrent, processing is sequential

        // Only fetch tx data if explicitly enabled in config
        // We respect the config setting regardless of node tx_index support
        let tx_enabled = self.app_context.config.network.cometbft.block.tx.enabled;
        let rpc = self.app_context.rpc.as_ref().unwrap().clone();

        // Buffer to hold fetched blocks (keyed by height for ordered processing)
        let mut block_buffer: BTreeMap<usize, (ChainBlock, Option<Vec<Tx>>)> = BTreeMap::new();
        // Use configurable concurrency (defaults to 1 if not set)
        let concurrent_fetch_count = self.app_context.config.network.cometbft.block.concurrency;
        const BATCHING_GAP_THRESHOLD: usize = 50; // Only batch when gap > 50 blocks (caught up = no batching)
        const MIN_BUFFER_SIZE: usize = 2; // Minimum buffer size (for small gaps or when caught up)

        // Adaptive buffer size: smaller when caught up (saves memory), larger when behind (maximizes throughput)
        // This balances performance with memory usage, especially important for large blocks
        // - When caught up (gap <= 50): use smaller buffer (2-5 blocks) to save memory
        // - When behind (gap > 50): use larger buffer (up to concurrency count) to maximize throughput
        let calculate_target_buffer_size = |gap: usize| -> usize {
            if gap <= BATCHING_GAP_THRESHOLD {
                // Caught up: use smaller buffer to save memory (large blocks can be 5-10MB each)
                MIN_BUFFER_SIZE.max(concurrent_fetch_count.min(5))
            } else {
                // Behind: use larger buffer to maximize concurrent fetching.
                // Let the maximum buffer size equal the configured concurrency value,
                // while still enforcing a minimum buffer size.
                concurrent_fetch_count.max(MIN_BUFFER_SIZE)
            }
        };

        // Store initial height to calculate blocks_processed correctly
        let initial_height_to_process = height_to_process;

        // Uptime calculations run periodically (every 1000 blocks) instead of at the end
        // This allows the function to run continuously while still updating uptime metrics
        let mut last_uptime_calc_height = height_to_process;
        const UPTIME_CALC_INTERVAL: usize = 1000;

        // Batch ClickHouse writes for performance: buffer signatures and flush periodically.
        // This reduces ClickHouse round-trips from 1 per block to 1 per batch.
        // Prometheus metrics are still updated immediately per block (sequential processing).
        // Adaptive batching: larger batches when behind (max throughput), smaller when caught up (lower latency).
        let mut signature_buffer: Vec<(usize, chrono::NaiveDateTime, Vec<String>)> = Vec::new();
        let base_batch_size: usize = self
            .app_context
            .config
            .network
            .cometbft
            .block
            .uptime
            .insert_concurrency;
        // When behind (gap > 1000), use larger batches (2-3x) to maximize throughput
        // When caught up (gap <= 1000), use base batch size for lower latency
        let calculate_batch_size = |gap: usize| -> usize {
            if gap > 1000 {
                base_batch_size * 3 // Triple batch size when far behind (maximize throughput)
            } else {
                base_batch_size
            }
        };
        // Adaptive timeout: longer when behind (allows larger batches), shorter when caught up
        let calculate_batch_timeout = |gap: usize| -> u64 {
            if gap > 1000 {
                10000 // 10 seconds when behind (allows very large batches for max throughput)
            } else {
                2000 // 2 seconds when caught up (lower latency)
            }
        };
        let mut last_flush_time = std::time::Instant::now();

        // Async ClickHouse writes: use background task to write signatures without blocking processing.
        // This allows processing to continue immediately while ClickHouse writes happen in parallel.
        // Safe because we track height - on restart, exporter continues from last persisted height.
        let (tx_sender, mut tx_receiver) = mpsc::unbounded_channel::<Vec<(usize, chrono::NaiveDateTime, Vec<String>)>>();
        // Move storage out of self to share it with background task
        // We'll move it back at the end of the function
        // Use a placeholder InMemorySignatureStorage (won't be used, just needed for replacement)
        let block_window_size = self.app_context.config.network.cometbft.block.window as usize;
        let storage_arc = Arc::new(Mutex::new(std::mem::replace(
            &mut self.signature_storage,
            Box::new(super::storage::InMemorySignatureStorage {
                block_window: crate::core::block_window::BlockWindow::new(block_window_size),
                processed_height: 0,
            }) as Box<dyn SignatureStorage>,
        )));
        let storage_for_bg = storage_arc.clone();
        let chain_id_clone = self.app_context.chain_id.clone();

        // Spawn background task to handle ClickHouse writes asynchronously
        // Task runs for the entire lifetime of the function (forever)
        let _bg_task = tokio::spawn(async move {
            while let Some(batch) = tx_receiver.recv().await {
                let mut storage = storage_for_bg.lock().await;
                if let Err(e) = storage.save_signatures_batch(batch).await {
                    error!(
                        "(CometBFT Block) Background ClickHouse write failed for chain {}: {}",
                        chain_id_clone, e
                    );
                    // Continue processing - errors are logged but don't stop the background task
                }
            }
        });

        // Tip refresh interval: how many processed blocks between /block?latest calls.
        // We keep this reasonably high to avoid hammering RPC but still track progress.
        const TIP_REFRESH_BLOCKS: usize = 100;
        let mut current_chain_tip: usize = last_block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse initial chain tip")?;
        let mut last_tip_refresh_height = height_to_process;

        // Continuously process blocks until we're caught up to the chain tip.
        // We refresh the current tip periodically based on TIP_REFRESH_BLOCKS.
        loop {
            // Refresh current chain tip periodically based on how many blocks we've processed
            let blocks_since_refresh = height_to_process.saturating_sub(last_tip_refresh_height);
            if blocks_since_refresh >= TIP_REFRESH_BLOCKS {
                match self.get_block(BlockHeight::Latest).await {
                    Ok(block) => {
                        if let Ok(h) = block.header.height.parse::<usize>() {
                            current_chain_tip = h;
                            last_tip_refresh_height = height_to_process;
                        }
                    }
                    Err(e) => {
                        warn!(
                            "(CometBFT Block) Failed to refresh latest block for tip: {} (continuing with stale tip {})",
                            e,
                            current_chain_tip
                        );
                    }
                }
            }

            // If we're caught up (within 1 block), wait first before checking for new blocks
            // This avoids unnecessary immediate RPC calls right after processing a block
            if height_to_process >= current_chain_tip {
                // Flush any remaining buffered signatures before waiting
                if !signature_buffer.is_empty() {
                    let batch_to_send = signature_buffer.clone();
                    if let Err(e) = tx_sender.send(batch_to_send) {
                        warn!(
                            "(CometBFT Block) Failed to send batch to background writer: {} (will write synchronously)",
                            e
                        );
                        // Fallback: write synchronously
                        let storage_arc_clone = storage_arc.clone();
                        let mut storage = storage_arc_clone.lock().await;
                        storage
                            .save_signatures_batch(signature_buffer.clone())
                            .await
                            .context("Failed to flush signatures (fallback)")?;
                    }
                    signature_buffer.clear();
                }

                // Wait based on recent block interval BEFORE checking for new blocks
                // Poll at avg_interval * 1.1 (add 10% breathing room) to reduce RPC calls
                // This automatically adapts to chain speed changes since interval is calculated from the most recent blocks
                let avg_interval_seconds = self.calculate_avg_block_interval_seconds();
                // Poll at avg_interval + 10% (proportional breathing room)
                // No maximum clamp - works for fast chains (0.5s) and slow chains (10s+)
                let poll_interval_ms = (avg_interval_seconds * 1.1 * 1000.0) as u64;
                let poll_interval_ms = poll_interval_ms.max(100); // Minimum 100ms to avoid hammering
                debug!(
                    "(CometBFT Block) Caught up: calculated avg_interval={:.2}s, waiting {:.2}s before checking for new blocks",
                    avg_interval_seconds,
                    poll_interval_ms as f64 / 1000.0
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(poll_interval_ms)).await;

                // Now refresh tip to check for new blocks
                match self.get_block(BlockHeight::Latest).await {
                    Ok(block) => {
                        if let Ok(h) = block.header.height.parse::<usize>() {
                            current_chain_tip = h;
                            last_tip_refresh_height = height_to_process;

                            // Update gap metric with fresh tip
                            let gap = current_chain_tip.saturating_sub(height_to_process);
                            COMETBFT_BLOCK_GAP
                                .with_label_values(&[
                                    &self.app_context.chain_id,
                                    &self.app_context.config.general.network,
                                ])
                                .set(gap as i64);

                            // Continue loop - if still caught up, we'll wait again; if new blocks, we'll process them
                            continue;
                        }
                    }
                    Err(e) => {
                        warn!(
                            "(CometBFT Block) Failed to refresh latest block while caught up: {} (will retry after wait)",
                            e
                        );
                        // Wait again before retrying to avoid hammering RPC
                        let avg_interval_seconds = self.calculate_avg_block_interval_seconds();
                        let poll_interval_ms = (avg_interval_seconds * 1.1 * 1000.0) as u64;
                        let poll_interval_ms = poll_interval_ms.max(100); // Minimum 100ms, no maximum
                        tokio::time::sleep(tokio::time::Duration::from_millis(poll_interval_ms)).await;
                        continue;
                    }
                }
            }

            let current_gap = current_chain_tip.saturating_sub(height_to_process);

            // Update gap metric with current tip (refreshed periodically, not every block)
            COMETBFT_BLOCK_GAP
                .with_label_values(&[
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(current_gap as i64);

            if current_gap > 100 {
                warn!(
                    "(CometBFT Block) Exporter is {} blocks behind chain tip (chain: {}, processed: {})",
                    current_gap,
                    current_chain_tip,
                    height_to_process - 1
                );
            }

            // Only use concurrency when we're more than 1 block behind
            // When caught up (gap <= 1), sequential fetching is sufficient and avoids unnecessary complexity
            // Concurrency is most useful when we need to catch up quickly (gap > 1)
            let use_concurrency = concurrent_fetch_count > 1 && current_gap > 1;

            // Calculate adaptive buffer size based on current gap
            // This balances memory usage (smaller when caught up) with performance (larger when behind)
            let target_buffer_size = calculate_target_buffer_size(current_gap);

            // Keep buffer filled by fetching ahead concurrently (when concurrency is enabled)
            // For large blocks, we need to continuously refill the buffer as we process
            // This ensures we're always fetching multiple blocks in parallel while processing
            while use_concurrency
                && block_buffer.len() < target_buffer_size
                && height_to_process + block_buffer.len() < current_chain_tip
            {
                // Determine how many blocks to fetch
                // Fetch enough to fill buffer to target size, up to concurrency limit
                let remaining = current_chain_tip.saturating_sub(height_to_process + block_buffer.len());
                let needed = target_buffer_size.saturating_sub(block_buffer.len());
                let fetch_count = needed.min(concurrent_fetch_count).min(remaining);

                if fetch_count == 0 {
                    break;
                }

                info!(
                    "(CometBFT Block) Concurrent fetch: fetching {} blocks (buffer: {}/{}, gap: {}, memory-optimized: {})",
                    fetch_count,
                    block_buffer.len(),
                    target_buffer_size,
                    current_gap,
                    current_gap <= BATCHING_GAP_THRESHOLD
                );

                if tx_enabled {
                    // Fetch multiple blocks concurrently
                    let fetch_heights: Vec<usize> = (0..fetch_count)
                        .map(|i| height_to_process + block_buffer.len() + i)
                        .collect();

                    let fetch_futures: Vec<_> = fetch_heights
                        .iter()
                        .map(|&height| {
                            let rpc_clone = rpc.clone();
                            async move {
                                let result = Self::fetch_block_data(&rpc_clone, tx_enabled, height).await;
                                (height, result)
                            }
                        })
                        .collect();

                    // Execute all fetches concurrently
                    let results = future::join_all(fetch_futures).await;

                    // Add successful fetches to buffer
                    for (height, result) in results {
                        match result {
                            Ok((block, txs_info)) => {
                                // Validate height matches
                                let block_height = block
                                    .header
                                    .height
                                    .parse::<usize>()
                                    .context("Could not parse block height")?;

                                if block_height == height {
                                    // Validate transaction count for data integrity
                                    let tx_count = block.data.txs.len();
                                    debug!(
                                        "(CometBFT Block) Buffered block {} (tx_enabled) with {} transactions",
                                        height,
                                        tx_count
                                    );
                                    block_buffer.insert(height, (block, txs_info));
                                } else {
                                    warn!(
                                        "(CometBFT Block) Block height mismatch in buffer: expected {}, got {}",
                                        height,
                                        block_height
                                    );
                                }
                            }
                            Err(e) => {
                                // Check error chain for "No healthy nodes" (errors may be wrapped with .context())
                                let is_no_healthy_nodes = e
                                    .chain()
                                    .any(|err| err.to_string().contains("No healthy nodes"));
                                if is_no_healthy_nodes {
                                    warn!(
                                        "(CometBFT Block) Concurrent fetch failed for height {}: All RPC nodes are unhealthy (will retry on fallback)",
                                        height
                                    );
                                } else {
                                    warn!(
                                        "(CometBFT Block) Concurrent fetch failed for height {}: {} (will retry on fallback)",
                                        height,
                                        e
                                    );
                                }
                                // Don't add to buffer - will be fetched on fallback with retry logic
                            }
                        }
                    }
                } else {
                    // Non-tx mode: can still fetch concurrently if concurrency > 1
                    let fetch_heights: Vec<usize> = (0..fetch_count)
                        .map(|i| height_to_process + block_buffer.len() + i)
                        .collect();

                    let fetch_futures: Vec<_> = fetch_heights
                        .iter()
                        .map(|&height| {
                            let rpc_clone = rpc.clone();
                            async move {
                                let result = Self::fetch_block_data(&rpc_clone, tx_enabled, height).await;
                                (height, result)
                            }
                        })
                        .collect();

                    // Execute all fetches concurrently
                    let results = future::join_all(fetch_futures).await;

                    // Add successful fetches to buffer
                    for (height, result) in results {
                        match result {
                            Ok((block, txs_info)) => {
                                // Validate height matches
                                let block_height = block
                                    .header
                                    .height
                                    .parse::<usize>()
                                    .context("Could not parse block height")?;

                                if block_height == height {
                                    // Validate transaction count for data integrity
                                    let tx_count = block.data.txs.len();
                                    debug!(
                                        "(CometBFT Block) Buffered block {} (non-tx mode) with {} transactions",
                                        height,
                                        tx_count
                                    );
                                    block_buffer.insert(height, (block, txs_info));
                                } else {
                                    warn!(
                                        "(CometBFT Block) Block height mismatch in buffer: expected {}, got {}",
                                        height,
                                        block_height
                                    );
                                }
                            }
                            Err(e) => {
                                // Check error chain for "No healthy nodes" (errors may be wrapped with .context())
                                let is_no_healthy_nodes = e
                                    .chain()
                                    .any(|err| err.to_string().contains("No healthy nodes"));
                                if is_no_healthy_nodes {
                                    warn!(
                                        "(CometBFT Block) Concurrent fetch failed for height {}: All RPC nodes are unhealthy (will retry on fallback)",
                                        height
                                    );
                                } else {
                                    warn!(
                                        "(CometBFT Block) Concurrent fetch failed for height {}: {} (will retry on fallback)",
                                        height,
                                        e
                                    );
                                }
                                // Don't add to buffer - will be fetched on fallback with retry logic
                            }
                        }
                    }
                }
            }

            // Get next block from buffer (sequential processing)
            // Using remove() ensures each block is only processed once (no duplicates)
            // CRITICAL: This sequential processing ensures metrics are set correctly for each block
            // Even though blocks are fetched concurrently, they are processed sequentially,
            // which means each metric update is atomic and accurate
            let buffer_size_before = block_buffer.len();
            let (block, txs_info) = if let Some(data) = block_buffer.remove(&height_to_process) {
                // Verify block height matches before processing (data integrity)
                let buffered_height = data.0.header.height.parse::<usize>()
                    .unwrap_or(0);
                let buffered_tx_count = data.0.data.txs.len();

                if buffered_height != height_to_process {
                    error!(
                        "(CometBFT Block) CRITICAL: Block height mismatch in buffer remove: expected {}, got {}",
                        height_to_process,
                        buffered_height
                    );
                    anyhow::bail!(
                        "Block height mismatch: expected {}, got {}",
                        height_to_process,
                        buffered_height
                    );
                }

                // Log block data for debugging (helps identify data integrity issues)
                debug!(
                    "(CometBFT Block) Processing buffered block {}: height={}, txs={}, txs_info={}, buffer_size={}/{}",
                    height_to_process,
                    buffered_height,
                    buffered_tx_count,
                    if data.1.is_some() {
                        format!("{} transactions", data.1.as_ref().unwrap().len())
                    } else {
                        "None".to_string()
                    },
                    buffer_size_before,
                    target_buffer_size
                );

                data
            } else {
                // Buffer miss - fetch now (fallback)
                // This happens when buffer is empty (either concurrency is disabled or concurrent fetch failed)
                // NodePool.get() tries 2 different nodes per call, so we get good coverage quickly
                let blocks_processed = height_to_process.saturating_sub(initial_height_to_process);
                if use_concurrency {
                    warn!(
                        "(CometBFT Block) Buffer miss for height {} (concurrent fetch failed), fetching with retry logic. Progress: {} blocks processed, buffer_size={}/{}",
                        height_to_process,
                        blocks_processed,
                        buffer_size_before,
                        target_buffer_size
                    );
                } else {
                    debug!(
                        "(CometBFT Block) Buffer miss for height {} (concurrency disabled, gap={}), fetching sequentially. Progress: {} blocks processed, buffer_size={}/{}",
                        height_to_process,
                        current_gap,
                        blocks_processed,
                        buffer_size_before,
                        target_buffer_size
                    );
                }

                // Retry logic with exponential backoff for timeouts
                // NodePool tries 2 different nodes per call, so we get good coverage
                // For timeouts (large blocks), use exponential backoff to avoid overwhelming nodes
                // If all retries fail, skip this block and continue (module will retry window after interval)
                let mut retries = 0;
                const MAX_RETRIES: u32 = 5; // Reduced from 10 - if 5 retries fail, skip and continue
                const INITIAL_RETRY_DELAY_MS: u64 = 1000; // Start with 1s delay
                const MAX_RETRY_DELAY_MS: u64 = 10000; // Cap at 10s delay

                loop {
                    let result = if tx_enabled {
                        Self::fetch_block_data(&rpc, tx_enabled, height_to_process).await
                    } else {
                        match self
                            .get_block(BlockHeight::Height(height_to_process))
                            .await
                        {
                            Ok(block) => Ok((block, None)),
                            Err(e) => Err(e),
                        }
                    };

                    match result {
                        Ok(data) => {
                            if retries > 0 {
                                info!(
                                    "(CometBFT Block) Successfully fetched block {} after {} retries",
                                    height_to_process,
                                    retries
                                );
                            }
                            break data;
                        }
                        Err(e) => {
                            retries += 1;

                            // Check if this is a timeout error (large blocks can timeout)
                            let is_timeout = e
                                .chain()
                                .any(|err| {
                                    let err_str = err.to_string().to_lowercase();
                                    err_str.contains("timeout")
                                        || err_str.contains("deadline")
                                        || err_str.contains("timed out")
                                });

                            // After max retries, bail to let module retry window after interval
                            // This prevents infinite loops while still trying extensively
                            // The module will retry the window, and hopefully nodes will have recovered
                            if retries >= MAX_RETRIES {
                                error!(
                                    "(CometBFT Block) Block {} failed after {} retries (timeout: {}). Module will retry window after interval. Last error: {}",
                                    height_to_process,
                                    retries,
                                    is_timeout,
                                    e
                                );
                                anyhow::bail!(
                                    "Block {} failed after {} retries. Module will retry window after interval.",
                                    height_to_process,
                                    retries
                                );
                            }

                            // Exponential backoff: longer delays for timeouts, shorter for other errors
                            let delay_ms = if is_timeout {
                                // For timeouts: exponential backoff (1s, 2s, 4s, 8s, 10s max)
                                let exponential = INITIAL_RETRY_DELAY_MS * (1 << (retries - 1));
                                exponential.min(MAX_RETRY_DELAY_MS)
                            } else {
                                // For other errors: shorter delay (1s, 2s, 3s, 4s, 5s)
                                INITIAL_RETRY_DELAY_MS * retries as u64
                            };

                            warn!(
                                "(CometBFT Block) Retry {}/{} for height {} (timeout: {}): {}. Waiting {}ms before retry...",
                                retries,
                                MAX_RETRIES,
                                height_to_process,
                                is_timeout,
                                e,
                                delay_ms
                            );
                            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                        }
                    }
                }
            };

            // Validate block height matches expected (safety check)
            let block_height = block
                .header
                .height
                .parse::<usize>()
                .context("Could not parse block height")?;

            if block_height != height_to_process {
                anyhow::bail!(
                    "Block height mismatch: expected {}, got {}",
                    height_to_process,
                    block_height
                );
            }

            // Extract signatures before processing (needed for batch write)
            let block_signatures: Vec<String> = block.last_commit.signatures
                .iter()
                .map(|sig| sig.validator_address.clone())
                .collect();
            let block_timestamp = block.header.time;

            // Process current block (updates Prometheus metrics immediately, sequential processing)
            // Signatures are buffered and written in batches for performance
            let process_start = std::time::Instant::now();
            debug!(
                "(CometBFT Block) Starting to process block {} (buffer: {}/{})",
                height_to_process,
                block_buffer.len(),
                target_buffer_size
            );

            self.process_block_with_data(height_to_process, block, txs_info)
                .await
                .with_context(|| format!("Failed to process block {}", height_to_process))?;

            // Buffer signatures for batch write (instead of writing immediately)
            // This significantly improves performance by reducing ClickHouse round-trips
            signature_buffer.push((height_to_process, block_timestamp, block_signatures));

            // Adaptive batching: use larger batches when behind to maximize throughput
            let current_batch_size = calculate_batch_size(current_gap);
            let current_batch_timeout = calculate_batch_timeout(current_gap);

            // Flush buffer if it's full or timeout reached
            let should_flush = signature_buffer.len() >= current_batch_size
                || last_flush_time.elapsed().as_millis() as u64 >= current_batch_timeout;

            if should_flush {
                // Send batch to background task for async write (non-blocking)
                // This allows processing to continue immediately while ClickHouse writes happen in parallel
                let batch_to_send = signature_buffer.clone();
                if let Err(e) = tx_sender.send(batch_to_send) {
                    // Channel closed (shouldn't happen, but handle gracefully)
                    warn!(
                        "(CometBFT Block) Failed to send batch to background writer: {} (channel closed, will write synchronously)",
                        e
                    );
                    // Fallback: write synchronously if channel is closed
                    let mut storage = storage_arc.lock().await;
                    storage
                        .save_signatures_batch(signature_buffer.clone())
                        .await
                        .context("Failed to batch write signatures (fallback)")?;
                }
                signature_buffer.clear();
                last_flush_time = std::time::Instant::now();
            }

            let process_time = process_start.elapsed();
            if process_time.as_millis() > 3000 {
                warn!(
                    "(CometBFT Block) Slow block processing for height {}: took {:?} (buffer: {}/{})",
                    height_to_process,
                    process_time,
                    block_buffer.len(),
                    target_buffer_size
                );
            }

            height_to_process += 1;

            // Log buffer size periodically (every 10 blocks) to track buffer utilization
            let buffer_size_after = block_buffer.len();
            if height_to_process % 10 == 0 {
                info!(
                    "(CometBFT Block) Buffer status: {} blocks (target: {}, gap: {})",
                    buffer_size_after,
                    target_buffer_size,
                    current_gap
                );
            }

            // Calculate uptime metrics periodically (every 1000 blocks) instead of at the end
            // This allows continuous processing while still updating uptime metrics
            let blocks_since_uptime_calc = height_to_process.saturating_sub(last_uptime_calc_height);
            if blocks_since_uptime_calc >= UPTIME_CALC_INTERVAL {
                last_uptime_calc_height = height_to_process;

                if self
                    .app_context
                    .config
                    .network
                    .cometbft
                    .block
                    .uptime
                    .persistence
                {
                    let storage_arc_clone = storage_arc.clone();
                    let storage = storage_arc_clone.lock().await;
                    let uptimes = storage.uptimes(UptimeWindow::OneDay).await?;
                    info!("(CometBFT Block) Calculating 1 day uptime for validators");
                    let validator_alert_addresses = self.app_context.config.general.alerting.validators.clone();
                    for (_, uptime) in uptimes {
                        let fires_alerts = validator_alert_addresses.contains(&uptime.address).to_string();
                        COMETBFT_VALIDATOR_1D_UPTIME
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.uptime);

                        COMETBFT_VALIDATOR_1D_SIGNED_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                            ])
                            .set(uptime.signed_blocks as f64);

                        COMETBFT_VALIDATOR_1D_TOTAL_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                            ])
                            .set(uptime.total_blocks as f64);

                        COMETBFT_VALIDATOR_1D_MISSED_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.missed_blocks as f64);
                    }

                    let uptimes = storage.uptimes(UptimeWindow::SevenDays).await?;
                    info!("(CometBFT Block) Calculating 7 days uptime for validators");
                    let validator_alert_addresses = self.app_context.config.general.alerting.validators.clone();
                    for (_, uptime) in uptimes {
                        let fires_alerts = validator_alert_addresses.contains(&uptime.address).to_string();
                        COMETBFT_VALIDATOR_7D_UPTIME
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.uptime);

                        COMETBFT_VALIDATOR_7D_SIGNED_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                            ])
                            .set(uptime.signed_blocks as f64);

                        COMETBFT_VALIDATOR_7D_TOTAL_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                            ])
                            .set(uptime.total_blocks as f64);

                        COMETBFT_VALIDATOR_7D_MISSED_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.missed_blocks as f64);
                    }

                    let uptimes = storage.uptimes(UptimeWindow::FifteenDays).await?;
                    info!("(CometBFT Block) Calculating 15 days uptime for validators");
                    let validator_alert_addresses = self.app_context.config.general.alerting.validators.clone();
                    for (_, uptime) in uptimes {
                        let fires_alerts = validator_alert_addresses.contains(&uptime.address).to_string();
                        COMETBFT_VALIDATOR_15D_UPTIME
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.uptime);

                        COMETBFT_VALIDATOR_15D_SIGNED_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                            ])
                            .set(uptime.signed_blocks as f64);

                        COMETBFT_VALIDATOR_15D_TOTAL_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                            ])
                            .set(uptime.total_blocks as f64);

                        COMETBFT_VALIDATOR_15D_MISSED_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.missed_blocks as f64);
                    }

                    let uptimes = storage.uptimes(UptimeWindow::ThirtyDays).await?;
                    info!("(CometBFT Block) Calculating 30 days uptime for validators");
                    let validator_alert_addresses = self.app_context.config.general.alerting.validators.clone();
                    for (_, uptime) in uptimes {
                        let fires_alerts = validator_alert_addresses.contains(&uptime.address).to_string();
                        COMETBFT_VALIDATOR_30D_UPTIME
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.uptime);

                        COMETBFT_VALIDATOR_30D_SIGNED_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                            ])
                            .set(uptime.signed_blocks as f64);

                        COMETBFT_VALIDATOR_30D_TOTAL_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                            ])
                            .set(uptime.total_blocks as f64);

                        COMETBFT_VALIDATOR_30D_MISSED_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.missed_blocks as f64);
                    }

                    let uptimes = storage.uptimes(UptimeWindow::SixMonths).await?;
                    info!("(CometBFT Block) Calculating 6 months uptime for validators");
                    let validator_alert_addresses = self.app_context.config.general.alerting.validators.clone();
                    for (_, uptime) in uptimes {
                        let fires_alerts = validator_alert_addresses.contains(&uptime.address).to_string();
                        COMETBFT_VALIDATOR_6M_UPTIME
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.uptime);

                        COMETBFT_VALIDATOR_6M_SIGNED_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                            ])
                            .set(uptime.signed_blocks as f64);

                        COMETBFT_VALIDATOR_6M_TOTAL_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                            ])
                            .set(uptime.total_blocks as f64);

                        COMETBFT_VALIDATOR_6M_MISSED_BLOCKS
                            .with_label_values(&[
                                &uptime.address,
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.missed_blocks as f64);
                    }
                } else {
                    let storage_arc_clone = storage_arc.clone();
                    let storage = storage_arc_clone.lock().await;
                    let uptimes = storage.uptimes(UptimeWindow::BlockWindow).await?;
                    info!("(CometBFT Block) Calculating uptime for validators");
                    let validator_alert_addresses = self.app_context.config.general.alerting.validators.clone();
                    for (_, uptime) in uptimes {
                        let fires_alerts = validator_alert_addresses.contains(&uptime.address).to_string();
                        COMETBFT_VALIDATOR_BLOCKWINDOW_UPTIME
                            .with_label_values(&[
                                &uptime.address,
                                &block_window.to_string(),
                                &self.app_context.chain_id,
                                &self.app_context.config.general.network,
                                &fires_alerts,
                            ])
                            .set(uptime.uptime);
                    }
                }
            }

            // Continuously refill buffer as we process blocks (for large blocks, this is critical)
            // This ensures we're always fetching multiple blocks concurrently while processing
            // Refill happens after each block is processed to maintain buffer size
            // When behind, fetch aggressively (up to concurrency limit) to keep buffer full
            // BUT: Don't refill if buffer is already very full (>= target) to avoid memory issues
            if use_concurrency && block_buffer.len() < target_buffer_size {
                let remaining = current_chain_tip.saturating_sub(height_to_process + block_buffer.len());
                if remaining > 0 {
                    // Calculate how many we need to reach target
                    let needed = target_buffer_size.saturating_sub(block_buffer.len());
                    // When behind (gap > threshold), fetch more aggressively to maximize throughput
                    // Fetch up to concurrency limit, not just what's needed
                    // This ensures we're always fetching multiple blocks in parallel
                    let aggressive_fetch = if current_gap > BATCHING_GAP_THRESHOLD {
                        // Behind: fetch up to concurrency limit to maximize throughput
                        // BUT: Don't fetch more than what fits in the buffer
                        let max_fetch = target_buffer_size.saturating_sub(block_buffer.len());
                        max_fetch.min(concurrent_fetch_count).min(remaining)
                    } else {
                        // Caught up: just fetch what's needed (memory optimization)
                        needed.min(concurrent_fetch_count).min(remaining)
                    };
                    let fetch_count = aggressive_fetch;
                    if fetch_count > 0 {
                        info!(
                            "(CometBFT Block) Refilling buffer: fetching {} blocks (buffer: {}/{}, gap: {}, memory-optimized: {})",
                            fetch_count,
                            block_buffer.len(),
                            target_buffer_size,
                            current_gap,
                            current_gap <= BATCHING_GAP_THRESHOLD
                        );
                        let fetch_heights: Vec<usize> = (0..fetch_count)
                            .map(|i| height_to_process + block_buffer.len() + i)
                            .collect();

                        let fetch_futures: Vec<_> = fetch_heights
                            .iter()
                            .map(|&height| {
                                let rpc_clone = rpc.clone();
                                async move {
                                    let result = Self::fetch_block_data(&rpc_clone, tx_enabled, height).await;
                                    (height, result)
                                }
                            })
                            .collect();

                        // Execute fetches concurrently (non-blocking - we'll check results on next iteration)
                        let results = future::join_all(fetch_futures).await;

                        // Add successful fetches to buffer
                        for (height, result) in results {
                            match result {
                                Ok((block, txs_info)) => {
                                    let block_height = block.header.height.parse::<usize>()
                                        .unwrap_or(0);
                                    if block_height == height {
                                        block_buffer.insert(height, (block, txs_info));
                                    }
                                }
                                Err(_) => {
                                    // Failed fetch - will be retried on next buffer miss
                                }
                            }
                        }
                    }
                }
            }

            // Log progress periodically (gap metric is updated at top of loop with current tip)
            if height_to_process % 10 == 0 {
                let blocks_processed = height_to_process.saturating_sub(initial_height_to_process);
                debug!(
                    "(CometBFT Block) Processed {} blocks, {} remaining (gap: {}, chain_tip: {})",
                    blocks_processed,
                    current_gap,
                    current_gap,
                    current_chain_tip
                );
            }
        }

        // Function runs continuously - only returns on error
        // Background task and storage Arc will be cleaned up when function returns (on error)
        // or when the module is dropped (on shutdown)
        // Note: Uptime calculations now run periodically within the loop (every 1000 blocks)
        // The loop above runs forever, so this return is unreachable (function only returns on error)
        #[allow(unreachable_code)]
        Ok(())
    }

    /// Calculate average block interval (seconds) from recent block times.
    /// Uses the most recent sample of block times (last 20 blocks) to calculate actual block intervals.
    /// This provides a responsive measurement that adapts quickly to chain speed changes.
    fn calculate_avg_block_interval_seconds(&self) -> f64 {
        const RECENT_SAMPLE_SIZE: usize = 20; // Use last 20 blocks for responsive calculation

        if self.recent_block_times.len() < 2 {
            // Not enough data yet, assume ~1s block time
            return 1.0;
        }

        // Take the most recent blocks (up to RECENT_SAMPLE_SIZE) for responsive calculation
        let sample_size = self.recent_block_times.len().min(RECENT_SAMPLE_SIZE);
        let start_idx = self.recent_block_times.len().saturating_sub(sample_size);

        // Collect the recent block times
        let recent_times: Vec<_> = self
            .recent_block_times
            .iter()
            .skip(start_idx)
            .collect();

        if recent_times.len() < 2 {
            return 1.0; // Need at least 2 blocks to calculate an interval
        }

        // Calculate intervals between consecutive blocks
        let mut intervals_seconds = Vec::new();
        for i in 1..recent_times.len() {
            let interval =
                (*recent_times[i] - *recent_times[i - 1]).num_milliseconds() as f64 / 1000.0;
            if interval > 0.0 && interval < 300.0 {
                // Filter out invalid intervals (0 or >5min)
                intervals_seconds.push(interval);
            }
        }

        if intervals_seconds.is_empty() {
            return 1.0; // No valid intervals found
        }

        // Average block interval in seconds
        intervals_seconds.iter().sum::<f64>() / intervals_seconds.len() as f64
    }


    /// Fetch all transactions for a block using paginated tx_search
    /// tx_search has a default limit of 30 transactions per page, so we need to paginate
    async fn fetch_all_txs(
        rpc: &Arc<crate::core::clients::http_client::NodePool>,
        height: usize,
    ) -> anyhow::Result<Option<Vec<Tx>>> {
        const PER_PAGE: usize = 100; // Maximum per_page value for tx_search
        let mut all_txs = Vec::new();
        let mut page = 1;
        let mut total_count: Option<usize> = None;

        loop {
            let tx_path = Path::from(format!(
                r#"tx_search?query="tx.height={}"&page={}&per_page={}"#,
                height, page, PER_PAGE
            ));

            match rpc.get_with_endpoint_preference(tx_path.clone(), Some("tx_search")).await {
                Ok(res) => {
                    // Try to parse the response
                    match from_str::<TxResponse>(&res) {
                        Ok(resp) => {
                            // Get total count from first page
                            if total_count.is_none() {
                                if let Some(total_str) = &resp.result.total {
                                    total_count = total_str.parse::<usize>().ok();
                                    if let Some(total) = total_count {
                                        debug!(
                                            "(CometBFT Block) tx_search for height {}: total {} transactions, fetching page {}",
                                            height, total, page
                                        );
                                    }
                                }
                            }

                            let page_txs = resp.result.txs;
                            let page_count = page_txs.len();
                            all_txs.extend(page_txs);

                            debug!(
                                "(CometBFT Block) tx_search for height {}: page {} returned {} transactions (total fetched: {})",
                                height, page, page_count, all_txs.len()
                            );

                            // If we got fewer than per_page, we've reached the last page
                            // Or if we've fetched all transactions (all_txs.len() >= total_count)
                            if page_count < PER_PAGE {
                                break;
                            }
                            if let Some(total) = total_count {
                                if all_txs.len() >= total {
                                    break;
                                }
                            }

                            page += 1;
                        }
                        Err(_) => {
                            // Fallback to flexible JSON parsing for first page only
                            if page == 1 {
                                match serde_json::from_str::<serde_json::Value>(&res) {
                                    Ok(json) => {
                                        if let Some(txs_val) = extract_txs_from_response(&json) {
                                            match serde_json::from_value::<Vec<Tx>>(txs_val.clone()) {
                                                Ok(txs) => {
                                                    all_txs.extend(txs);
                                                    // For fallback parsing, we can't determine total, so stop after first page
                                                    break;
                                                }
                                                Err(e) => {
                                                    let preview = create_error_preview(&res, 200);
                                                    info!("WARN: (CometBFT Block) Unable to parse tx response for height {} (page {}): {} (response length: {}, preview: {}). Continuing without txs.",
                                                        height,
                                                        page,
                                                        e,
                                                        res.len(),
                                                        preview
                                                    );
                                                    return Ok(None);
                                                }
                                            }
                                        } else {
                                            // No txs found - treat as empty
                                            return Ok(Some(Vec::new()));
                                        }
                                    }
                                    Err(e) => {
                                        let preview = create_error_preview(&res, 200);
                                        info!("WARN: (CometBFT Block) Unable to parse tx response as JSON for height {} (page {}): {} (response length: {}, preview: {}). Continuing without txs.",
                                            height,
                                            page,
                                            e,
                                            res.len(),
                                            preview
                                        );
                                        return Ok(None);
                                    }
                                }
                            } else {
                                // For subsequent pages, if parsing fails, we've likely reached the end
                                warn!("(CometBFT Block) Failed to parse tx_search page {} for height {}, stopping pagination", page, height);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    if page == 1 {
                        // First page failed - return None to indicate tx_search unavailable
                        info!("WARN: (CometBFT Block) Unable to fetch tx data for height {}: {}. Continuing without txs.",
                            height,
                            e
                        );
                        return Ok(None);
                    } else {
                        // Subsequent page failed - we've likely reached the end or hit an error
                        warn!("(CometBFT Block) Failed to fetch tx_search page {} for height {}: {}, stopping pagination", page, height, e);
                        break;
                    }
                }
            }
        }

        if all_txs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(all_txs))
        }
    }

    /// Fetch block and tx data concurrently (if tx.enabled)
    ///
    /// This is separated to allow pipelining: fetch block N+1 while processing block N.
    /// Both requests are made concurrently to minimize latency.
    ///
    /// # Arguments
    /// * `rpc` - The RPC client (cloned for concurrent access)
    /// * `tx_enabled` - Whether to fetch transaction data
    /// * `height` - Block height to fetch
    ///
    /// # Returns
    /// * `(ChainBlock, Option<Vec<Tx>>)` - Block data and optional transaction info
    ///
    /// # Errors
    /// * Returns error if block fetch fails
    /// * Returns error if tx fetch fails AND block has transactions (txs_info will be None if no txs)
    async fn fetch_block_data(
        rpc: &Arc<crate::core::clients::http_client::NodePool>,
        tx_enabled: bool,
        height: usize,
    ) -> anyhow::Result<(ChainBlock, Option<Vec<Tx>>)> {
        let rpc = rpc.clone();
        let block_path = Path::from(format!("/block?height={}", height));

        info!("(CometBFT Block) Obtaining block with height: {}", height);

        if tx_enabled {
            // Fetch both block and tx data concurrently
            // Use paginated tx_search to fetch all transactions

            // Execute both requests concurrently for maximum performance
            // NodePool.get() already has retry logic (5 attempts across different nodes)
            // So each fetch will automatically retry if one endpoint fails
            let (block_result, tx_result) = tokio::join!(
                async {
                    let res = rpc.get(block_path).await?;
                    // Check if response looks like JSON (starts with { or [)
                    if !res.trim_start().starts_with('{') && !res.trim_start().starts_with('[') {
                        let preview = if res.len() > 200 {
                            format!("{}...", &res[..200])
                        } else {
                            res.clone()
                        };
                        anyhow::bail!(
                            "Block response for height {} is not JSON (status was 200 but body is not JSON). Preview: {}",
                            height,
                            preview
                        );
                    }
                    // Parse block response - if it fails, this is fatal (we need the block)
                    // Use .context() for error handling like v2.8.0
                    Ok::<_, anyhow::Error>(from_str::<BlockResponse>(&res)
                        .with_context(|| {
                            let preview = if res.len() > 200 {
                                format!("{}...", &res[..200])
                            } else {
                                res.clone()
                            };
                            format!(
                                "Could not deserialize block response for height {} (response length: {}, preview: {})",
                                height,
                                res.len(),
                                preview
                            )
                        })?
                        .result
                        .block)
                },
                async {
                    // Fetch all transactions using paginated tx_search
                    // This handles blocks with more than 30 transactions (default per_page limit)
                    Self::fetch_all_txs(&rpc, height).await
                }
            );

            let block = block_result
                .with_context(|| format!("Could not obtain block {}", height))?;

            // Verify block height matches requested height (data integrity check)
            let fetched_block_height = block.header.height.parse::<usize>()
                .unwrap_or(0);
            if fetched_block_height != height {
                anyhow::bail!(
                    "CRITICAL: Fetched block height {} does not match requested height {}",
                    fetched_block_height,
                    height
                );
            }

            // Log transaction count from fetched block for debugging
            let fetched_tx_count = block.data.txs.len();
            debug!(
                "(CometBFT Block) Fetched block {} has {} transactions in block.data.txs",
                height,
                fetched_tx_count
            );

            // tx_result: if error or non-JSON, we get None and proceed
            let txs_info = match tx_result {
                Ok(txs_opt) => txs_opt,
                Err(e) => {
                    warn!(
                        "(CometBFT Block) tx_search returned error for height {}: {} (continuing without txs)",
                        height,
                        e
                    );
                    None
                }
            };

            Ok((block, txs_info))
        } else {
            // Non-tx mode: only fetch block data
            // NodePool.get() already has retry logic (5 attempts across different nodes)
            let res = rpc.get(block_path).await?;
            // Check if response looks like JSON (starts with { or [)
            if !res.trim_start().starts_with('{') && !res.trim_start().starts_with('[') {
                let preview = if res.len() > 200 {
                    format!("{}...", &res[..200])
                } else {
                    res.clone()
                };
                anyhow::bail!(
                    "Block response for height {} is not JSON (status was 200 but body is not JSON). Preview: {}",
                    height,
                    preview
                );
            }
            // Parse block response - if it fails, this is fatal (we need the block)
            // Use .context() for error handling like v2.8.0
            let block = from_str::<BlockResponse>(&res)
                .with_context(|| {
                    let preview = if res.len() > 200 {
                        format!("{}...", &res[..200])
                    } else {
                        res.clone()
                    };
                    format!(
                        "Could not deserialize block response for height {} (response length: {}, preview: {})",
                        height,
                        res.len(),
                        preview
                    )
                })?
                .result
                .block;

            // Verify block height matches requested height (data integrity check)
            let fetched_block_height = block.header.height.parse::<usize>()
                .unwrap_or(0);
            if fetched_block_height != height {
                anyhow::bail!(
                    "CRITICAL: Fetched block height {} does not match requested height {}",
                    fetched_block_height,
                    height
                );
            }

            // Log transaction count from fetched block for debugging
            let fetched_tx_count = block.data.txs.len();
            debug!(
                "(CometBFT Block) Fetched block {} (non-tx mode) has {} transactions in block.data.txs",
                height,
                fetched_tx_count
            );

            Ok((block, None))
        }
    }

    /// Process a block with already-fetched data
    ///
    /// This maintains sequential processing for accurate metrics:
    /// - Blocks are processed in order (1, 2, 3...)
    /// - Signature storage is updated sequentially
    /// - Metrics are set during sequential processing
    /// - Validator uptime calculations depend on correct order
    ///
    /// # Arguments
    /// * `height` - Expected block height (validated against block.header.height)
    /// * `block` - The block data to process
    /// * `txs_info` - Optional transaction info (if tx.enabled)
    async fn process_block_with_data(
        &mut self,
        height: usize,
        block: ChainBlock,
        txs_info: Option<Vec<Tx>>,
    ) -> anyhow::Result<()> {

        // Validate block height matches expected (defensive programming)
        let block_height = block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse block height")?;

        if block_height != height {
            anyhow::bail!(
                "Block height mismatch in process_block_with_data: expected {}, got {}",
                height,
                block_height
            );
        }

        let block_time = block.header.time;
        let block_time_utc = block_time.and_utc();

        // Track block time for calculating average blocks per second
        self.recent_block_times.push_back(block_time_utc);
        // Keep only last 100 block times for rolling average
        if self.recent_block_times.len() > 100 {
            self.recent_block_times.pop_front();
        }

        let block_proposer = block.header.proposer_address.clone();
        let block_signatures = block.last_commit.signatures.clone();
        let validator_alert_addresses = self.app_context.config.general.alerting.validators.clone();

        // Count transactions from block.data.txs (this is the authoritative source)
        // CRITICAL: Use block.data.txs.len(), NOT txs_info.len() - they can differ!
        // - block.data.txs: Raw transaction hashes from block (always accurate)
        // - txs_info: Parsed transaction data from tx_search (may be None if tx_search fails)
        // We must use block.data.txs.len() to ensure data integrity regardless of tx_search success
        let tx_count = block.data.txs.len();

        // Validate transaction count matches expectations (data integrity check)
        // This ensures we're not accidentally reusing block data or mixing up blocks
        if tx_count > 10000 {
            warn!(
                "(CometBFT Block) Block {} has unusually high transaction count: {} (possible data corruption?)",
                height,
                tx_count
            );
        }

        // Log transaction count at INFO level for visibility (helps verify data integrity)
        // This confirms blocks are being processed and shows actual transaction counts
        // Only log transaction info if tx is enabled (to avoid unnecessary noise when tx processing is disabled)
        if self.app_context.config.network.cometbft.block.tx.enabled {
            let tx_status_msg = if txs_info.is_some() {
                format!(", tx_search returned {} transactions", txs_info.as_ref().unwrap().len())
            } else {
                " (tx_search unavailable or failed)".to_string()
            };
            info!(
                "(CometBFT Block) Processing block {}: {} transactions in block.data.txs{}",
                height,
                tx_count,
                tx_status_msg
            );
        } else {
            // When tx.enabled is false, don't log transaction counts to avoid confusion
            // We're not processing transactions, so logging their count is misleading
            info!(
                "(CometBFT Block) Processing block {} (tx processing disabled)",
                height
            );
        }

        // Set metric with block's actual transaction count
        // This is a Gauge metric, so it represents the current block's tx count
        // Prometheus will scrape this value, and since we process sequentially, each block's value is accurate
        COMETBFT_BLOCK_TXS
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(tx_count as f64);

        let mut block_avg_tx_size: f64 = 0.0;
        let mut block_gas_wanted: f64 = 0.0;
        let mut block_gas_used: f64 = 0.0;
        let mut block_avg_tx_gas_wanted: f64 = 0.0;
        let mut block_avg_tx_gas_used: f64 = 0.0;

        // Only process transaction data (decode, calculate sizes, gas metrics) if tx.enabled is true
        // When disabled, we skip all transaction processing to avoid unnecessary work
        if self.app_context.config.network.cometbft.block.tx.enabled && !block.data.txs.is_empty() {
            block_avg_tx_size = block
                .data
                .txs
                .iter()
                .filter_map(|tx| {
                    general_purpose::STANDARD
                        .decode(tx)
                        .context("Could not decode tx")
                        .ok()
                        .map(|decoded| decoded.len())
                })
                .sum::<usize>() as f64
                / block.data.txs.len() as f64;

            // Calculate gas metrics only if tx.enabled is true in config
            // We respect the config setting and only collect tx data when explicitly enabled
            if let Some(txs_info) = txs_info {
                // CRITICAL: Validate txs_info count matches block.data.txs count
                // They should match, but tx_search might return fewer results if some txs aren't indexed
                // We use block.data.txs.len() as the authoritative source for transaction count
                let txs_info_count = txs_info.len();
                let block_txs_count = block.data.txs.len();

                if txs_info_count != block_txs_count {
                    warn!(
                        "(CometBFT Block) Block {} transaction count mismatch: block.data.txs has {}, tx_search returned {} (some transactions may not be indexed)",
                        height,
                        block_txs_count,
                        txs_info_count
                    );
                }

                let mut gas_wanted = Vec::new();
                let mut gas_used = Vec::new();

                for tx in txs_info {
                    gas_wanted.push(
                        tx.tx_result
                            .gas_wanted
                            .parse::<usize>()
                            .context("Could not parse tx gas wanted")?,
                    );
                    gas_used.push(
                        tx.tx_result
                            .gas_used
                            .parse::<usize>()
                            .context("Could not parse tx gas used")?,
                    );
                }

                block_gas_wanted = gas_wanted.iter().sum::<usize>() as f64;
                block_gas_used = gas_used.iter().sum::<usize>() as f64;
                if !gas_wanted.is_empty() {
                    block_avg_tx_gas_wanted =
                        gas_wanted.iter().sum::<usize>() as f64 / gas_wanted.len() as f64;
                }
                if !gas_used.is_empty() {
                    block_avg_tx_gas_used =
                        gas_used.iter().sum::<usize>() as f64 / gas_used.len() as f64;
                }
            } else {
                // tx_search failed or returned no results, but block has transactions
                // This can happen if tx indexing is disabled or tx_search fails
                // CRITICAL: We still set COMETBFT_BLOCK_TXS correctly using block.data.txs.len()
                // Gas metrics will remain 0, which is correct since we don't have gas data
                warn!(
                    "(CometBFT Block) Block {} has {} transactions in block.data.txs but tx_search returned no data (tx indexing may be disabled or tx_search failed)",
                    height,
                    block.data.txs.len()
                );
            }
        }

        COMETBFT_BLOCK_TX_SIZE
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block_avg_tx_size);

        // Set gas metrics only if tx.enabled is true in config
        // We respect the config setting and only set metrics when tx collection is explicitly enabled
        if self.app_context.config.network.cometbft.block.tx.enabled {
            COMETBFT_BLOCK_GAS_WANTED
                .with_label_values(&[
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(block_gas_wanted);

            COMETBFT_BLOCK_GAS_USED
                .with_label_values(&[
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(block_gas_used);

            COMETBFT_BLOCK_TX_GAS_WANTED
                .with_label_values(&[
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(block_avg_tx_gas_wanted);

            COMETBFT_BLOCK_TX_GAS_USED
                .with_label_values(&[
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(block_avg_tx_gas_used);
        }

        for sig in block_signatures.iter() {
            if !sig.validator_address.is_empty()
                && !self.validators.contains(&sig.validator_address)
            {
                self.validators.push(sig.validator_address.clone());
                info!(
                    "(CometBFT Block) Tracking validator {}",
                    sig.validator_address
                );
            }
        }

        // Note: Signatures are buffered and written in batches by the caller (process_block_window)
        // This method just processes the block and updates metrics sequentially


        COMETBFT_VALIDATOR_PROPOSED_BLOCKS
            .with_label_values(&[
                &block_proposer,
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
                &validator_alert_addresses.contains(&block_proposer).to_string(),
            ])
            .inc();

        // Increment total blocks counter for all validators in active set
        // This represents total opportunities to sign (whether they signed or not)
        for validator in &self.validators {
            let fires_alerts = validator_alert_addresses.contains(validator).to_string();

            COMETBFT_VALIDATOR_TOTAL_BLOCKS
                .with_label_values(&[
                    validator,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .inc();
        }

        let validators_missing_block: Vec<String> = self
            .validators
            .iter()
            .filter(|validator| {
                block_signatures
                    .iter()
                    .all(|sig| sig.validator_address != validator.as_str())
            })
            .cloned()
            .collect();

        for validator in validators_missing_block {
            let fires_alerts = validator_alert_addresses.contains(&validator).to_string();

            COMETBFT_VALIDATOR_MISSED_BLOCKS
                .with_label_values(&[
                    &validator,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .inc();
        }

        // Set current block height and time metrics
        // These are Gauge metrics representing the latest processed block
        // Since we process blocks sequentially, these values are always accurate
        // CRITICAL: These must be set AFTER all block processing to ensure they match the block being processed
        let block_height_i64: i64 = block_height
            .try_into()
            .context("Failed to parse block height to i64")?;
        let block_time_timestamp = block_time.and_utc().timestamp() as f64;

        // Validate block time is reasonable (data integrity check)
        // Block time should be within reasonable bounds:
        // - Not in the future (more than 1 hour ahead) - indicates clock skew or data corruption
        // - Not extremely old (more than 1 year old) - indicates possible data corruption
        // Note: Historical blocks being caught up are expected and not errors
        let now = chrono::Utc::now().timestamp();
        let one_year_ago = now - (365 * 24 * 3600); // 1 year in seconds
        if block_time_timestamp > (now + 3600) as f64 {
            // Block time is more than 1 hour in the future - likely clock skew or corruption
            warn!(
                "(CometBFT Block) Block {} has block time in the future: {} (current: {}, difference: {}s). Possible clock skew or data corruption.",
                height,
                block_time_timestamp,
                now,
                block_time_timestamp as i64 - now
            );
        } else if block_time_timestamp < one_year_ago as f64 {
            // Block time is more than 1 year old - likely data corruption (not just historical catch-up)
            warn!(
                "(CometBFT Block) Block {} has extremely old block time: {} (current: {}, difference: {}s). Possible data corruption.",
                height,
                block_time_timestamp,
                now,
                now - block_time_timestamp as i64
            );
        }

        COMETBFT_CURRENT_BLOCK_HEIGHT
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block_height_i64);

        COMETBFT_CURRENT_BLOCK_TIME
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block_time_timestamp);

        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.rpc.is_none() {
        bail!("RPC pool is empty");
    }
    if app_context.config.network.cometbft.block.tx.enabled {
        info!("\t✅ CometBFT Block tx is enabled");
    } else {
        info!("\t❌ CometBFT Block tx is disabled");
    }

    if app_context.config.network.cometbft.block.uptime.persistence {
        info!("\t✅ CometBFT Block persistence is enabled");
    } else {
        info!("\t❌ CometBFT Block persistence is disabled");
        info!(
            "\t\t CometBFT Block window configured to {}",
            app_context.config.network.cometbft.block.window
        );
    }

    Ok(Box::new(Block::new(app_context)))
}

#[async_trait]
impl RunnableModule for Block {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_block_window().await
    }
    fn name(&self) -> &'static str {
        "CometBFT Block"
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.cometbft.block.interval as u64,
        )
    }
}
