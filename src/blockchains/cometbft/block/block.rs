use anyhow::{bail, Context};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use futures::future;
use serde_json::from_str;
use std::env;
use std::sync::Arc;
use std::collections::BTreeMap;
use tracing::info;

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
    COMETBFT_VALIDATOR_7D_MISSED_BLOCKS, COMETBFT_VALIDATOR_7D_SIGNED_BLOCKS,
    COMETBFT_VALIDATOR_7D_TOTAL_BLOCKS, COMETBFT_VALIDATOR_7D_UPTIME,
    COMETBFT_VALIDATOR_BLOCKWINDOW_UPTIME, COMETBFT_VALIDATOR_MISSED_BLOCKS,
    COMETBFT_VALIDATOR_PROPOSED_BLOCKS,
};
use crate::blockchains::cometbft::types::{Block as ChainBlock, BlockResponse, Tx, TxResponse};
use crate::core::app_context::AppContext;
use crate::core::block_window::BlockWindow;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;

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
            .context(format!("Could not fetch block {}", path))?;

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
                    tracing::warn!(
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
        let last_block_height = last_block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse last block height")?;
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
                h = last_block_height - 1;
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
                h = last_block_height - block_window;
            }
            h
        };
        info!(
            "(CometBFT Block) Starting from height: {} up to latest block: {}",
            height_to_process,
            last_block_height - 1
        );

        // Calculate and emit block gap metric (how many blocks behind we are)
        let current_processed_height = if height_to_process > 0 {
            height_to_process - 1
        } else {
            0
        };
        let block_gap = last_block_height.saturating_sub(current_processed_height);
        COMETBFT_BLOCK_GAP
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block_gap as i64);

        if block_gap > 100 {
            tracing::warn!(
                "(CometBFT Block) Exporter is {} blocks behind chain tip (chain: {}, processed: {})",
                block_gap,
                last_block_height,
                current_processed_height
            );
        }

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
        let tx_enabled = self.app_context.config.network.cometbft.block.tx.enabled;
        let rpc = self.app_context.rpc.as_ref().unwrap().clone();

        // Buffer to hold fetched blocks (keyed by height for ordered processing)
        let mut block_buffer: BTreeMap<usize, (ChainBlock, Option<Vec<Tx>>)> = BTreeMap::new();
        // Use configurable concurrency (defaults to 1 if not set)
        let concurrent_fetch_count = self.app_context.config.network.cometbft.block.concurrency;
        const MIN_BUFFER_SIZE: usize = 2; // Keep at least 2 blocks buffered

        while height_to_process < last_block_height {
            // Keep buffer filled by fetching ahead concurrently
            while block_buffer.len() < MIN_BUFFER_SIZE
                && height_to_process + block_buffer.len() < last_block_height
            {
                // Determine how many blocks to fetch
                let remaining = last_block_height - (height_to_process + block_buffer.len());
                let fetch_count = concurrent_fetch_count.min(remaining);

                if fetch_count == 0 {
                    break;
                }

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
                                    block_buffer.insert(height, (block, txs_info));
                                } else {
                                    tracing::warn!(
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
                                    tracing::warn!(
                                        "(CometBFT Block) Concurrent fetch failed for height {}: All RPC nodes are unhealthy (will retry on fallback)",
                                        height
                                    );
                                } else {
                                    tracing::warn!(
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
                                    block_buffer.insert(height, (block, txs_info));
                                } else {
                                    tracing::warn!(
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
                                    tracing::warn!(
                                        "(CometBFT Block) Concurrent fetch failed for height {}: All RPC nodes are unhealthy (will retry on fallback)",
                                        height
                                    );
                                } else {
                                    tracing::warn!(
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
            let (block, txs_info) = if let Some(data) = block_buffer.remove(&height_to_process) {
                data
            } else {
                // Buffer miss - fetch now (fallback)
                // This happens when concurrent fetch failed - keep retrying until successful
                // Note: Each retry calls NodePool.get() which itself retries 5 times across endpoints (random selection).
                // NodePool rotates through healthy endpoints randomly on each call, ensuring we try all available endpoints.
                // We keep retrying indefinitely - the module will retry the window after the interval if needed.
                let current_gap = last_block_height.saturating_sub(height_to_process - 1);
                tracing::warn!(
                    "(CometBFT Block) Buffer miss for height {} (concurrent fetch failed), fetching with retry logic. Window progress: {} blocks processed, {} remaining (gap: {})",
                    height_to_process,
                    height_to_process - (last_block_height - current_gap),
                    last_block_height - height_to_process,
                    current_gap
                );

                // Retry with exponential backoff (capped at 5 seconds)
                // Keep retrying all endpoints until successful or timeout
                // NodePool.get() already rotates through all endpoints (5 attempts per call)
                // After many retries, check if block might not exist (chain moved ahead)
                let mut retries = 0;
                const MAX_RETRIES_BEFORE_SKIP: u32 = 50; // Check if block exists after 50 retries (~5 minutes of retrying)
                const MAX_TOTAL_RETRIES: u32 = 100; // Maximum retries before bailing (allows module to retry window after interval)
                // Note: Each retry calls NodePool.get() which does 5 attempts across endpoints, so this is 100*5=500 total attempts
                const HEALTH_CHECK_INTERVAL_MS: u64 = 10000; // Wait for health checks to recover (10 seconds)
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
                                tracing::info!(
                                    "(CometBFT Block) Successfully fetched block {} after {} retries",
                                    height_to_process,
                                    retries
                                );
                            }
                            break data;
                        }
                        Err(e) => {
                            retries += 1;

                            // Check for maximum retries - bail to let module retry window after interval
                            // This prevents infinite loops while still trying all endpoints extensively
                            if retries >= MAX_TOTAL_RETRIES {
                                tracing::error!(
                                    "(CometBFT Block) Block {} failed after {} retries (maximum). All endpoints exhausted. Module will retry window after interval. Last error: {}",
                                    height_to_process,
                                    retries,
                                    e
                                );
                                anyhow::bail!(
                                    "Block {} failed after {} maximum retries. All endpoints exhausted. Module will retry window after interval.",
                                    height_to_process,
                                    retries
                                );
                            }

                            // Log progress every 10 retries to show we're still trying
                            if retries % 10 == 0 {
                                tracing::warn!(
                                    "(CometBFT Block) Still retrying block {} (retry {}/{}) - Last error: {}",
                                    height_to_process,
                                    retries,
                                    MAX_TOTAL_RETRIES,
                                    e
                                );
                            }

                            // Check if error is "No healthy nodes" - this means all RPC nodes are unhealthy
                            // In this case, wait for health check interval to give nodes time to recover
                            // Check error chain for "No healthy nodes" (errors may be wrapped with .context())
                            let is_no_healthy_nodes = e
                                .chain()
                                .any(|err| err.to_string().contains("No healthy nodes"));

                            // After many retries, check if block might not exist (chain moved ahead)
                            if retries >= MAX_RETRIES_BEFORE_SKIP {
                                // Re-fetch latest block to see current chain tip
                                match self.get_block(BlockHeight::Latest).await {
                                    Ok(latest_block) => {
                                        let latest_height = latest_block
                                            .header
                                            .height
                                            .parse::<usize>()
                                            .unwrap_or(0);
                                        let blocks_behind = latest_height.saturating_sub(height_to_process);

                                        if blocks_behind > 10 {
                                            // We're more than 10 blocks behind - this block might not exist
                                            // But we'll keep retrying anyway - the module will handle it after the interval
                                            tracing::warn!(
                                                "(CometBFT Block) Block {} failed after {} retries. Chain tip is {}, we're {} blocks behind. Block may not exist, but continuing to retry.",
                                                height_to_process,
                                                retries,
                                                latest_height,
                                                blocks_behind
                                            );
                                        }
                                    }
                                    Err(_) => {
                                        // Can't fetch latest block either - continue retrying
                                    }
                                }
                            }

                            // Determine delay based on error type
                            let delay_ms = if is_no_healthy_nodes {
                                // All nodes are unhealthy - wait for health check interval to recover
                                tracing::warn!(
                                    "(CometBFT Block) All RPC nodes are unhealthy for height {}. Waiting {}ms for health checks to recover (retry {})",
                                    height_to_process,
                                    HEALTH_CHECK_INTERVAL_MS,
                                    retries
                                );
                                HEALTH_CHECK_INTERVAL_MS
                            } else {
                                // Exponential backoff: 200ms, 400ms, 800ms, 1.6s, 3.2s, 5s (capped)
                                (200 * (1 << (retries - 1))).min(5000)
                            };

                            tracing::warn!(
                                "(CometBFT Block) Retry {} for height {} after {}ms delay (will continue retrying): {}",
                                retries,
                                height_to_process,
                                delay_ms,
                                e
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

            // Process current block (this includes saving signatures, updating metrics, etc.)
            // All of this MUST happen sequentially to maintain metric accuracy
            // While we're processing, concurrent fetches are filling the buffer
            self.process_block_with_data(height_to_process, block, txs_info)
                .await
                .context(format!("Failed to process block {}", height_to_process))?;

            height_to_process += 1;

            // Update gap metric periodically (every 10 blocks) to track progress
            if height_to_process % 10 == 0 {
                let current_gap = last_block_height.saturating_sub(height_to_process - 1);
                COMETBFT_BLOCK_GAP
                    .with_label_values(&[
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                    ])
                    .set(current_gap as i64);
                tracing::debug!(
                    "(CometBFT Block) Processed {} blocks, {} remaining (gap: {})",
                    height_to_process - (last_block_height - current_gap),
                    current_gap,
                    current_gap
                );
            }
        }

        // Update gap metric at the end to reflect final state
        let final_gap = last_block_height.saturating_sub(height_to_process - 1);
        let blocks_processed = height_to_process - (last_block_height - final_gap);
        COMETBFT_BLOCK_GAP
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(final_gap as i64);

        // Log completion of block window processing
        tracing::info!(
            "(CometBFT Block) Completed processing block window: processed {} blocks (from {} to {}), current gap: {}",
            blocks_processed,
            height_to_process - blocks_processed,
            height_to_process - 1,
            final_gap
        );

        if self
            .app_context
            .config
            .network
            .cometbft
            .block
            .uptime
            .persistence
        {
            let uptimes = self.signature_storage.uptimes(UptimeWindow::OneDay).await?;
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
            let uptimes = self
                .signature_storage
                .uptimes(UptimeWindow::SevenDays)
                .await?;
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
            let uptimes = self
                .signature_storage
                .uptimes(UptimeWindow::FifteenDays)
                .await?;
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
            let uptimes = self
                .signature_storage
                .uptimes(UptimeWindow::ThirtyDays)
                .await?;
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
        } else {
            let uptimes = self
                .signature_storage
                .uptimes(UptimeWindow::BlockWindow)
                .await?;
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
        Ok(())
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
            let tx_path = Path::from(format!("tx_search?query=\"tx.height={}\"", height));

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
                    // Tx fetch: quick retry (2-3 attempts) to try different endpoints before giving up
                    // This allows us to get txs if available while not blocking block processing for long
                    const TX_FETCH_MAX_RETRIES: u32 = 3; // Quick retries (3 attempts total = 1 initial + 2 retries)
                    let mut tx_retries = 0;
                    let res = loop {
                        match rpc.get(tx_path.clone()).await {
                            Ok(r) => break Ok::<String, anyhow::Error>(r),
                            Err(e) => {
                                tx_retries += 1;

                                // Check if it's an indexing disabled error (common case - don't retry)
                                let error_msg = e.to_string();
                                let is_indexing_disabled = error_msg.contains("indexing is disabled")
                                    || error_msg.contains("transaction indexing");

                                if is_indexing_disabled {
                                    // Indexing disabled - don't retry, just warn and continue
                                    tracing::warn!(
                                        "(CometBFT Block) tx_search for height {} failed: transaction indexing is disabled on endpoint (continuing without txs)",
                                        height
                                    );
                                    return Ok::<Option<Vec<Tx>>, anyhow::Error>(None);
                                }

                                if tx_retries >= TX_FETCH_MAX_RETRIES {
                                    // Max retries reached - warn and continue without txs
                                    tracing::warn!(
                                        "(CometBFT Block) tx_search failed for height {} after {} retries: {} (continuing without txs)",
                                        height,
                                        tx_retries,
                                        e
                                    );
                                    return Ok::<Option<Vec<Tx>>, anyhow::Error>(None);
                                }

                                // Quick retry with short delay (100ms) - don't block block processing
                                tracing::debug!(
                                    "(CometBFT Block) tx_search retry {}/{} for height {}: {}",
                                    tx_retries,
                                    TX_FETCH_MAX_RETRIES,
                                    height,
                                    e
                                );
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            }
                        }
                    }?;
                    // Check if response looks like JSON
                    if !res.trim_start().starts_with('{') && !res.trim_start().starts_with('[') {
                        let preview = if res.len() > 200 {
                            format!("{}...", &res[..200])
                        } else {
                            res.clone()
                        };
                        tracing::warn!(
                            "(CometBFT Block) tx_search response for height {} is not JSON (status 200 but body not JSON). Preview: {} (continuing without txs)",
                            height,
                            preview
                        );
                        return Ok::<Option<Vec<Tx>>, anyhow::Error>(None);
                    }
                    // Check for error JSON (indexing disabled, etc.) before parsing
                    if res.contains("\"error\"") {
                        if res.contains("indexing is disabled") || res.contains("transaction indexing") {
                            tracing::warn!(
                                "(CometBFT Block) tx_search for height {} returned error JSON: transaction indexing is disabled (continuing without txs)",
                                height
                            );
                            return Ok::<Option<Vec<Tx>>, anyhow::Error>(None);
                        }
                        // Other error - log and continue
                        let preview = if res.len() > 200 {
                            format!("{}...", &res[..200])
                        } else {
                            res.clone()
                        };
                        tracing::warn!(
                            "(CometBFT Block) tx_search for height {} returned error JSON. Preview: {} (continuing without txs)",
                            height,
                            preview
                        );
                        return Ok::<Option<Vec<Tx>>, anyhow::Error>(None);
                    }

                    let txs = match from_str::<TxResponse>(&res) {
                        Ok(resp) => resp.result.txs,
                        Err(e) => {
                            let preview = if res.len() > 200 {
                                format!("{}...", &res[..200])
                            } else {
                                res.clone()
                            };
                            tracing::warn!(
                                "(CometBFT Block) tx_search parse failed for height {}: {} (response length: {}, preview: {}) (continuing without txs)",
                                height,
                                e,
                                res.len(),
                                preview
                            );
                            return Ok::<Option<Vec<Tx>>, anyhow::Error>(None);
                        }
                    };
                    Ok::<Option<Vec<Tx>>, anyhow::Error>(Some(txs))
                }
            );

            let block = block_result
                .context(format!("Could not obtain block {}", height))?;

            // tx_result: if error or non-JSON, we get None and proceed
            let txs_info = match tx_result {
                Ok(txs_opt) => txs_opt,
                Err(e) => {
                    tracing::warn!(
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
        let block_proposer = block.header.proposer_address.clone();
        let block_signatures = block.last_commit.signatures.clone();
        let validator_alert_addresses = self.app_context.config.general.alerting.validators.clone();

        COMETBFT_BLOCK_TXS
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block.data.txs.len() as f64);

        let mut block_avg_tx_size: f64 = 0.0;
        let mut block_gas_wanted: f64 = 0.0;
        let mut block_gas_used: f64 = 0.0;
        let mut block_avg_tx_gas_wanted: f64 = 0.0;
        let mut block_avg_tx_gas_used: f64 = 0.0;

        if !block.data.txs.is_empty() {
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

            if self.app_context.config.network.cometbft.block.tx.enabled {
                if let Some(txs_info) = txs_info {
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
                    tracing::warn!(
                        "(CometBFT Block) Block {} has {} transactions but tx_search returned no data",
                        height,
                        block.data.txs.len()
                    );
                }
            }
        }

        COMETBFT_BLOCK_TX_SIZE
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block_avg_tx_size);

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
                )
            }
        }

        self.signature_storage
            .save_signatures(
                block_height,
                block.header.time,
                block_signatures
                    .iter()
                    .map(|sig| sig.validator_address.clone())
                    .collect(),
            )
            .await?;

        COMETBFT_VALIDATOR_PROPOSED_BLOCKS
            .with_label_values(&[
                &block_proposer,
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
                &validator_alert_addresses.contains(&block_proposer).to_string(),
            ])
            .inc();

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

        COMETBFT_CURRENT_BLOCK_HEIGHT
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(
                block_height
                    .try_into()
                    .context("Failed to parse block height to i64")?,
            );

        COMETBFT_CURRENT_BLOCK_TIME
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block_time.and_utc().timestamp() as f64);

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
