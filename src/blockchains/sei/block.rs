use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::env;

use anyhow::{bail, Context};
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
    COMETBFT_BLOCK_STUCK_HEIGHT, COMETBFT_BLOCK_STUCK_DURATION_SECONDS, COMETBFT_BLOCK_STUCK_RETRY_COUNT,
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
    COMETBFT_VALIDATOR_MISSED_BLOCKS,
    COMETBFT_VALIDATOR_PROPOSED_BLOCKS, COMETBFT_VALIDATOR_TOTAL_BLOCKS,
};
use crate::blockchains::sei::types::{
    SeiBlock, SeiBlockDirect, SeiBlockResponse, SeiSignature,
};
use crate::core::app_context::AppContext;
use crate::core::block_window::BlockWindow;
use crate::core::clients::http_client::NodePool;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;
use crate::core::utils::{create_error_preview, extract_txs_from_response};
use crate::blockchains::cometbft::block::storage::{
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

    /// Fetch all transactions for a block using paginated tx_search (static version for concurrent fetching)
    /// tx_search has a default limit of 30 transactions per page, so we need to paginate
    async fn fetch_sei_txs_static(
        rpc: &Arc<NodePool>,
        height: usize,
    ) -> Option<Vec<crate::blockchains::sei::types::SeiTx>> {
        const PER_PAGE: usize = 100; // Maximum per_page value for tx_search
        let mut all_txs = Vec::new();
        let mut page = 1;
        let mut total_count: Option<usize> = None;

        loop {
            let tx_path = Path::from(format!(
                r#"tx_search?query="tx.height={}"&page={}&per_page={}"#,
                height, page, PER_PAGE
            ));

        // Tx fetch: use endpoint preference to prioritize nodes with tx_search support
        // NodePool will try preferred nodes first, then fall back to all nodes if they fail
        // This ensures we get tx data when available, but don't block for long if preferred nodes are down
            match rpc.get_with_endpoint_preference(tx_path.clone(), Some("tx_search")).await {
            Ok(res) => {
                    // Try to parse the response
                    match from_str::<crate::blockchains::sei::types::SeiTxResponse>(&res) {
                        Ok(resp) => {
                            // Handle both response formats (with result wrapper or direct)
                            let (page_txs, total_opt) = match resp {
                                crate::blockchains::sei::types::SeiTxResponse::WithResult { ref result } => {
                                    (result.txs.clone(), result.total.as_ref())
                                }
                                crate::blockchains::sei::types::SeiTxResponse::Direct { ref txs, ref total } => {
                                    (txs.clone(), total.as_ref())
                                }
                            };

                            // Get total count from first page
                            if total_count.is_none() {
                                if let Some(total_str) = total_opt {
                                    total_count = total_str.parse::<usize>().ok();
                                    if let Some(total) = total_count {
                                        debug!(
                                            "(Sei Block) tx_search for height {}: total {} transactions, fetching page {}",
                                            height, total, page
                                        );
                                    }
                                }
                            }

                            let page_count = page_txs.len();
                            all_txs.extend(page_txs);

                            debug!(
                                "(Sei Block) tx_search for height {}: page {} returned {} transactions (total fetched: {})",
                                height,
                                page,
                                page_count,
                                all_txs.len()
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
                                            match serde_json::from_value::<Vec<crate::blockchains::sei::types::SeiTx>>(
                                                txs_val.clone(),
                                            ) {
                                                Ok(txs) => {
                                                    all_txs.extend(txs);
                                                    // For fallback parsing, we can't determine total, so stop after first page
                                                    break;
                                                }
                                Err(e) => {
                                                    let preview = create_error_preview(&res, 200);
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
                                        let preview = create_error_preview(&res, 200);
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
                                warn!(
                                    "(Sei Block) Failed to parse tx_search page {} for height {}, stopping pagination",
                                    page,
                                    height
                                );
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
                        warn!(
                            "(Sei Block) Failed to fetch tx_search page {} for height {}: {}, stopping pagination",
                            page,
                            height,
                            e
                        );
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

    async fn get_block(&mut self, height: BlockHeight) -> anyhow::Result<SeiBlock> {
        let path = match height {
            BlockHeight::Height(h) => {
                info!("(Sei Block) Obtaining block with height: {}", h);
                format!("/block?height={}", h)
            }
            BlockHeight::Latest => {
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
            .with_context(|| format!("Could not fetch block {}", path))?;

        // Match CometBFT’s defensive behavior: if body isn't JSON, bail with preview
        if !res.trim_start().starts_with('{') && !res.trim_start().starts_with('[') {
            let preview = create_error_preview(&res, 200);
            anyhow::bail!(
                "Block response for {} is not JSON (status was 200 but body is not JSON). Preview: {}",
                match height {
                    BlockHeight::Height(h) => format!("height {}", h),
                    BlockHeight::Latest => "latest block".to_string(),
                },
                preview
            );
        }

        // Be tolerant: parse as generic JSON first, then extract block (object or JSON string)
        let v: serde_json::Value = serde_json::from_str(&res)
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
            })?;

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
        txs_info: Option<Vec<crate::blockchains::sei::types::SeiTx>>,
        current_gap: usize,
    ) -> anyhow::Result<()> {
        let block_height = block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse block height")?;
        if block_height != height {
            anyhow::bail!(
                "Block height mismatch in Sei process_block: expected {}, got {}",
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

        // CATCH-UP MODE OPTIMIZATION: When far behind (gap > catchup_mode_threshold), defer non-critical metric updates
        // This significantly speeds up processing during catch-up (learned from CometBFT being 30x faster)
        // - Skip validator metrics (most expensive: ~86 metric updates per block)
        // - Only update essential metrics (gap, current height) every block
        // - Update all metrics periodically (every 1000 blocks) to maintain accuracy
        let catchup_mode_threshold = self.app_context.config.network.sei.block.catchup_mode_threshold;
        const METRIC_UPDATE_INTERVAL: usize = 1000; // Update all metrics every 1000 blocks in catch-up mode (aggressive optimization for faster catch-up)
        let is_catchup_mode = current_gap > catchup_mode_threshold;
        let should_update_all_metrics = !is_catchup_mode || (height % METRIC_UPDATE_INTERVAL == 0);

        let block_proposer = block.header.proposer_address.clone();
        let block_signatures: Vec<SeiSignature> = block.last_commit.signatures.clone();
        let validator_alert_addresses = self.app_context.config.general.alerting.validators.clone();

        // Count transactions from block.data.txs (authoritative source, even if tx_search fails)
        let tx_count = block.data.txs.len();

        // Validate transaction count is sane (data integrity check)
        if tx_count > 10000 {
            warn!(
                "(Sei Block) Block {} has unusually high transaction count: {} (possible data corruption?)",
                height,
                tx_count
            );
        }

        // tx count - only update in catch-up mode if should_update_all_metrics
        if should_update_all_metrics {
        COMETBFT_BLOCK_TXS
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
                .set(tx_count as f64);
        }

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

            // Use tx data that was fetched concurrently with the block (if available)
            if let Some(txs_info) = txs_info.as_ref() {
                // Data integrity: tx_search may return fewer txs (indexing gaps), but we should log it.
                if txs_info.len() != tx_count {
                    warn!(
                        "(Sei Block) Block {} tx count mismatch: block.data.txs has {}, tx_search returned {} (some transactions may not be indexed or tx_search failed partially)",
                        height,
                        tx_count,
                        txs_info.len()
                    );
                }
                    let mut gas_wanted = Vec::new();
                    let mut gas_used = Vec::new();

                for tx in txs_info.iter() {
                    if let Some(result) = tx.tx_result.as_ref() {
                        if let Some(gw) = result.gas_wanted.as_ref() {
                            if let Ok(v) = gw.parse::<usize>() {
                                gas_wanted.push(v);
                            }
                        }
                        if let Some(gu) = result.gas_used.as_ref() {
                            if let Ok(v) = gu.parse::<usize>() {
                                gas_used.push(v);
                            }
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
            } else {
                // tx_search failed or returned no results, but block has transactions
                warn!(
                    "(Sei Block) Block {} has {} transactions in block.data.txs but tx_search returned no data (tx indexing may be disabled or tx_search failed)",
                    height,
                    tx_count
                );
            }
        }

        // Match CometBFT behavior: TX_SIZE is always set (even in catch-up mode)
        COMETBFT_BLOCK_TX_SIZE
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(block_avg_tx_size);

        // Set gas metrics only if tx.enabled is true in config
        // In catch-up mode, only update periodically to speed up processing
        if self.app_context.config.network.sei.block.tx.enabled && should_update_all_metrics {
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

        // Log transaction count at INFO level (match CometBFT style, including tx_search status)
        if self.app_context.config.network.sei.block.tx.enabled {
            let tx_status_msg = if txs_info.is_some() {
                format!(", tx_search returned {} transactions", txs_info.as_ref().unwrap().len())
            } else {
                " (tx_search unavailable or failed)".to_string()
            };
            info!(
                "(Sei Block) Processing block {}: {} transactions in block.data.txs{}",
                height,
                tx_count,
                tx_status_msg
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

        // Always update proposer metric (cheap, single update)
        if should_update_all_metrics {
        COMETBFT_VALIDATOR_PROPOSED_BLOCKS
            .with_label_values(&[
                &block_proposer,
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
                &validator_alert_addresses.contains(&block_proposer).to_string(),
            ])
            .inc();
        }

        // Always update validator metrics if not in catch-up mode, or periodically in catch-up mode
        if should_update_all_metrics {
            // OPTIMIZATION: Pre-compute alert flags and use HashSet for faster lookups
            // This reduces string allocations and improves performance when processing many validators
            let validator_alert_set: std::collections::HashSet<&String> = validator_alert_addresses.iter().collect();
            let block_signature_addresses: std::collections::HashSet<&str> = block_signatures
            .iter()
                .map(|sig| sig.validator_address.as_str())
            .collect();

            // Increment total blocks counter for all validators in active set
            // This represents total opportunities to sign (whether they signed or not)
            // OPTIMIZATION: Pre-compute fires_alerts string once per validator to avoid repeated allocations
            for validator in &self.validators {
                let fires_alerts = validator_alert_set.contains(validator).to_string();

                COMETBFT_VALIDATOR_TOTAL_BLOCKS
                    .with_label_values(&[
                        validator,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        &fires_alerts,
                    ])
                    .inc();

                // Check if validator missed this block (faster lookup with HashSet)
                if !block_signature_addresses.contains(validator.as_str()) {
            COMETBFT_VALIDATOR_MISSED_BLOCKS
                .with_label_values(&[
                            validator,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                    &fires_alerts,
                ])
                .inc();
                }
            }
        }

        // Set current block height and time metrics (match CometBFT’s safety checks)
        let block_height_i64: i64 = block_height
            .try_into()
            .context("Failed to parse block height to i64")?;
        let block_time_timestamp = block_time.and_utc().timestamp() as f64;

        // Validate block time is reasonable (data integrity check)
        let now = chrono::Utc::now().timestamp();
        let one_year_ago = now - (365 * 24 * 3600);
        if block_time_timestamp > (now + 3600) as f64 {
            warn!(
                "(Sei Block) Block {} has block time in the future: {} (current: {}, difference: {}s). Possible clock skew or data corruption.",
                height,
                block_time_timestamp,
                now,
                block_time_timestamp as i64 - now
            );
        } else if block_time_timestamp < one_year_ago as f64 {
            warn!(
                "(Sei Block) Block {} has extremely old block time: {} (current: {}, difference: {}s). Possible data corruption.",
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

    /// Fetch block and tx data concurrently (if tx.enabled)
    /// This matches CometBFT's fetch_block_data pattern for maximum performance
    async fn fetch_block_data(
        rpc: &Arc<NodePool>,
        tx_enabled: bool,
        height: usize,
    ) -> anyhow::Result<(SeiBlock, Option<Vec<crate::blockchains::sei::types::SeiTx>>)> {
        let rpc = rpc.clone();
        let block_path = format!("/block?height={}", height);

        info!("(Sei Block) Obtaining block with height: {}", height);

        if tx_enabled {
            // Fetch both block and tx data concurrently for maximum performance
            let (block_result, tx_result) = tokio::join!(
                async {
                    let res = rpc
                        .get(Path::from(block_path.clone()))
                        .await
                        .context(format!("Could not fetch Sei block {}", block_path))?;

                    // Match CometBFT’s defensive behavior: if body isn't JSON, bail with preview
                    if !res.trim_start().starts_with('{') && !res.trim_start().starts_with('[') {
                        let preview = create_error_preview(&res, 200);
                        anyhow::bail!(
                            "Block response for height {} is not JSON (status was 200 but body is not JSON). Preview: {}",
                            height,
                            preview
                        );
                    }

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
                },
                async {
                    // Fetch tx data concurrently
                    Self::fetch_sei_txs_static(&rpc, height).await
                }
            );

            let block = block_result?;

            // Verify block height matches
            let fetched_block_height = block.header.height.parse::<usize>().unwrap_or(0);
            if fetched_block_height != height {
                anyhow::bail!(
                    "CRITICAL: Fetched Sei block height {} does not match requested height {}",
                    fetched_block_height,
                    height
                );
            }

            Ok((block, tx_result))
        } else {
            // Non-tx mode: only fetch block
            let block = Self::fetch_block_static(&rpc, height).await?;
            Ok((block, None))
        }
    }

    /// Fetch a Sei block by height (static version for concurrent fetching)
    async fn fetch_block_static(
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
        // Retry fetching latest block until successful (NodePool already retries, but we add extra resilience)
        let last_block = loop {
            match self.get_block(BlockHeight::Latest).await {
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

        // Match CometBFT startup semantics:
        // - If persistence enabled and no previous height, start from latest-1 (caught up immediately)
        // - Otherwise, start from window-back
        let mut height_to_process = if self
            .app_context
            .config
            .network
            .sei
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
            "(Sei Block) Starting from height: {} (will process continuously until caught up)",
            height_to_process
        );

        // Buffer & concurrency logic (mirrors CometBFT semantics, without tx_search)
        let rpc = self.app_context.rpc.as_ref().unwrap().clone();
        let catchup_mode_threshold = self.app_context.config.network.sei.block.catchup_mode_threshold;
        let tx_enabled = self.app_context.config.network.sei.block.tx.enabled;

        // Buffer of fetched blocks keyed by height (stores block and optional tx data)
        let mut block_buffer: BTreeMap<usize, (SeiBlock, Option<Vec<crate::blockchains::sei::types::SeiTx>>)> = BTreeMap::new();
        let concurrent_fetch_count = self.app_context.config.network.sei.block.concurrency;
        const BATCHING_GAP_THRESHOLD: usize = 50;
        const MIN_BUFFER_SIZE: usize = 2;

        let calculate_target_buffer_size = |gap: usize| -> usize {
            if gap <= BATCHING_GAP_THRESHOLD {
                // Caught up: use smaller buffer to save memory (large blocks can be 5-10MB each)
                MIN_BUFFER_SIZE.max(concurrent_fetch_count.min(5))
            } else {
                // Behind: use larger buffer to maximize concurrent fetching.
                // Allow buffer to be 2x concurrency for better pipelining during catch-up
                // This ensures we always have blocks ready while processing continues
                let buffer_size = concurrent_fetch_count * 2;
                buffer_size
                    .max(concurrent_fetch_count)
                    .max(MIN_BUFFER_SIZE)
            }
        };

        // Store initial height to calculate blocks_processed correctly
        let initial_height_to_process = height_to_process;

        // Track stuck state for metrics (when we're retrying the same block)
        let mut stuck_block_height: Option<usize> = None;
        let mut stuck_start_time: Option<std::time::Instant> = None;
        let mut stuck_retry_count: u32 = 0;

        // Uptime calculations run periodically (every 1000 blocks) instead of at the end
        // This allows the function to run continuously while still updating uptime metrics
        let mut last_uptime_calc_height = height_to_process;
        const UPTIME_CALC_INTERVAL: usize = 1000;

        // Tip refresh interval: how many processed blocks between /block?latest calls.
        // We keep this reasonably high to avoid hammering RPC but still track progress.
        // During catch-up (large gap), refresh less frequently to reduce RPC load
        const TIP_REFRESH_BLOCKS_NORMAL: usize = 100; // When caught up or close
        const TIP_REFRESH_BLOCKS_CATCHUP: usize = 500; // When far behind (gap > catchup_mode_threshold)
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
        // When behind (gap > catchup_mode_threshold), use larger batches (2-3x) to maximize throughput
        // When caught up (gap <= catchup_mode_threshold), use base batch size for lower latency
        let calculate_batch_size = |gap: usize| -> usize {
            if gap > catchup_mode_threshold {
                base_batch_size * 10 // 10x batch size when far behind (maximize throughput for fast catch-up)
            } else {
                base_batch_size
            }
        };
        // Adaptive timeout: longer when behind (allows larger batches), shorter when caught up
        let calculate_batch_timeout = |gap: usize| -> u64 {
            if gap > catchup_mode_threshold {
                10_000 // 10 seconds when behind (allows very large batches for max throughput)
            } else {
                2_000 // 2 seconds when caught up (lower latency)
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

        // Spawn background task to handle ClickHouse writes asynchronously
        // Task runs for the entire lifetime of the function (forever)
        // Retry logic is handled inside save_signatures_batch, so we log final failures here
        let _bg_task = tokio::spawn(async move {
            while let Some(batch) = tx_receiver.recv().await {
                let mut storage = storage_for_bg.lock().await;
                if let Err(e) = storage.save_signatures_batch(batch.clone()).await {
                    // This error means all retries were exhausted - data is permanently lost
                    // Log as critical error since this is a data integrity issue
                    error!(
                        "(Sei Block) CRITICAL: Background ClickHouse write failed after all retries for chain {} (blocks {}-{}): {}. Data may be permanently lost!",
                        chain_id_clone,
                        batch.first().map(|(h, _, _)| h).unwrap_or(&0),
                        batch.last().map(|(h, _, _)| h).unwrap_or(&0),
                        e
                    );
                    // Continue processing - errors are logged but don't stop the background task
                    // The exporter will continue from the last successfully persisted height on restart
                }
            }
            debug!("(Sei Block) Background ClickHouse writer task exiting (channel closed)");
        });

        // Continuously process blocks until we're caught up to the chain tip.
        // We refresh the current tip periodically based on TIP_REFRESH_BLOCKS.
        loop {
            // Calculate current gap to determine refresh interval
            let current_gap = current_chain_tip.saturating_sub(height_to_process);
            let tip_refresh_blocks = if current_gap > catchup_mode_threshold {
                TIP_REFRESH_BLOCKS_CATCHUP
            } else {
                TIP_REFRESH_BLOCKS_NORMAL
            };

            // Refresh current chain tip periodically based on how many blocks we've processed
            let blocks_since_refresh = height_to_process.saturating_sub(last_tip_refresh_height);
            if blocks_since_refresh >= tip_refresh_blocks {
                match self.get_block(BlockHeight::Latest).await {
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

            if current_gap > 100 {
                warn!(
                    "(Sei Block) Exporter is {} blocks behind chain tip (chain: {}, processed: {})",
                    current_gap,
                    current_chain_tip,
                    height_to_process.saturating_sub(1)
                );
            }

            // Only use concurrency when we're more than 1 block behind
            // When caught up (gap <= 1), sequential fetching is sufficient and avoids unnecessary complexity
            // Concurrency is most useful when we need to catch up quickly (gap > 1)
            let use_concurrency = concurrent_fetch_count > 1 && current_gap > 1;
            let target_buffer_size = calculate_target_buffer_size(current_gap);

            // Match CometBFT’s buffer fill behavior: continuously try to fill up to target size,
            // but break out if we’re not making progress (avoid infinite loops).
            const MAX_INITIAL_FILL_ATTEMPTS: usize = 10;
            let mut initial_fill_attempts = 0;
            while use_concurrency
                && block_buffer.len() < target_buffer_size
                && height_to_process + block_buffer.len() < current_chain_tip
                && initial_fill_attempts < MAX_INITIAL_FILL_ATTEMPTS
            {
                let remaining =
                    current_chain_tip.saturating_sub(height_to_process + block_buffer.len());
                let needed = target_buffer_size.saturating_sub(block_buffer.len());
                let fetch_count = needed.min(concurrent_fetch_count).min(remaining);
                if fetch_count == 0 {
                    break;
                }

                let buffer_size_before_fetch = block_buffer.len();
                initial_fill_attempts += 1;

                info!(
                    "(Sei Block) Concurrent fetch: fetching {} blocks (buffer: {}/{}, gap: {}, memory-optimized: {})",
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
                                    .context("Could not parse Sei block height")?;

                                if block_height == height {
                                    // Validate transaction count for data integrity
                                    let tx_count = block.data.txs.len();
                                    debug!(
                                        "(Sei Block) Buffered block {} (tx_enabled) with {} transactions",
                                        height,
                                        tx_count
                                    );
                                    block_buffer.insert(height, (block, txs_info));
                                } else {
                                    warn!(
                                        "(Sei Block) Block height mismatch in buffer: expected {}, got {}",
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
                                        "(Sei Block) Concurrent fetch failed for height {}: All RPC nodes are unhealthy (will retry on fallback)",
                                        height
                                    );
                                } else {
                                    warn!(
                                        "(Sei Block) Concurrent fetch failed for height {}: {} (will retry on fallback)",
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
                                    .context("Could not parse Sei block height")?;

                                if block_height == height {
                                    // Validate transaction count for data integrity
                                    let tx_count = block.data.txs.len();
                                    debug!(
                                        "(Sei Block) Buffered block {} (non-tx mode) with {} transactions",
                                        height,
                                        tx_count
                                    );
                                    block_buffer.insert(height, (block, txs_info));
                                } else {
                                    warn!(
                                        "(Sei Block) Block height mismatch in buffer: expected {}, got {}",
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
                                        "(Sei Block) Concurrent fetch failed for height {}: All RPC nodes are unhealthy (will retry on fallback)",
                                        height
                                    );
                                } else {
                                    warn!(
                                        "(Sei Block) Concurrent fetch failed for height {}: {} (will retry on fallback)",
                                        height,
                                        e
                                    );
                                }
                                // Don't add to buffer - will be fetched on fallback with retry logic
                            }
                        }
                    }
                }

                if block_buffer.len() == buffer_size_before_fetch {
                    debug!(
                        "(Sei Block) Initial buffer fill stuck: buffer size unchanged after fetch attempt {} (buffer: {}/{})",
                        initial_fill_attempts,
                        block_buffer.len(),
                        target_buffer_size
                    );
                    if initial_fill_attempts >= 3 {
                        warn!(
                            "(Sei Block) Initial buffer fill: {} blocks in buffer (target: {}). Some blocks failed to fetch - will process sequentially with retry logic",
                            block_buffer.len(),
                            target_buffer_size
                        );
                        break;
                    }
                }
            }

            // Get next block from buffer or fetch on-demand (fallback)
            let buffer_size_before = block_buffer.len();
            let (block, txs_info, block_from_buffer) =
                if let Some(data) = block_buffer.remove(&height_to_process) {
                // Successfully got block from buffer - clear stuck state if we were stuck on a different block
                if stuck_block_height != Some(height_to_process) && stuck_block_height.is_some() {
                    // We were stuck on a different block, clear stuck metrics
                    COMETBFT_BLOCK_STUCK_HEIGHT
                        .with_label_values(&[
                            &self.app_context.chain_id,
                            &self.app_context.config.general.network,
                        ])
                        .set(0);
                    COMETBFT_BLOCK_STUCK_DURATION_SECONDS
                        .with_label_values(&[
                            &self.app_context.chain_id,
                            &self.app_context.config.general.network,
                        ])
                        .set(0.0);
                    COMETBFT_BLOCK_STUCK_RETRY_COUNT
                        .with_label_values(&[
                            &self.app_context.chain_id,
                            &self.app_context.config.general.network,
                        ])
                        .set(0);
                    stuck_block_height = None;
                    stuck_start_time = None;
                    stuck_retry_count = 0;
                }
                let buffered_height = data.0.header.height.parse::<usize>().unwrap_or(0);
                let buffered_tx_count = data.0.data.txs.len();

                if buffered_height != height_to_process {
                    error!(
                        "(Sei Block) CRITICAL: Block height mismatch in buffer remove: expected {}, got {}",
            height_to_process,
                        buffered_height
                    );
                    anyhow::bail!(
                        "Block height mismatch: expected {}, got {}",
                        height_to_process,
                        buffered_height
                    );
                }

                debug!(
                    "(Sei Block) Processing buffered block {}: height={}, txs={}, txs_info={}, buffer_size={}/{}",
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

                (data.0, data.1, true)
            } else {
                let current_gap = current_chain_tip.saturating_sub(height_to_process);
                let blocks_processed =
                    height_to_process.saturating_sub(initial_height_to_process);
                // Only warn if concurrency was enabled but failed; otherwise use debug since this is expected
                if use_concurrency {
                    warn!(
                        "(Sei Block) Buffer miss for height {} (concurrent fetch failed), fetching with retry logic. Progress: {} blocks processed, buffer_size={}/{}",
                        height_to_process,
                        blocks_processed,
                        buffer_size_before,
                        target_buffer_size
                    );
                } else {
                    debug!(
                        "(Sei Block) Buffer miss for height {} (concurrency disabled, gap={}), fetching sequentially. Progress: {} blocks processed, buffer_size={}/{}",
                        height_to_process,
                        current_gap,
                        blocks_processed,
                        buffer_size_before,
                        target_buffer_size
                    );
                }

                // Retry logic with exponential backoff - NEVER skip blocks, keep retrying indefinitely
                // NodePool tries multiple nodes per call, so we get good coverage across RPC endpoints
                // We track "stuck" state to provide visibility when a block is difficult to fetch
                let mut retries = 0u32;
                const INITIAL_RETRY_DELAY_MS: u64 = 1000; // Start with 1s delay
                const MAX_RETRY_DELAY_MS: u64 = 30000; // Cap at 30s delay (longer for persistent issues)
                const STUCK_THRESHOLD_RETRIES: u32 = 10; // Consider "stuck" after 10 retries

                // Track if we're stuck on this block (for metrics)
                // Reset stuck tracking if we're starting to retry a new block
                if stuck_block_height != Some(height_to_process) {
                    // Starting to retry a new block - reset stuck tracking
                    *stuck_block_height.get_or_insert_with(|| height_to_process) = height_to_process;
                    *stuck_start_time.get_or_insert_with(|| std::time::Instant::now()) = std::time::Instant::now();
                    stuck_retry_count = 0;
                }

                loop {
                    let result = if tx_enabled {
                        Self::fetch_block_data(&rpc, tx_enabled, height_to_process).await
                    } else {
                        match self.get_block(BlockHeight::Height(height_to_process)).await {
                            Ok(block) => Ok((block, None)),
                            Err(e) => Err(e),
                        }
                    };
                    match result {
                        Ok((block, txs_info)) => {
                            // Successfully fetched - clear stuck state and metrics
                            if retries > 0 {
                                info!(
                                    "(Sei Block) Successfully fetched block {} after {} retries{}",
                                    height_to_process,
                                    retries,
                                    if stuck_retry_count > 0 {
                                        format!(" (was stuck for {:.1}s)",
                                            stuck_start_time.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0))
                                    } else {
                                        String::new()
                                    }
                                );
                            }

                            // Clear stuck metrics
                            COMETBFT_BLOCK_STUCK_HEIGHT
                                .with_label_values(&[
                                    &self.app_context.chain_id,
                                    &self.app_context.config.general.network,
                                ])
                                .set(0);
                            COMETBFT_BLOCK_STUCK_DURATION_SECONDS
                                .with_label_values(&[
                                    &self.app_context.chain_id,
                                    &self.app_context.config.general.network,
                                ])
                                .set(0.0);
                            COMETBFT_BLOCK_STUCK_RETRY_COUNT
                                .with_label_values(&[
                                    &self.app_context.chain_id,
                                    &self.app_context.config.general.network,
                                ])
                                .set(0);

                            stuck_block_height = None;
                            stuck_start_time = None;
                            stuck_retry_count = 0;

                            break (block, txs_info, false);
                        }
                        Err(e) => {
                            retries += 1;
                            stuck_retry_count = retries;

                            // Check if this is a timeout error (large blocks can timeout)
                            let is_timeout = e
                                .chain()
                                .any(|err| {
                                    let err_str = err.to_string().to_lowercase();
                                    err_str.contains("timeout")
                                        || err_str.contains("deadline")
                                        || err_str.contains("timed out")
                                });

                            // Update stuck metrics if we're past the threshold
                            if retries >= STUCK_THRESHOLD_RETRIES {
                                if let Some(stuck_start) = stuck_start_time {
                                    let stuck_duration = stuck_start.elapsed().as_secs_f64();

                                    COMETBFT_BLOCK_STUCK_HEIGHT
                                        .with_label_values(&[
                                            &self.app_context.chain_id,
                                            &self.app_context.config.general.network,
                                        ])
                                        .set(height_to_process as i64);
                                    COMETBFT_BLOCK_STUCK_DURATION_SECONDS
                                        .with_label_values(&[
                                            &self.app_context.chain_id,
                                            &self.app_context.config.general.network,
                                        ])
                                        .set(stuck_duration);
                                    COMETBFT_BLOCK_STUCK_RETRY_COUNT
                                        .with_label_values(&[
                                            &self.app_context.chain_id,
                                            &self.app_context.config.general.network,
                                        ])
                                        .set(retries as i64);

                                    // Log periodically (every 10 retries) to avoid spam
                                    if retries % 10 == 0 {
                                        warn!(
                                            "(Sei Block) Stuck on block {}: {} retries, stuck for {:.1}s (timeout: {}). Last error: {}",
                                            height_to_process,
                                            retries,
                                            stuck_duration,
                                            is_timeout,
                                            e
                                        );
                                    }
                                }
                            }

                            // Exponential backoff: longer delays for timeouts, shorter for other errors
                            // Cap at MAX_RETRY_DELAY_MS to avoid extremely long waits
                            let delay_ms = if is_timeout {
                                // For timeouts: exponential backoff (1s, 2s, 4s, 8s, 16s, 30s max)
                                let exponential = INITIAL_RETRY_DELAY_MS * (1 << (retries.saturating_sub(1).min(4)));
                                exponential.min(MAX_RETRY_DELAY_MS)
                            } else {
                                // For other errors: linear backoff (1s, 2s, 3s, 4s, ... up to 30s max)
                                (INITIAL_RETRY_DELAY_MS * retries as u64).min(MAX_RETRY_DELAY_MS)
                            };

                            // Log retry attempts (less frequently as retries increase to avoid spam)
                            if retries <= 5 || retries % 10 == 0 {
                                warn!(
                                    "(Sei Block) Retry {} for height {} (timeout: {}): {}. Waiting {}ms before retry...",
                                    retries,
                                    height_to_process,
                                    is_timeout,
                                    e,
                                    delay_ms
                                );
                            }
                            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
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

            self.process_block(height_to_process, block, txs_info, current_gap)
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

            // Calculate uptime metrics periodically (every 1000 blocks) instead of at the end
            // This allows continuous processing while still updating uptime metrics
            let blocks_since_uptime_calc = height_to_process.saturating_sub(last_uptime_calc_height);
            if blocks_since_uptime_calc >= UPTIME_CALC_INTERVAL {
                last_uptime_calc_height = height_to_process;

        if self
            .app_context
            .config
            .network
            .sei
            .block
            .uptime
            .persistence
        {
                    let storage_arc_clone = storage_arc.clone();
                    let storage = storage_arc_clone.lock().await;
                    let uptimes = storage.uptimes(UptimeWindow::OneDay).await?;
                    info!("(Sei Block) Calculating 1 day uptime for validators");
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
                    info!("(Sei Block) Calculating 7 days uptime for validators");
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
                    info!("(Sei Block) Calculating 15 days uptime for validators");
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
                    info!("(Sei Block) Calculating 30 days uptime for validators");
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
                    info!("(Sei Block) Calculating 6 months uptime for validators");
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
                }
            }

            // Match CometBFT: only refill if we successfully processed from buffer.
            // If we had a buffer miss (processed sequentially), don’t refill to avoid infinite loops
            // hammering the same failing blocks.
            if use_concurrency && block_buffer.len() < target_buffer_size && block_from_buffer {
                let remaining = current_chain_tip.saturating_sub(height_to_process + block_buffer.len());
                if remaining > 0 {
                    // Calculate how many we need to reach target
                    let needed = target_buffer_size.saturating_sub(block_buffer.len());
                    // When behind (gap > threshold), fetch more aggressively to maximize throughput
                    // OPTIMIZATION: Fetch many blocks in parallel to keep buffer full, even if processing is slower
                    // This creates a pipeline: while processing block N, we fetch blocks N+100 to N+120
                    // This way, RPC latency doesn't block processing - we always have blocks ready
                    let aggressive_fetch = if current_gap > BATCHING_GAP_THRESHOLD {
                        // Behind: fetch aggressively to keep buffer full
                        // Strategy: Fetch enough blocks to maintain a "lookahead buffer"
                        // If buffer is 99/100, fetch 10-20 blocks to keep it full while processing continues
                        // This ensures RPC requests are pipelined ahead of processing
                        let buffer_space = target_buffer_size.saturating_sub(block_buffer.len());
                        // Fetch a "lookahead" amount: more aggressive for faster catch-up
                        // Increased to 50-150 blocks ahead for better pipelining
                        let lookahead_fetch = (concurrent_fetch_count / 2).max(50).min(150); // Fetch 50-150 blocks ahead
                        let fetch_target = buffer_space.max(lookahead_fetch); // At least fill buffer, ideally fetch ahead
                        fetch_target.min(concurrent_fetch_count).min(remaining)
                    } else {
                        // Caught up: just fetch what's needed (memory optimization)
                        needed.min(concurrent_fetch_count).min(remaining)
                    };
                    let fetch_count = aggressive_fetch;
                    // Only refill if we have room AND we're not stuck on the same failing blocks
                    // Check: if buffer hasn't grown in the last iteration, skip refill to avoid infinite loop
                    // The sequential fallback will handle missing blocks
                    if fetch_count > 0 && block_buffer.len() > 0 {
                        // Only refill if we have blocks in buffer (means we're making progress)
                        // If buffer is empty, let the sequential fallback handle it
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
                                        block_buffer.insert(height, (block, txs_info));
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

            // Match CometBFT: periodic debug progress line (helps track catch-up rate vs gap)
            if height_to_process % 10 == 0 {
                let blocks_processed = height_to_process.saturating_sub(initial_height_to_process);
                debug!(
                    "(Sei Block) Processed {} blocks, {} remaining (gap: {}, chain_tip: {})",
                    blocks_processed,
                    current_gap,
                    current_gap,
                    current_chain_tip
                );
            }
        }

        // Function runs continuously - only returns on error
        //
        // BACKGROUND TASK CLEANUP:
        // When the function returns (on error via ?), the following happens:
        // 1. tx_sender is dropped → unbounded channel closes immediately
        // 2. Background task's tx_receiver.recv() returns None (channel closed)
        // 3. The while loop in the background task exits
        // 4. The background task completes and exits cleanly
        // 5. bg_task handle is dropped → task becomes "detached" but is already completed
        //
        // IMPORTANT: The background task exits cleanly and does NOT leave a shadow process because:
        // - When the channel closes, recv() returns None immediately (no blocking wait)
        // - The task's while loop exits, and the async block completes
        // - The task finishes execution and is cleaned up by Tokio's runtime
        // - No infinite loops or blocking operations keep it alive
        //
        // The task handle (bg_task) is intentionally not awaited because:
        // - The loop only exits on error (via ?), which propagates immediately
        // - Waiting for bg_task.await would block error propagation unnecessarily
        // - The task exits naturally and quickly when the channel closes
        // - Any in-flight batch writes will complete before the task exits (they're already in progress)
        //
        // NOTE: If you need to guarantee all buffered batches are flushed before shutdown,
        // you would need to restructure to catch errors, flush, close channel, wait for task,
        // then return error. This is a larger refactor and may not be necessary since:
        // - The task processes batches quickly (they're already in the channel)
        // - The exporter resumes from last persisted height on restart anyway
        // - The task exits cleanly when the channel closes
        //
        // The loop above runs forever, so this return is unreachable (function only returns on error)
        #[allow(unreachable_code)]
        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.rpc.is_none() {
        bail!("RPC pool is empty");
    }
    if app_context.config.network.sei.block.tx.enabled {
        info!("\t✅ Sei Block tx is enabled");
    } else {
        info!("\t❌ Sei Block tx is disabled");
    }

    if app_context.config.network.sei.block.uptime.persistence {
        info!("\t✅ Sei Block persistence is enabled");
    } else {
        info!("\t❌ Sei Block persistence is disabled");
        info!(
            "\t\t Sei Block window configured to {}",
            app_context.config.network.sei.block.window
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
        "Sei Block"
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.sei.block.interval as u64,
        )
    }
}
