use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use serde_json::from_str;
use tracing::info;

use crate::blockchains::sei::metrics::{COMETBFT_BLOCK_TXS, COMETBFT_CURRENT_BLOCK_HEIGHT};
use crate::blockchains::sei::types::SeiTxResponse;
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;

pub struct Block {
    pub app_context: Arc<AppContext>,
}

impl Block {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn fetch_sei_txs(&self, height: u64) -> anyhow::Result<Vec<crate::blockchains::sei::types::SeiTx>> {
        let path = format!("/tx_search?query=\"tx.height={}\"", height);
        let client = self.app_context.rpc.as_ref().unwrap();

        let res = client
            .get(Path::from(path))
            .await
            .context(format!("Could not fetch txs for height {}", height))?;

        let tx_response: SeiTxResponse = from_str(&res)
            .context("Could not deserialize Sei txs response")?;

        Ok(tx_response.txs)
    }

    async fn process_block(&self, height: u64) -> anyhow::Result<()> {
        // Update current block height
        COMETBFT_CURRENT_BLOCK_HEIGHT
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(height as i64);

        // Fetch and process transactions
        let txs = self.fetch_sei_txs(height).await?;

        // Update transaction count as a gauge to match Allora naming
        COMETBFT_BLOCK_TXS
            .with_label_values(&[
                &self.app_context.chain_id,
                &self.app_context.config.general.network,
            ])
            .set(txs.len() as f64);

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
        info!("(Sei Block) Processing latest block");

        // For now, just process a recent block
        // In a real implementation, you'd want to track the latest block height
        let latest_height = 198855000; // This should be fetched dynamically
        self.process_block(latest_height).await?;

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
