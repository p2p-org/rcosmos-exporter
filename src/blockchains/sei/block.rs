use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use serde_json::from_str;
use tracing::info;

use crate::blockchains::sei::metrics::{COMETBFT_BLOCK_TXS, COMETBFT_CURRENT_BLOCK_HEIGHT};
use crate::blockchains::cometbft::metrics::{
    COMETBFT_BLOCK_GAS_USED, COMETBFT_BLOCK_GAS_WANTED, COMETBFT_BLOCK_TX_GAS_USED,
    COMETBFT_BLOCK_TX_GAS_WANTED, COMETBFT_BLOCK_TX_SIZE, COMETBFT_CURRENT_BLOCK_TIME,
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
use crate::blockchains::sei::types::{
    SeiBlock, SeiBlockDirect, SeiBlockResponse, SeiSignature, SeiTxResponse,
};
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;
use crate::blockchains::cometbft::block::storage::{
    ClickhouseSignatureStorage, InMemorySignatureStorage, SignatureStorage, UptimeWindow,
};

pub struct Block {
    pub app_context: Arc<AppContext>,
    validators: Vec<String>,
    signature_storage: Box<dyn SignatureStorage>,
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
            })
        } else {
            Box::new(InMemorySignatureStorage {
                block_window: crate::core::block_window::BlockWindow::new(
                    app_context.config.network.sei.block.window as usize,
                ),
                processed_height: 0,
            })
        };
        Self { app_context, validators: Vec::new(), signature_storage }
    }

    async fn fetch_sei_txs(&self, height: usize) -> anyhow::Result<Vec<crate::blockchains::sei::types::SeiTx>> {
        let path = format!("/tx_search?query=\"tx.height={}\"", height);
        let client = self.app_context.rpc.as_ref().unwrap();

        let res = client
            .get(Path::from(path))
            .await
            .context(format!("Could not fetch txs for height {}", height))?;

        // Handle both shapes: {"result":{"txs":[...]}} and {"txs":[...]}
        let v: serde_json::Value = serde_json::from_str(&res)
            .context("Could not deserialize Sei txs response")?;
        let txs_val_opt = v
            .get("result")
            .and_then(|r| r.get("txs"))
            .or_else(|| v.get("txs"));
        if let Some(txs_val) = txs_val_opt {
            let txs: Vec<crate::blockchains::sei::types::SeiTx> = serde_json::from_value(txs_val.clone())
                .context("Could not decode Sei txs array")?;
            return Ok(txs);
        }

        // Fallback to strict struct parsing
        if let Ok(resp) = from_str::<SeiTxResponse>(&res) {
            return Ok(resp.result.txs);
        }
        // No txs found; treat as empty list rather than erroring
        Ok(Vec::new())
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

    async fn process_block(&mut self, height: usize) -> anyhow::Result<()> {
        let block = self.get_block(Some(height)).await?;
        let block_height = block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse block height")?;

        let block_time = block.header.time;
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

            if self.app_context.config.network.sei.block.tx.enabled {
                let txs_info = self
                    .fetch_sei_txs(height)
                    .await
                    .context(format!("Could not obtain txs info from block {}", height))?;

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
                    block_avg_tx_gas_wanted = gas_wanted.iter().sum::<usize>() as f64 / gas_wanted.len() as f64;
                    block_avg_tx_gas_used = gas_used.iter().sum::<usize>() as f64 / gas_used.len() as f64;
                }
            }
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

        // track validators seen and proposed/missed
        for sig in block_signatures.iter() {
            if !sig.validator_address.is_empty() && !self.validators.contains(&sig.validator_address) {
                self.validators.push(sig.validator_address.clone());
                info!("(Sei Block) Tracking validator {}", sig.validator_address);
            }
        }

        self.signature_storage
            .save_signatures(
                block_height,
                block.header.time,
                block_signatures
                    .iter()
                    .map(|s| s.validator_address.clone())
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
        // Process a rolling window similar to CometBFT
        let last_block = self.get_block(None).await.context("Could not obtain last block")?;
        let last_block_height = last_block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse last block height")?;
        let block_window = self.app_context.config.network.sei.block.window as usize;

        let mut height_to_process = self
            .signature_storage
            .get_last_processed_height()
            .await?
            .unwrap_or(0)
            + 1;
        if height_to_process <= 1 {
            height_to_process = last_block_height.saturating_sub(block_window);
        }

        info!(
            "(Sei Block) Starting from height: {} up to latest block: {}",
            height_to_process,
            last_block_height - 1
        );
        while height_to_process < last_block_height {
            self.process_block(height_to_process)
                .await
                .context(format!("Failed to process block {}", height_to_process))?;
            height_to_process += 1;
        }
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
