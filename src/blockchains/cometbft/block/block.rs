use anyhow::{bail, Context};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use serde_json::from_str;
use std::env;
use std::sync::Arc;
use tracing::info;

use crate::blockchains::cometbft::metrics::{
    COMETBFT_BLOCK_GAS_USED, COMETBFT_BLOCK_GAS_WANTED, COMETBFT_BLOCK_TXS,
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

    async fn get_block_txs(&mut self, height: usize) -> anyhow::Result<Vec<Tx>> {
        let res = self
            .app_context
            .rpc
            .as_ref()
            .unwrap()
            .get(Path::from(format!(
                "tx_search?query=\"tx.height={}\"",
                height
            )))
            .await
            .context(format!("Could not fetch txs for height {}", height))?;

        Ok(from_str::<TxResponse>(&res)
            .context("Could not deserialize txs response")?
            .result
            .txs)
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
            .context("Could not deserialize block response")?
            .result
            .block)
    }

    async fn process_block_window(&mut self) -> anyhow::Result<()> {
        let last_block = self
            .get_block(BlockHeight::Latest)
            .await
            .context("Could not obtain last block")?;
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

    async fn process_block(&mut self, height: usize) -> anyhow::Result<()> {
        let block = self
            .get_block(BlockHeight::Height(height))
            .await
            .context(format!("Could not obtain block {}", height))?;

        let block_height = block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse block height")?;

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
                let txs_info = self
                    .get_block_txs(height)
                    .await
                    .context(format!("Could not obtain txs info from block {}", height))?;

                let mut gas_wanted = Vec::new();
                let mut gas_used = Vec::new();

                for tx in txs_info {
                    gas_wanted.push(
                        tx.tx_result
                            .gas_wanted
                            .parse::<usize>()
                            .context("Could not parse tx gas used")?,
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
                block_avg_tx_gas_wanted =
                    gas_wanted.iter().sum::<usize>() as f64 / gas_wanted.len() as f64;
                block_avg_tx_gas_used =
                    gas_used.iter().sum::<usize>() as f64 / gas_used.len() as f64;
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
