use crate::core::block_window::BlockWindow;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use tracing::{debug, warn};
use clickhouse::{sql::Identifier, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub enum UptimeWindow {
    BlockWindow,
    OneDay,
    SevenDays,
    FifteenDays,
    ThirtyDays,
    SixMonths,
}

impl UptimeWindow {
    pub fn as_interval(&self) -> Option<&'static str> {
        match self {
            UptimeWindow::OneDay => Some("1"),
            UptimeWindow::SevenDays => Some("7"),
            UptimeWindow::FifteenDays => Some("15"),
            UptimeWindow::ThirtyDays => Some("30"),
            UptimeWindow::SixMonths => Some("180"),
            UptimeWindow::BlockWindow => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValidatorUptime {
    pub address: String,
    pub uptime: f64,
    pub signed_blocks: u64,
    pub missed_blocks: u64,
    pub total_blocks: u64,
    pub first_time_seen: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait SignatureStorage: Send + Sync {
    async fn save_signatures(
        &mut self,
        height: usize,
        timestamp: NaiveDateTime,
        signatures: Vec<String>,
    ) -> Result<()>;

    /// Batch save signatures for multiple blocks. Default implementation calls save_signatures in a loop.
    /// ClickHouse implementation overrides this for better performance.
    async fn save_signatures_batch(
        &mut self,
        blocks: Vec<(usize, NaiveDateTime, Vec<String>)>,
    ) -> Result<()> {
        for (height, timestamp, signatures) in blocks {
            self.save_signatures(height, timestamp, signatures).await?;
        }
        Ok(())
    }

    async fn uptimes(&self, window: UptimeWindow) -> Result<HashMap<String, ValidatorUptime>>;
    async fn get_last_processed_height(&self) -> anyhow::Result<Option<usize>>;
}

pub struct InMemorySignatureStorage {
    pub block_window: BlockWindow,
    pub processed_height: usize,
}

#[async_trait]
impl SignatureStorage for InMemorySignatureStorage {
    async fn save_signatures(
        &mut self,
        height: usize,
        _timestamp: NaiveDateTime,
        signatures: Vec<String>,
    ) -> Result<()> {
        self.block_window.add_block_signers(signatures);
        self.processed_height = height;
        Ok(())
    }
    async fn uptimes(&self, window: UptimeWindow) -> Result<HashMap<String, ValidatorUptime>> {
        match window {
            UptimeWindow::BlockWindow => {
                let blocks = self.block_window.blocks();
                let window_size = blocks.len() as u64;
                let mut counts: HashMap<String, u64> = HashMap::new();
                for block_signers in blocks {
                    for signer in block_signers {
                        *counts.entry(signer.clone()).or_insert(0) += 1;
                    }
                }
                let mut result = HashMap::new();
                for (address, signed_blocks) in counts {
                    let missed_blocks = window_size.saturating_sub(signed_blocks);
                    let uptime = if window_size > 0 {
                        (signed_blocks as f64) / (window_size as f64) * 100.0
                    } else {
                        0.0
                    };
                    result.insert(
                        address.clone(),
                        ValidatorUptime {
                            address,
                            uptime,
                            signed_blocks,
                            missed_blocks,
                            total_blocks: window_size,
                            first_time_seen: None,
                        },
                    );
                }
                Ok(result)
            }
            _ => bail!("Only BlockWindow is supported for in-memory storage"),
        }
    }
    async fn get_last_processed_height(&self) -> anyhow::Result<Option<usize>> {
        Ok(Some(self.processed_height))
    }
}

#[derive(Debug, Row, Deserialize, Serialize)]
struct ValidatorSignature<'a> {
    chain_id: &'a str,
    height: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    timestamp: DateTime<Utc>,
    address: &'a str,
    signed: u8,
}

#[derive(Row, Deserialize, Serialize, Debug)]
struct Validator<'a> {
    chain_id: &'a str,
    address: &'a str,
}

#[derive(Row, Deserialize, Debug, Clone)]
struct Uptime<'a> {
    address: &'a str,
    total_blocks: u64,
    missed: u64,
    signed_blocks: u64,
    uptime: f64,
}

#[derive(Row, Deserialize, Debug, Clone)]
struct FirstSeen<'a> {
    address: &'a str,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    first_seen: DateTime<Utc>,
}

