use anyhow::{Context};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;  

use crate::{
    blockchains::namada::types::{Validator},
    core::{chain_id::ChainId, clients::blockchain_client::BlockchainClient, exporter::Task},
};

use super::metrics::{
    NAMADA_BLOCK_GAS_USED, NAMADA_BLOCK_GAS_WANTED, NAMADA_CURRENT_BLOCK_HEIGHT, NAMADA_CURRENT_BLOCK_TIME,
    NAMADA_VALIDATOR_MISSED_BLOCKS, NAMADA_VALIDATOR_UPTIME, NAMADA_CURRENT_EPOCH
};

pub struct NamadaBlockScrapper {
    client: Arc<BlockchainClient>,
    processed_epoch: u64,
    chain_id: ChainId,
    network: String,
    validator_alert_addresses: Vec<String>,
}

impl NamadaBlockScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        _block_window: usize,
        chain_id: ChainId,
        network: String,
        validator_alert_addresses: Vec<String>,
    ) -> Self {
        Self {
            client,
            processed_epoch: 0,
            chain_id,
            network,
            validator_alert_addresses,
        }
    }

    async fn get_current_epoch(&self) -> anyhow::Result<u64> {
        let res = self
            .client
            .with_rest()
            .get("api/v1/chain/epoch/latest")
            .await
            .context("Could not fetch current epoch")?;
        let value = serde_json::from_str::<serde_json::Value>(&res)?;
        let epoch_str = value
            .get("epoch")
            .and_then(|e| e.as_str())
            .context("Could not parse epoch string")?;
        Ok(epoch_str.parse()?)
    }

    // For non-paginated endpoint
    async fn get_validators_all(&self) -> anyhow::Result<Vec<Validator>> {
        let res = self
            .client
            .with_rest()
            .get("api/v1/pos/validator/all")
            .await
            .context("Could not fetch all validators")?;
        Ok(serde_json::from_str(&res)?)
    }

    #[allow(dead_code)]
    async fn get_validators_paginated(&self, page: Option<u32>, state: Option<&[&str]>, sort_field: Option<&str>, sort_order: Option<&str>) -> anyhow::Result<Vec<Validator>> {
        let mut url = String::from("api/v1/pos/validator");
        let mut params = vec![];
        if let Some(page) = page {
            params.push(format!("page={}", page));
        }
        if let Some(state) = state {
            for s in state {
                params.push(format!("state={}", s));
            }
        }
        if let Some(sort_field) = sort_field {
            params.push(format!("sortField={}", sort_field));
        }
        if let Some(sort_order) = sort_order {
            params.push(format!("sortOrder={}", sort_order));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        let res = self
            .client
            .with_rest()
            .get(&url)
            .await
            .context("Could not fetch paginated validators")?;
        let value: serde_json::Value = serde_json::from_str(&res)?;
        let results = value.get("results").ok_or_else(|| anyhow::anyhow!("Missing 'results' field in paginated validators response"))?;
        Ok(serde_json::from_value(results.clone())?)
    }

    // Use the non-paginated version by default
    async fn get_validators(&self) -> anyhow::Result<Vec<Validator>> {
        self.get_validators_all().await
    }

    async fn process_block_window(&mut self) -> anyhow::Result<()> {
        let current_epoch = self.get_current_epoch().await?;
        NAMADA_CURRENT_EPOCH
            .with_label_values(&[&self.chain_id.to_string(), &self.network])
            .set(current_epoch as i64);

        // Fetch latest block info
        let block_res = self.client.with_rest().get("api/v1/chain/block/latest").await?;
        let block_json: serde_json::Value = serde_json::from_str(&block_res)?;
        let block = &block_json["block"];
        let height = block["height"].as_u64().unwrap_or(0);
        let time_str = block["time"].as_str().unwrap_or("");
        let gas_used = block["gas_used"].as_u64().unwrap_or(0);
        let gas_wanted = block["gas_wanted"].as_u64().unwrap_or(0);

        NAMADA_CURRENT_BLOCK_HEIGHT.with_label_values(&[&self.chain_id.to_string(), &self.network]).set(height as i64);
        // Convert time_str to unix timestamp if possible
        let block_time = chrono::DateTime::parse_from_rfc3339(time_str).map(|dt| dt.timestamp()).unwrap_or(0);
        NAMADA_CURRENT_BLOCK_TIME.with_label_values(&[&self.chain_id.to_string(), &self.network]).set(block_time);
        NAMADA_BLOCK_GAS_USED.with_label_values(&[&self.chain_id.to_string(), &self.network, &height.to_string()]).set(gas_used as i64);
        NAMADA_BLOCK_GAS_WANTED.with_label_values(&[&self.chain_id.to_string(), &self.network, &height.to_string()]).set(gas_wanted as i64);

        let epoch_to_process = current_epoch - 1;
        if epoch_to_process == self.processed_epoch {
            info!("(Namada Scrapper) Epoch to be processed: {}, has been already processed. Skipping... ", epoch_to_process);
            return Ok(());
        }
        info!("(Namada Scrapper) Processing epoch: {}", epoch_to_process);
        let validators = self.get_validators().await?;
        
        for validator in validators {
            let _fires_alerts = self
                .validator_alert_addresses
                .contains(&validator.address)
                .to_string();

            // Missed blocks (signing info)
            let signing_info_url = format!("api/v1/pos/validator/{}/signing-info", validator.address);
            let signing_info_res = self.client.with_rest().get(&signing_info_url).await;
            let missed_blocks = if let Ok(signing_info_res) = signing_info_res {
                let signing_info: serde_json::Value = serde_json::from_str(&signing_info_res).unwrap_or_default();
                signing_info["missed_blocks_counter"].as_u64().unwrap_or(0)
            } else { 0 };
            NAMADA_VALIDATOR_MISSED_BLOCKS
                .with_label_values(&[&validator.address, &self.chain_id.to_string(), &self.network])
                .set(missed_blocks as i64);

            // Uptime
            let uptime_url = format!("api/v1/pos/validator/{}/uptime", validator.address);
            let uptime_res = self.client.with_rest().get(&uptime_url).await;
            let uptime_val = if let Ok(uptime_res) = uptime_res {
                let uptime_json: serde_json::Value = serde_json::from_str(&uptime_res).unwrap_or_default();
                uptime_json["uptime"].as_f64().unwrap_or(0.0)
            } else { 0.0 };
            NAMADA_VALIDATOR_UPTIME
                .with_label_values(&[&validator.address, &self.chain_id.to_string(), &self.network])
                .set((uptime_val * 100.0) as i64); // as percentage
        }

        self.processed_epoch = epoch_to_process;
        Ok(())
    }
}

#[async_trait]
impl Task for NamadaBlockScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_block_window().await
    }
    fn name(&self) -> &'static str {
        "Namada Scrapper"
    }
}
