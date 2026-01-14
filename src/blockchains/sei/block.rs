use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use anyhow::Context;
use futures::future;
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use serde_json::from_str;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::blockchains::sei::metrics::{COMETBFT_BLOCK_TXS, COMETBFT_CURRENT_BLOCK_HEIGHT};
use crate::blockchains::cometbft::metrics::{
    COMETBFT_BLOCK_GAS_USED, COMETBFT_BLOCK_GAS_WANTED, COMETBFT_BLOCK_GAP,
    COMETBFT_BLOCK_TX_GAS_USED, COMETBFT_BLOCK_TX_GAS_WANTED, COMETBFT_BLOCK_TX_SIZE,
    COMETBFT_CURRENT_BLOCK_TIME,
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
    COMETBFT_VALIDATOR_PROPOSED_BLOCKS,
};
use crate::blockchains::sei::types::{
    SeiBlock, SeiBlockDirect, SeiBlockResponse, SeiSignature,
};
use crate::core::app_context::AppContext;
use crate::core::block_window::BlockWindow;
use crate::core::clients::http_client::NodePool;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;
use crate::blockchains::cometbft::block::storage::{
    ClickhouseSignatureStorage, InMemorySignatureStorage, SignatureStorage, UptimeWindow,
};

pub struct Block {
    pub app_context: Arc<AppContext>,
    validators: Vec<String>,
    signature_storage: Box<dyn SignatureStorage>,
    // Track recent block timestamps to calculate average blocks per second
    recent_block_times: VecDeque<chrono::DateTime<chrono::Utc>>,
}