const VALIDATORS_TABLE: &str = "validators";
const VALIDATORS_SIGNATURES_TABLE: &str = "validators_signatures";

pub struct ClickhouseSignatureStorage {
    pub clickhouse_client: clickhouse::Client,
    pub chain_id: String,
    // Cache validators to avoid querying ClickHouse on every block
    // Refreshed periodically (every 100 blocks) to pick up new validators
    pub cached_validators: Option<(std::collections::HashSet<String>, usize)>,
}

impl ClickhouseSignatureStorage {
    pub async fn get_validators(&self) -> anyhow::Result<std::collections::HashSet<String>> {
        let mut validators = std::collections::HashSet::new();
        let mut cursor = self
            .clickhouse_client
            .query("SELECT ?fields FROM ? WHERE chain_id = ?")
            .bind(Identifier(VALIDATORS_TABLE))
            .bind(self.chain_id.as_str())
            .fetch::<Validator<'_>>()?;

        while let Some(row) = cursor.next().await? {
            validators.insert(row.address.to_string());
        }
        Ok(validators)
    }
}

#[async_trait]
impl SignatureStorage for ClickhouseSignatureStorage {
    async fn save_signatures(
        &mut self,
        height: usize,
        timestamp: NaiveDateTime,
        signatures: Vec<String>,
    ) -> anyhow::Result<()> {
        let start = std::time::Instant::now();

        // Cache validators to avoid querying ClickHouse on every block (major performance improvement)
        // Refresh cache every 100 blocks to pick up new validators
        const CACHE_REFRESH_INTERVAL: usize = 100;
        let should_refresh_cache = self.cached_validators
            .as_ref()
            .map(|(_, cached_height)| height.saturating_sub(*cached_height) >= CACHE_REFRESH_INTERVAL)
            .unwrap_or(true);

        if should_refresh_cache {
            let cache_start = std::time::Instant::now();
            let validators = self.get_validators().await?;
            debug!("(ClickHouse) Validator cache refresh took {:?} ({} validators)", cache_start.elapsed(), validators.len());
            self.cached_validators = Some((validators, height));
        }

        // Only insert new validator addresses
        // Get current validators first, then update cache if needed
        let current_validators_set = self.cached_validators.as_ref().unwrap().0.clone();
        let new_addresses: Vec<String> = signatures
            .iter()
            .filter(|addr| !current_validators_set.contains(*addr))
            .cloned()
            .collect();
        if !new_addresses.is_empty() {
            // Add new validators to cache immediately
            if let Some((ref mut cached, ref mut cached_height)) = self.cached_validators {
                for addr in &new_addresses {
                    cached.insert(addr.clone());
                }
                *cached_height = height; // Update cache height
            }

            let validator_insert_start = std::time::Instant::now();
            let mut insert_validators = self.clickhouse_client.insert(VALIDATORS_TABLE)?;
            for address in &new_addresses {
                insert_validators
                    .write(&Validator {
                        chain_id: self.chain_id.as_str(),
                        address,
                    })
                    .await
                    .context("Failed to write validator")?;
            }
            insert_validators
                .end()
                .await
                .context("Failed to end validator insert")?;
            debug!("(ClickHouse) Validator insert took {:?} ({} new validators)", validator_insert_start.elapsed(), new_addresses.len());
        }

        // Save signatures for this block
        let sig_insert_start = std::time::Instant::now();
        let mut insert_sigs = self.clickhouse_client.insert(VALIDATORS_SIGNATURES_TABLE)?;
        let timestamp = DateTime::<Utc>::from_naive_utc_and_offset(timestamp, Utc);
        let validator_count = current_validators_set.len();

        for address in current_validators_set.iter() {
            let signed = if signatures.contains(address) { 1 } else { 0 };
            let signature = &ValidatorSignature {
                height: height as u64,
                chain_id: self.chain_id.as_str(),
                address: address.as_str(),
                timestamp,
                signed,
            };
            insert_sigs
                .write(signature)
                .await
                .context("Failed to write signature")?;
        }
        let write_end = std::time::Instant::now();
        insert_sigs
            .end()
            .await
            .context("Failed to end signature insert")?;
        let total_sig_time = sig_insert_start.elapsed();
        let flush_time = write_end.elapsed();

        if total_sig_time.as_millis() > 1000 {
            warn!(
                "(ClickHouse) Slow signature insert for block {}: total={:?}, writes={:?}, flush={:?}, validators={}",
                height,
                total_sig_time,
                write_end.duration_since(sig_insert_start),
                flush_time,
                validator_count
            );
        }

        let total_time = start.elapsed();
        if total_time.as_millis() > 2000 {
            warn!(
                "(ClickHouse) Slow save_signatures for block {}: total={:?}, validators={}",
                height,
                total_time,
                validator_count
            );
        }

        Ok(())
    }

