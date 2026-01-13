use crate::core::block_window::BlockWindow;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
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
        // Only insert new validator addresses
        let current_validators = self.get_validators().await?;
        let new_addresses: Vec<String> = signatures
            .iter()
            .filter(|addr| !current_validators.contains(*addr))
            .cloned()
            .collect();
        if !new_addresses.is_empty() {
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
        }
        // Save signatures for this block
        let mut insert_sigs = self.clickhouse_client.insert(VALIDATORS_SIGNATURES_TABLE)?;
        let timestamp = DateTime::<Utc>::from_naive_utc_and_offset(timestamp, Utc);

        for address in &current_validators {
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
        insert_sigs
            .end()
            .await
            .context("Failed to end signature insert")?;
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