impl Block {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        fn read_env_var(key: &str) -> String {
            std::env::var(key).unwrap_or_else(|_| panic!("{key} env variable should be set"))
        }
        let signature_storage: Box<dyn SignatureStorage> = if app_context
            .config
            .network
            .sei
            .block
            .uptime
            .persistence
        {
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
                block_window: crate::core::block_window::BlockWindow::new(
                    app_context.config.network.sei.block.window as usize,
                ),
                processed_height: 0,
            })
        };
        Self {
            app_context,
            validators: Vec::new(),
            signature_storage,
            recent_block_times: VecDeque::with_capacity(100), // Keep last 100 block times for rolling average
        }
    }

    /// Fetch all transactions for a block using paginated tx_search
    /// tx_search has a default limit of 30 transactions per page, so we need to paginate
    async fn fetch_sei_txs(&self, height: usize) -> Option<Vec<crate::blockchains::sei::types::SeiTx>> {
        const PER_PAGE: usize = 100; // Maximum per_page value for tx_search
        let mut all_txs = Vec::new();
        let mut page = 1;
        let mut total_count: Option<usize> = None;
        let client = self.app_context.rpc.as_ref().unwrap();

        loop {
            let path = format!(
                r#"/tx_search?query="tx.height={}"&page={}&per_page={}"#,
                height, page, PER_PAGE
            );

            // Tx fetch: use endpoint preference to prioritize nodes with tx_search support
            // NodePool will try preferred nodes first, then fall back to all nodes if they fail
            // This ensures we get tx data when available, but don't block for long if preferred nodes are down
            match client.get_with_endpoint_preference(Path::from(path), Some("tx_search")).await {
                Ok(res) => {
                    // Try to parse the response
                    match from_str::<crate::blockchains::sei::types::SeiTxResponse>(&res) {
                        Ok(resp) => {
                            // Get total count from first page
                            if total_count.is_none() {
                                if let Some(total_str) = &resp.result.total {
                                    total_count = total_str.parse::<usize>().ok();
                                    if let Some(total) = total_count {
                                        debug!(
                                            "(Sei Block) tx_search for height {}: total {} transactions, fetching page {}",
                                            height, total, page
                                        );
                                    }
                                }
                            }

                            let page_txs = resp.result.txs;
                            let page_count = page_txs.len();
                            all_txs.extend(page_txs);

                            debug!(
                                "(Sei Block) tx_search for height {}: page {} returned {} transactions (total fetched: {})",
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
                                    Ok(v) => {
                                        let txs_val_opt = v
                                            .get("result")
                                            .and_then(|r| r.get("txs"))
                                            .or_else(|| v.get("txs"));
                                        if let Some(txs_val) = txs_val_opt {
                                            match serde_json::from_value::<Vec<crate::blockchains::sei::types::SeiTx>>(txs_val.clone()) {
                                                Ok(txs) => {
                                                    all_txs.extend(txs);
                                                    // For fallback parsing, we can't determine total, so stop after first page
                                                    break;
                                                }
                                                Err(e) => {
                                                    let preview = if res.len() > 200 {
                                                        format!("{}...", &res[..200])
                                                    } else {
                                                        res.clone()
                                                    };
                                                    warn!(
                                                        "(Sei Block) Unable to parse tx response for height {} (page {}): {} (response length: {}, preview: {}). Continuing without txs.",
                                                        height,
                                                        page,
                                                        e,
                                                        res.len(),
                                                        preview
                                                    );
                                                    return None;
                                                }
                                            }
                                        } else {
                                            // No txs found - treat as empty
                                            return Some(Vec::new());
                                        }
                                    }
                                    Err(e) => {
                                        let preview = if res.len() > 200 {
                                            format!("{}...", &res[..200])
                                        } else {
                                            res.clone()
                                        };
                                        warn!(
                                            "(Sei Block) Unable to parse tx response as JSON for height {} (page {}): {} (response length: {}, preview: {}). Continuing without txs.",
                                            height,
                                            page,
                                            e,
                                            res.len(),
                                            preview
                                        );
                                        return None;
                                    }
                                }
                            } else {
                                // For subsequent pages, if parsing fails, we've likely reached the end
                                warn!("(Sei Block) Failed to parse tx_search page {} for height {}, stopping pagination", page, height);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    if page == 1 {
                        // First page failed - return None to indicate tx_search unavailable
                        warn!(
                            "(Sei Block) Unable to fetch tx data for height {}: {}. Continuing without txs.",
                            height,
                            e
                        );
                        return None;
                    } else {
                        // Subsequent page failed - we've likely reached the end or hit an error
                        warn!("(Sei Block) Failed to fetch tx_search page {} for height {}: {}, stopping pagination", page, height, e);
                        break;
                    }
                }
            }
        }

        if all_txs.is_empty() {
            None
        } else {
            Some(all_txs)
        }
    }

    async fn get_block(&self, height: Option<usize>) -> anyhow::Result<SeiBlock> {
        let path = match height {
            Some(h) => {
                info!("(Sei Block) Obtaining block with height: {}", h);
                format!("/block?height={}", h)
            }
            None => {
                info!("(Sei Block) Obtaining latest block");
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
        // Be tolerant: parse as generic JSON first, then extract block (object or JSON string)
        let v: serde_json::Value = serde_json::from_str(&res)
            .context("Could not deserialize Sei block response")?;

        // Helper to decode either an object or a JSON-in-string
        fn decode_block_value(val: &serde_json::Value) -> anyhow::Result<SeiBlock> {
            if val.is_object() {
                let block: SeiBlock = serde_json::from_value(val.clone())
                    .context("Could not decode Sei block (object)")?;
                Ok(block)
            } else if let Some(s) = val.as_str() {
                // Some nodes return the block JSON as a string. Parse it.
                let inner: serde_json::Value = serde_json::from_str(s)
                    .context("Could not parse embedded block JSON string")?;
                if let Some(inner_block) = inner.get("block") {
                    let block: SeiBlock = serde_json::from_value(inner_block.clone())
                        .context("Could not decode embedded block from 'block'")?;
                    Ok(block)
                } else {
                    let block: SeiBlock = serde_json::from_value(inner)
                        .context("Could not decode embedded block JSON as SeiBlock")?;
                    Ok(block)
                }
            } else {
                anyhow::bail!("Unsupported block value type")
            }
        }

        if let Some(block_val) = v.get("result").and_then(|r| r.get("block")) {
            return decode_block_value(block_val);
        }
        if let Some(block_val) = v.get("block") {
            return decode_block_value(block_val);
        }
        // As a last resort, try strict structs (in case of exact match)
        if let Ok(resp) = from_str::<SeiBlockResponse>(&res) {
            return Ok(resp.result.block);
        }
        if let Ok(direct) = from_str::<SeiBlockDirect>(&res) {
            return Ok(direct.block);
        }
        anyhow::bail!("Block not found in Sei response (expected result.block or block)")
    }

    async fn process_block(
        &mut self,
        height: usize,
        block: SeiBlock,
    ) -> anyhow::Result<()> {
        let block_height = block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse block height")?;

        let block_time = block.header.time;
        let block_time_utc = block_time.and_utc();

        // Track block time for calculating average blocks per second
        self.recent_block_times.push_back(block_time_utc);
        // Keep only last 100 block times for rolling average
        if self.recent_block_times.len() > 100 {
            self.recent_block_times.pop_front();
        }

        let block_proposer = block.header.proposer_address.clone();
        let block_signatures: Vec<SeiSignature> = block.last_commit.signatures.clone();
        let validator_alert_addresses = self.app_context.config.general.alerting.validators.clone();

        // tx count
        COMETBFT_BLOCK_TXS
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block.data.txs.len() as f64);

        // avg tx size and gas metrics
        let mut block_avg_tx_size: f64 = 0.0;
        let mut block_gas_wanted: f64 = 0.0;
        let mut block_gas_used: f64 = 0.0;
        let mut block_avg_tx_gas_wanted: f64 = 0.0;
        let mut block_avg_tx_gas_used: f64 = 0.0;

        // Only process transaction data (decode, calculate sizes, gas metrics) if tx.enabled is true.
        // When disabled, we skip all transaction processing to avoid unnecessary work.
        if self.app_context.config.network.sei.block.tx.enabled && !block.data.txs.is_empty() {
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

            // Fetch tx data - gracefully handle failures (returns None if tx_search fails)
            if let Some(txs_info) = self.fetch_sei_txs(height).await {
                let mut gas_wanted = Vec::new();
                let mut gas_used = Vec::new();

                for tx in txs_info {
                    if let Some(result) = tx.tx_result {
                        if let Some(gw) = result.gas_wanted {
                            if let Ok(v) = gw.parse::<usize>() { gas_wanted.push(v); }
                        }
                        if let Some(gu) = result.gas_used {
                            if let Ok(v) = gu.parse::<usize>() { gas_used.push(v); }
                        }
                    }
                }

                block_gas_wanted = gas_wanted.iter().sum::<usize>() as f64;
                block_gas_used = gas_used.iter().sum::<usize>() as f64;
                if !gas_wanted.is_empty() {
                    block_avg_tx_gas_wanted =
                        gas_wanted.iter().sum::<usize>() as f64 / gas_wanted.len() as f64;
                    block_avg_tx_gas_used =
                        gas_used.iter().sum::<usize>() as f64 / gas_used.len() as f64;
                }
            }
            // If fetch_sei_txs returns None, we continue without tx data (already logged in fetch_sei_txs)
        }

        COMETBFT_BLOCK_TX_SIZE
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block_avg_tx_size);

        if self.app_context.config.network.sei.block.tx.enabled {
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

        // Log transaction count at INFO level for visibility (helps verify data integrity)
        // This confirms blocks are being processed and shows actual transaction counts
        // Only log transaction info if tx is enabled (to avoid unnecessary noise when tx processing is disabled)
        let tx_count = block.data.txs.len();
        if self.app_context.config.network.sei.block.tx.enabled {
            // For Sei, we don't have tx_search like CometBFT, so just log the count from block.data.txs
            info!(
                "(Sei Block) Processing block {}: {} transactions in block.data.txs",
                height,
                tx_count
            );
        } else {
            // When tx.enabled is false, don't log transaction counts to avoid confusion
            // We're not processing transactions, so logging their count is misleading
            info!(
                "(Sei Block) Processing block {} (tx processing disabled)",
                height
            );
        }

        // track validators seen and proposed/missed
        for sig in block_signatures.iter() {
            if !sig.validator_address.is_empty() && !self.validators.contains(&sig.validator_address) {
                self.validators.push(sig.validator_address.clone());
                info!("(Sei Block) Tracking validator {}", sig.validator_address);
            }
        }

        // Note: Signatures are now buffered and written in batches (handled in process_block_window)
        // We don't call save_signatures here anymore - it's done via the batch buffer

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
            .filter(|validator| block_signatures.iter().all(|sig| sig.validator_address != validator.as_str()))
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
            .set(block_height as i64);

        COMETBFT_CURRENT_BLOCK_TIME
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block_time.and_utc().timestamp() as f64);

        Ok(())
    }

    /// Fetch a Sei block by height using the same tolerant parsing as `get_block`,
    /// but without borrowing `self`, so we can use it from concurrent tasks.
    async fn fetch_block(
        rpc: &Arc<NodePool>,
        height: usize,
    ) -> anyhow::Result<SeiBlock> {
        let path = format!("/block?height={}", height);
        let res = rpc
            .get(Path::from(path.clone()))
            .await
            .context(format!("Could not fetch Sei block {}", path))?;

        let v: serde_json::Value = serde_json::from_str(&res)
            .context("Could not deserialize Sei block response")?;

        fn decode_block_value(val: &serde_json::Value) -> anyhow::Result<SeiBlock> {
            if val.is_object() {
                let block: SeiBlock = serde_json::from_value(val.clone())
                    .context("Could not decode Sei block (object)")?;
                Ok(block)
            } else if let Some(s) = val.as_str() {
                let inner: serde_json::Value = serde_json::from_str(s)
                    .context("Could not parse embedded Sei block JSON string")?;
                if let Some(inner_block) = inner.get("block") {
                    let block: SeiBlock = serde_json::from_value(inner_block.clone())
                        .context("Could not decode embedded Sei block from 'block'")?;
                    Ok(block)
                } else {
                    let block: SeiBlock = serde_json::from_value(inner)
                        .context("Could not decode embedded Sei block JSON as SeiBlock")?;
                    Ok(block)
                }
            } else {
                anyhow::bail!("Unsupported Sei block value type")
            }
        }

        if let Some(block_val) = v.get("result").and_then(|r| r.get("block")) {
            return decode_block_value(block_val);
        }
        if let Some(block_val) = v.get("block") {
            return decode_block_value(block_val);
        }

        if let Ok(resp) = from_str::<SeiBlockResponse>(&res) {
            return Ok(resp.result.block);
        }
        if let Ok(direct) = from_str::<SeiBlockDirect>(&res) {
            return Ok(direct.block);
        }

        anyhow::bail!("Sei block not found in response (expected result.block or block)")
    }

    /// Calculate average blocks per second from recent block times
    /// Returns a rolling average based on the last 100 blocks, or 1.0 if not enough data
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


    async fn process_block_window(&mut self) -> anyhow::Result<()> {
        // Fetch latest block height with retry similar to CometBFT for robustness
        let last_block = loop {
            match self.get_block(None).await {
                Ok(block) => break block,
                Err(e) => {
                    warn!(
                        "(Sei Block) Failed to fetch latest block, retrying with backoff: {}",
                        e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        };

        let block_window = self.app_context.config.network.sei.block.window as usize;

        let mut height_to_process = self
            .signature_storage
            .get_last_processed_height()
            .await?
            .unwrap_or(0)
            + 1;
        if height_to_process <= 1 {
            // If no previous height, start from a reasonable point (window blocks back from latest)
            let initial_tip = last_block
                .header
                .height
                .parse::<usize>()
                .context("Could not parse initial Sei block height")?;
            height_to_process = initial_tip.saturating_sub(block_window);
        }

        // Buffer & concurrency logic (mirrors CometBFT semantics, without tx_search)
        let rpc = self.app_context.rpc.as_ref().unwrap().clone();

        // Buffer of fetched blocks keyed by height
        let mut block_buffer: BTreeMap<usize, SeiBlock> = BTreeMap::new();
        let concurrent_fetch_count = self.app_context.config.network.sei.block.concurrency;
        const BATCHING_GAP_THRESHOLD: usize = 50;
        const MIN_BUFFER_SIZE: usize = 2;

        let calculate_target_buffer_size = |gap: usize| -> usize {
            if gap <= BATCHING_GAP_THRESHOLD {
                MIN_BUFFER_SIZE.max(concurrent_fetch_count.min(5))
            } else {
                concurrent_fetch_count.max(MIN_BUFFER_SIZE)
            }
        };

        // Store initial height to calculate blocks_processed correctly
        let initial_height_to_process = height_to_process;

        // Tip refresh interval: how many processed blocks between /block?latest calls.
        // We keep this reasonably high to avoid hammering RPC but still track progress.
        const TIP_REFRESH_BLOCKS: usize = 100;
        let mut current_chain_tip: usize = last_block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse initial chain tip")?;
        let mut last_tip_refresh_height = height_to_process;

        // Batch ClickHouse writes for performance: buffer signatures and flush periodically.
        // This reduces ClickHouse round-trips from 1 per block to 1 per batch.
        // Prometheus metrics are still updated immediately per block (sequential processing).
        // Adaptive batching: larger batches when behind (max throughput), smaller when caught up (lower latency).
        // Async writes: signatures are written in a background task so block processing is not blocked by ClickHouse.
        let mut signature_buffer: Vec<(usize, chrono::NaiveDateTime, Vec<String>)> = Vec::new();
        let base_batch_size: usize = self
            .app_context
            .config
            .network
            .sei
            .block
            .uptime
            .insert_concurrency;
        let calculate_batch_size = |gap: usize| -> usize {
            if gap > 1000 {
                // When far behind, use larger batches for maximum throughput
                base_batch_size * 3
            } else {
                base_batch_size
            }
        };
        let calculate_batch_timeout = |gap: usize| -> u64 {
            if gap > 1000 {
                // When far behind, allow bigger batches to accumulate
                10_000
            } else {
                // When near tip, flush more frequently for lower latency
                2_000
            }
        };
        let mut last_flush_time = std::time::Instant::now();

        // Async ClickHouse writes: use background task to write signatures without blocking processing.
        // This is safe because we track last processed height in ClickHouse; on restart we resume from there.
        let (tx_sender, mut tx_receiver) = mpsc::unbounded_channel::<Vec<(usize, chrono::NaiveDateTime, Vec<String>)>>();
        // Move storage out of self to share it with the background task; use an in-memory placeholder in self.
        let storage_arc = Arc::new(Mutex::new(std::mem::replace(
            &mut self.signature_storage,
            Box::new(InMemorySignatureStorage {
                block_window: BlockWindow::new(block_window),
                processed_height: 0,
            }) as Box<dyn SignatureStorage>,
        )));
        let storage_for_bg = storage_arc.clone();
        let chain_id_clone = self.app_context.chain_id.clone();

        // Task runs for the entire lifetime of the function (forever)
        let _bg_task = tokio::spawn(async move {
            while let Some(batch) = tx_receiver.recv().await {
                let mut storage = storage_for_bg.lock().await;
                if let Err(e) = storage.save_signatures_batch(batch).await {
                    error!(
                        "(Sei Block) Background ClickHouse write failed for chain {}: {}",
                        chain_id_clone, e
                    );
                }
            }
        });

        // Initial buffer fill (only once, before main processing loop)
        let mut initial_buffer_filled = false;

        // Continuously process blocks until we're caught up to the chain tip.
        // We refresh the current tip periodically based on TIP_REFRESH_BLOCKS.
        loop {
            // Refresh current chain tip periodically based on how many blocks we've processed
            let blocks_since_refresh = height_to_process.saturating_sub(last_tip_refresh_height);
            if blocks_since_refresh >= TIP_REFRESH_BLOCKS {
                match self.get_block(None).await {
                    Ok(block) => {
                        if let Ok(h) = block.header.height.parse::<usize>() {
                            current_chain_tip = h;
                            last_tip_refresh_height = height_to_process;
                        }
                    }
                    Err(e) => {
                        warn!(
                            "(Sei Block) Failed to refresh latest block for tip: {} (continuing with stale tip {})",
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
                            "(Sei Block) Failed to send batch to background writer: {} (will write synchronously)",
                            e
                        );
                        let storage_arc_clone = storage_arc.clone();
                        let mut storage = storage_arc_clone.lock().await;
                        storage
                            .save_signatures_batch(signature_buffer.clone())
                            .await
                            .context("Failed to flush Sei signatures (fallback)")?;
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
                    "(Sei Block) Caught up: calculated avg_interval={:.2}s, waiting {:.2}s before checking for new blocks",
                    avg_interval_seconds,
                    poll_interval_ms as f64 / 1000.0
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(poll_interval_ms)).await;

                // Now refresh tip to check for new blocks
                match self.get_block(None).await {
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
                            "(Sei Block) Failed to refresh latest block while caught up: {} (will retry after wait)",
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

            // Only use concurrency when we're more than 1 block behind
            // When caught up (gap <= 1), sequential fetching is sufficient and avoids unnecessary complexity
            // Concurrency is most useful when we need to catch up quickly (gap > 1)
            let use_concurrency = concurrent_fetch_count > 1 && current_gap > 1;
            let target_buffer_size = calculate_target_buffer_size(current_gap);

            // Initial buffer fill with concurrent fetches (only once, before processing starts)
            // After initial fill, we rely on the refill logic after each block is processed
            if !initial_buffer_filled && use_concurrency && block_buffer.len() < target_buffer_size {
                let remaining = current_chain_tip.saturating_sub(height_to_process + block_buffer.len());
                if remaining > 0 {
                    let needed = target_buffer_size.saturating_sub(block_buffer.len());
                    let fetch_count = needed.min(concurrent_fetch_count).min(remaining);

                    if fetch_count > 0 {
                        info!(
                            "(Sei Block) Initial buffer fill: fetching {} blocks (buffer: {}/{}, gap: {})",
                            fetch_count,
                            block_buffer.len(),
                            target_buffer_size,
                            current_gap
                        );

                        let fetch_heights: Vec<usize> = (0..fetch_count)
                            .map(|i| height_to_process + block_buffer.len() + i)
                            .collect();

                        let fetch_futures: Vec<_> = fetch_heights
                            .iter()
                            .map(|&height| {
                                let rpc_clone = rpc.clone();
                                async move {
                                    let result = Self::fetch_block(&rpc_clone, height).await;
                                    (height, result)
                                }
                            })
                            .collect();

                        let results = future::join_all(fetch_futures).await;

                        for (height, result) in results {
                            match result {
                                Ok(block) => {
                                    let block_height = block
                                        .header
                                        .height
                                        .parse::<usize>()
                                        .context("Could not parse Sei block height")?;

                                    if block_height == height {
                                        let tx_count = block.data.txs.len();
                                        debug!(
                                            "(Sei Block) Buffered block {} with {} transactions",
                                            height,
                                            tx_count
                                        );
                                        block_buffer.insert(height, block);
                                    } else {
                                        warn!(
                                            "(Sei Block) Block height mismatch in buffer: expected {}, got {}",
                                            height,
                                            block_height
                                        );
                                    }
                                }
                                Err(_) => {
                                    // Failed fetch - will be retried on fallback
                                }
                            }
                        }
                    }
                }
                // Mark as filled (even if not completely full) to avoid infinite loops
                initial_buffer_filled = true;
            }

            // Get next block from buffer or fetch on-demand (fallback)
            let buffer_size_before = block_buffer.len();
            let block = if let Some(block) = block_buffer.remove(&height_to_process) {
                let buffered_height = block.header.height.parse::<usize>().unwrap_or(0);
                let buffered_tx_count = block.data.txs.len();

                if buffered_height != height_to_process {
                    anyhow::bail!(
                        "Sei block height mismatch in buffer remove: expected {}, got {}",
                        height_to_process,
                        buffered_height
                    );
                }

                debug!(
                    "(Sei Block) Processing buffered block {}: height={}, txs={}, buffer_size={}/{}",
                    height_to_process,
                    buffered_height,
                    buffered_tx_count,
                    buffer_size_before,
                    target_buffer_size
                );

                block
            } else {
                let current_gap = current_chain_tip.saturating_sub(height_to_process);
                let blocks_processed =
                    height_to_process.saturating_sub(initial_height_to_process);
                // Only warn if concurrency was enabled but failed; otherwise use debug since this is expected
                if use_concurrency {
                    warn!(
                        "(Sei Block) Buffer miss for height {} (concurrent fetch failed), fetching with simple retry. Progress: {} blocks processed, {} remaining (gap: {}), buffer_size={}/{}",
                        height_to_process,
                        blocks_processed,
                        current_gap,
                        current_gap,
                        buffer_size_before,
                        target_buffer_size
                    );
                } else {
                    debug!(
                        "(Sei Block) Buffer miss for height {} (concurrency disabled, gap={}), fetching sequentially. Progress: {} blocks processed, {} remaining, buffer_size={}/{}",
                        height_to_process,
                        current_gap,
                        blocks_processed,
                        current_gap,
                        buffer_size_before,
                        target_buffer_size
                    );
                }

                // Simple retry loop (shorter than CometBFT since Sei blocks are lighter)
                let mut retries = 0u32;
                const MAX_RETRIES: u32 = 5;

                loop {
                    let result = self.get_block(Some(height_to_process)).await;
                    match result {
                        Ok(block) => {
                            if retries > 0 {
                                info!(
                                    "(Sei Block) Successfully fetched block {} after {} retries",
                                    height_to_process,
                                    retries
                                );
                            }
                            break block;
                        }
                        Err(e) => {
                            retries += 1;
                            if retries >= MAX_RETRIES {
                                anyhow::bail!(
                                    "Sei block {} failed after {} retries: {}",
                                    height_to_process,
                                    retries,
                                    e
                                );
                            }
                            warn!(
                                "(Sei Block) Retry {}/{} for block {} failed: {}",
                                retries,
                                MAX_RETRIES,
                                height_to_process,
                                e
                            );
                            tokio::time::sleep(tokio::time::Duration::from_millis(
                                500 * retries as u64,
                            ))
                            .await;
                        }
                    }
                }
            };

            // Process current block (updates Prometheus metrics immediately, sequential processing)
            // Signatures are buffered and written in batches for performance
            let process_start = std::time::Instant::now();
            debug!(
                "(Sei Block) Starting to process block {} (buffer: {}/{})",
                height_to_process,
                block_buffer.len(),
                target_buffer_size
            );

            let block_timestamp = block.header.time;
            let block_signatures: Vec<String> = block
                .last_commit
                .signatures
                .iter()
                .map(|s| s.validator_address.clone())
                .collect();

            self.process_block(height_to_process, block)
                .await
                .context(format!("Failed to process Sei block {}", height_to_process))?;

            // Buffer signatures for batch write (instead of writing immediately)
            // This significantly improves performance by reducing ClickHouse round-trips
            signature_buffer.push((height_to_process, block_timestamp, block_signatures));

            // Adaptive batching: use larger batches when behind to maximize throughput
            let current_batch_size = calculate_batch_size(current_gap);
            let current_batch_timeout = calculate_batch_timeout(current_gap);

            let should_flush = signature_buffer.len() >= current_batch_size
                || last_flush_time.elapsed().as_millis() as u64 >= current_batch_timeout;

            if should_flush {
                let batch_to_send = signature_buffer.clone();
                if let Err(e) = tx_sender.send(batch_to_send) {
                    warn!(
                        "(Sei Block) Failed to send batch to background writer: {} (channel closed, writing synchronously)",
                        e
                    );
                    // Fallback: write synchronously if channel is closed
                    let storage_arc_clone = storage_arc.clone();
                    let mut storage = storage_arc_clone.lock().await;
                    storage
                        .save_signatures_batch(signature_buffer.clone())
                        .await
                        .context("Failed to batch write Sei signatures (fallback)")?;
                }
                signature_buffer.clear();
                last_flush_time = std::time::Instant::now();
            }

            let process_time = process_start.elapsed();
            if process_time.as_millis() > 3000 {
                warn!(
                    "(Sei Block) Slow block processing for height {}: took {:?} (buffer: {}/{})",
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
                    "(Sei Block) Buffer status: {} blocks (target: {}, gap: {})",
                    buffer_size_after,
                    target_buffer_size,
                    current_gap
                );
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
                            "(Sei Block) Refilling buffer: fetching {} blocks (buffer: {}/{}, gap: {}, memory-optimized: {})",
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
                                    let result = Self::fetch_block(&rpc_clone, height).await;
                                    (height, result)
                                }
                            })
                            .collect();

                        // Execute fetches concurrently (non-blocking - we'll check results on next iteration)
                        let results = future::join_all(fetch_futures).await;

                        // Add successful fetches to buffer
                        for (height, result) in results {
                            match result {
                                Ok(block) => {
                                    let block_height = block
                                        .header
                                        .height
                                        .parse::<usize>()
                                        .context("Could not parse Sei block height")?;

                                    if block_height == height {
                                        let tx_count = block.data.txs.len();
                                        debug!(
                                            "(Sei Block) Buffered block {} with {} transactions",
                                            height,
                                            tx_count
                                        );
                                        block_buffer.insert(height, block);
                                    } else {
                                        warn!(
                                            "(Sei Block) Block height mismatch in buffer: expected {}, got {}",
                                            height,
                                            block_height
                                        );
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
        }

        // Function runs continuously - only returns on error
        // Background task and storage Arc will be cleaned up when function returns (on error)
        // or when the module is dropped (on shutdown)
        // The loop above runs forever, so this return is unreachable (function only returns on error)
        #[allow(unreachable_code)]
        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.rpc.is_none() {
        anyhow::bail!("Config is missing RPC node pool");
    }
    Ok(Box::new(Block::new(app_context)))
}

#[async_trait]
impl RunnableModule for Block {
    async fn run(&mut self) -> anyhow::Result<()> {
        let block_window = self.app_context.config.network.sei.block.window as usize;

        // Process block window with concurrent buffer logic
        self.process_block_window().await?;

        // Uptime metrics (unchanged from original Sei implementation)
        if self
            .app_context
            .config
            .network
            .sei
            .block
            .uptime
            .persistence
        {
            // 1d
            let uptimes = self.signature_storage.uptimes(UptimeWindow::OneDay).await?;
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
            // 7d
            let uptimes = self.signature_storage.uptimes(UptimeWindow::SevenDays).await?;
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
            // 15d
            let uptimes = self.signature_storage.uptimes(UptimeWindow::FifteenDays).await?;
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
            // 30d
            let uptimes = self.signature_storage.uptimes(UptimeWindow::ThirtyDays).await?;
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
            // 6m
            let uptimes = self.signature_storage.uptimes(UptimeWindow::SixMonths).await?;
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
            // In-memory: emit block-window uptime
            let uptimes = self
                .signature_storage
                .uptimes(UptimeWindow::BlockWindow)
                .await?;
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

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.sei.block.interval as u64,
        )
    }

    fn name(&self) -> &'static str {
        "Sei Block"
    }
}