    /// Override batch method for ClickHouse - much faster than default implementation
    async fn save_signatures_batch(
        &mut self,
        blocks: Vec<(usize, NaiveDateTime, Vec<String>)>,
    ) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let start = std::time::Instant::now();
        let first_height = blocks[0].0;
        let last_height = blocks[blocks.len() - 1].0;

        // Refresh validator cache if needed (check against first block in batch)
        const CACHE_REFRESH_INTERVAL: usize = 100;
        let should_refresh_cache = self.cached_validators
            .as_ref()
            .map(|(_, cached_height)| first_height.saturating_sub(*cached_height) >= CACHE_REFRESH_INTERVAL)
            .unwrap_or(true);

        if should_refresh_cache {
            let cache_start = std::time::Instant::now();
            let validators = self.get_validators().await?;
            debug!("(ClickHouse) Validator cache refresh took {:?} ({} validators)", cache_start.elapsed(), validators.len());
            self.cached_validators = Some((validators, last_height));
        }

        let current_validators_set = self.cached_validators.as_ref().unwrap().0.clone();
        let validator_count = current_validators_set.len();

        // Collect all new validator addresses across the batch
        let mut all_new_addresses = std::collections::HashSet::new();
        for (_, _, signatures) in &blocks {
            for addr in signatures {
                if !current_validators_set.contains(addr) {
                    all_new_addresses.insert(addr.clone());
                }
            }
        }

        // Insert new validators if any
        if !all_new_addresses.is_empty() {
            // Update cache immediately
            if let Some((ref mut cached, ref mut cached_height)) = self.cached_validators {
                for addr in &all_new_addresses {
                    cached.insert(addr.clone());
                }
                *cached_height = last_height;
            }

            let validator_insert_start = std::time::Instant::now();
            let mut insert_validators = self.clickhouse_client.insert(VALIDATORS_TABLE)?;
            for address in &all_new_addresses {
                insert_validators
                    .write(&Validator {
                        chain_id: self.chain_id.as_str(),
                        address,
                    })
                    .await
                    .context("Failed to write validator")?;
            }
            insert_validators
                .end()
                .await
                .context("Failed to end validator insert")?;
            debug!("(ClickHouse) Validator insert took {:?} ({} new validators)", validator_insert_start.elapsed(), all_new_addresses.len());
        }

        // Batch insert all signatures for all blocks in one operation
        let sig_insert_start = std::time::Instant::now();
        let mut insert_sigs = self.clickhouse_client.insert(VALIDATORS_SIGNATURES_TABLE)?;

        for (height, timestamp, signatures) in &blocks {
            let timestamp = DateTime::<Utc>::from_naive_utc_and_offset(*timestamp, Utc);
            for address in current_validators_set.iter() {
                let signed = if signatures.contains(address) { 1 } else { 0 };
                let signature = &ValidatorSignature {
                    height: *height as u64,
                    chain_id: self.chain_id.as_str(),
                    address: address.as_str(),
                    timestamp,
                    signed,
                };
                insert_sigs
                    .write(signature)
                    .await
                    .context("Failed to write signature")?;
            }
        }

        let write_end = std::time::Instant::now();
        insert_sigs
            .end()
            .await
            .context("Failed to end signature batch insert")?;
        let total_sig_time = sig_insert_start.elapsed();
        let flush_time = write_end.elapsed();

        let total_time = start.elapsed();
        let blocks_count = blocks.len();
        let rows_written = blocks_count * validator_count;

        if total_sig_time.as_millis() > 1000 {
            warn!(
                "(ClickHouse) Slow signature batch insert for blocks {}-{}: total={:?}, writes={:?}, flush={:?}, blocks={}, rows={}, validators={}",
                first_height,
                last_height,
                total_sig_time,
                write_end.duration_since(sig_insert_start),
                flush_time,
                blocks_count,
                rows_written,
                validator_count
            );
        }

        debug!(
            "(ClickHouse) Batch insert: {} blocks ({} rows) in {:?} ({:.2}ms per block, {:.2}μs per row)",
            blocks_count,
            rows_written,
            total_time,
            total_time.as_millis() as f64 / blocks_count as f64,
            total_time.as_micros() as f64 / rows_written as f64
        );

        Ok(())
    }

    async fn uptimes(&self, window: UptimeWindow) -> Result<HashMap<String, ValidatorUptime>> {
        let query = format!(
            r#"
            SELECT
                address,
                sum(total_blocks) AS total_blocks,
                sum(missed) AS missed,
                total_blocks - missed AS signed_blocks,
                100 * ((total_blocks - missed) / total_blocks) as uptime
            FROM
            (
                SELECT
                    chain_id,
                    address,
                    countMerge(total_blocks) AS total_blocks,
                    sumMerge(missed) AS missed
                FROM validator_uptime_buckets
                WHERE bucket_start >= now() - INTERVAL {} DAY AND chain_id = ?
                GROUP BY chain_id, address
            )
            GROUP BY chain_id, address
            ORDER BY missed DESC
            "#,
            match window.as_interval() {
                Some(interval) => interval,
                None => bail!("BlockWindow is not supported for ClickHouse storage"),
            }
        );
        let mut cursor = self
            .clickhouse_client
            .query(&query)
            .bind(self.chain_id.as_str())
            .fetch::<Uptime<'_>>()?;
        let mut uptimes = std::collections::HashMap::new();
        while let Some(row) = cursor.next().await? {
            uptimes.insert(
                row.address.to_string(),
                ValidatorUptime {
                    address: row.address.to_string(),
                    uptime: row.uptime,
                    signed_blocks: row.signed_blocks,
                    missed_blocks: row.missed,
                    total_blocks: row.total_blocks,
                    first_time_seen: None,
                },
            );
        }

        let mut first_seen_map = std::collections::HashMap::new();
        let mut cursor = self
            .clickhouse_client
            .query(
                "SELECT address, minMerge(first_seen) AS first_seen FROM validator_first_seen WHERE chain_id = ? GROUP BY chain_id, address"
            )
            .bind(self.chain_id.to_string())
            .fetch::<FirstSeen<'_>>()?;
        while let Some(row) = cursor.next().await? {
            let filtered_address = row.address.trim_end_matches('\0');
            let filtered_address: String = filtered_address
                .chars()
                .filter(|c| !c.is_control())
                .collect();
            first_seen_map.insert(filtered_address, row.first_seen);
        }

        for (address, uptime) in uptimes.iter_mut() {
            if let Some(first_seen) = first_seen_map.get(address) {
                uptime.first_time_seen = Some(*first_seen);
            }
        }
        Ok(uptimes)
    }

    async fn get_last_processed_height(&self) -> anyhow::Result<Option<usize>> {
        let query = r#"
            SELECT ?fields
            FROM ?
            WHERE chain_id = ?
            ORDER BY height DESC
            LIMIT 1
        "#;

        let mut cursor = self
            .clickhouse_client
            .query(query)
            .bind(Identifier(VALIDATORS_SIGNATURES_TABLE))
            .bind(self.chain_id.as_str())
            .fetch::<ValidatorSignature<'_>>()?;

        let row = cursor.next().await?;

        if let Some(signature) = row {
            return Ok(Some(signature.height as usize));
        } else {
            return Ok(None);
        }
    }
}
