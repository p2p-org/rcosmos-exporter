use std::{env, sync::Arc};

use crate::{
    blockchains::namada::{
        metrics::{
            TENDERMINT_VALIDATOR_15DAYS_MISSED_BLOCKS, TENDERMINT_VALIDATOR_15DAYS_SIGNED_BLOCKS,
            TENDERMINT_VALIDATOR_15DAYS_UPTIME, TENDERMINT_VALIDATOR_1DAY_MISSED_BLOCKS,
            TENDERMINT_VALIDATOR_1DAY_SIGNED_BLOCKS, TENDERMINT_VALIDATOR_1DAY_UPTIME,
            TENDERMINT_VALIDATOR_30DAYS_MISSED_BLOCKS, TENDERMINT_VALIDATOR_30DAYS_SIGNED_BLOCKS,
            TENDERMINT_VALIDATOR_30DAYS_UPTIME, TENDERMINT_VALIDATOR_7DAYS_MISSED_BLOCKS,
            TENDERMINT_VALIDATOR_7DAYS_SIGNED_BLOCKS, TENDERMINT_VALIDATOR_7DAYS_UPTIME,
            TENDERMINT_VALIDATOR_FIRST_TIME_SEEN,
        },
        types::Validator,
    },
    core::{
        chain_id::ChainId, clients::blockchain_client::BlockchainClient, clients::path::Path,
        exporter::Task,
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use clickhouse::{sql::Identifier, Client, Row};
use serde::{Deserialize, Serialize};
use tracing::info;

use anyhow::Context;

#[derive(Debug, Row, Deserialize, Serialize)]
struct NamadaValidatorSignature<'a> {
    chain_id: &'a str,
    height: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    timestamp: DateTime<Utc>,
    address: &'a str,
    signed: u8,
}

#[derive(Row, Deserialize, Serialize)]
struct NamadaValidator<'a> {
    chain_id: &'a str,
    address: &'a str,
}

#[derive(Row, Deserialize, Debug, Clone)]
struct NamadaUptime<'a> {
    address: &'a str,
    missed: u64,
    signed_blocks: u64,
    uptime: f64,
}

#[derive(Row, Deserialize, Debug, Clone)]
struct NamadaFirstSeen<'a> {
    address: &'a str,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    first_seen: DateTime<Utc>,
}

fn read_env_var(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("{key} env variable should be set"))
}

const VALIDATORS_TABLE: &'static str = "validators";
const VALIDATORS_SIGNATURES_TABLE: &'static str = "validators_signatures";

pub struct NamadaUptimeTracker {
    client: Arc<BlockchainClient>,
    clickhouse_client: Client,
    chain_id: ChainId,
    network: String,
    validators: Vec<String>,
}

impl NamadaUptimeTracker {
    pub fn new(client: Arc<BlockchainClient>, chain_id: ChainId, network: String) -> Self {
        Self {
            client,
            chain_id,
            network,
            validators: Vec::new(),
            clickhouse_client: Client::default()
                .with_url(read_env_var("CLICKHOUSE_URL"))
                .with_user(read_env_var("CLICKHOUSE_USER"))
                .with_password(read_env_var("CLICKHOUSE_PASSWORD"))
                .with_database(read_env_var("CLICKHOUSE_DATABASE")),
        }
    }

    async fn get_validators(&self) -> anyhow::Result<Vec<NamadaValidator>> {
        let mut validators = vec![];
        let mut cursor = self
            .clickhouse_client
            .query("SELECT ?fields FROM ? WHERE chain_id = ?")
            .bind(Identifier(VALIDATORS_TABLE))
            .bind(self.chain_id.as_str())
            .fetch::<NamadaValidator<'_>>()?;

        while let Some(row) = cursor.next().await? {
            validators.push(row)
        }
        Ok(validators)
    }

    async fn save_validators(&self, addresses: Vec<String>) -> anyhow::Result<()> {
        let mut insert = self.clickhouse_client.insert(VALIDATORS_TABLE)?;
        for address in &addresses {
            insert
                .write(&NamadaValidator {
                    chain_id: self.chain_id.as_str(),
                    address,
                })
                .await?;
        }
        insert.end().await?;
        Ok(())
    }

    async fn save_validators_signatures(
        &self,
        validator_signatures: Vec<(String, bool)>,
        height: u64,
        timestamp: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let mut insert = self.clickhouse_client.insert(VALIDATORS_SIGNATURES_TABLE)?;

        for validator_signature in &validator_signatures {
            let signature = &NamadaValidatorSignature {
                height,
                chain_id: self.chain_id.as_str(),
                address: &validator_signature.0,
                timestamp,
                signed: if validator_signature.1 { 1 } else { 0 },
            };

            insert.write(signature).await?;
        }

        insert.end().await?;
        Ok(())
    }

    async fn get_current_epoch(&self) -> anyhow::Result<u64> {
        let res = self
            .client
            .with_rest()
            .get(Path::from("/api/v1/chain/epoch/latest"))
            .await
            .context("Could not fetch current epoch")?;

        let value = serde_json::from_str::<serde_json::Value>(&res)?;
        let epoch_str = value
            .get("epoch")
            .and_then(|e| e.as_str())
            .context("Could not parse epoch string")?;
        Ok(epoch_str.parse()?)
    }

    async fn get_validators_from_api(&self) -> anyhow::Result<Vec<Validator>> {
        let res = self
            .client
            .with_rest()
            .get(Path::from("/api/v1/pos/validator/all"))
            .await
            .context("Could not fetch validators")?;
        Ok(serde_json::from_str(&res)?)
    }

    async fn get_last_processed_height(&self) -> anyhow::Result<Option<u64>> {
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
            .fetch::<NamadaValidatorSignature<'_>>()?;

        let row = cursor.next().await?;

        if let Some(signature) = row {
            return Ok(Some(signature.height));
        } else {
            return Ok(None);
        }
    }

    async fn process_height(&mut self, height: u64) -> anyhow::Result<()> {
        info!("(Namada Uptime Tracker) Processing height: {}", height);

        let api_validators = self.get_validators_from_api().await?;
        let addresses: Vec<String> = api_validators.iter().map(|v| v.address.clone()).collect();

        let new_addresses: Vec<String> = addresses
            .iter()
            .filter(|addr| !self.validators.iter().any(|v| v == *addr))
            .cloned()
            .collect();

        self.validators.extend(new_addresses.clone());

        self.save_validators(new_addresses).await?;

        // For Namada, we consider a validator "signed" if they are in consensus state
        let signatures: Vec<(String, bool)> = self
            .validators
            .clone()
            .into_iter()
            .map(|val| {
                let is_signed = api_validators
                    .iter()
                    .find(|v| v.address == val)
                    .map(|v| v.state.as_deref() == Some("consensus"))
                    .unwrap_or(false);
                (val, is_signed)
            })
            .collect();

        let now = Utc::now();
        self.save_validators_signatures(signatures, height, now)
            .await?;

        // Process uptime metrics for different intervals
        let uptime_intervals = vec![30, 15, 7, 1];

        for interval in uptime_intervals {
            let query = format!(
                "\n            SELECT\n                address,\n                missed,\n                total_blocks - missed AS signed_blocks,\n                100.0 * (total_blocks - missed) / total_blocks AS uptime\n            FROM validator_uptime_{}d\n            WHERE chain_id = ?",
                interval
            );

            let mut cursor = self
                .clickhouse_client
                .query(&query)
                .bind(self.chain_id.to_string())
                .fetch::<NamadaUptime<'_>>()?;

            while let Some(row) = cursor.next().await? {
                let filtered_address = row.address.trim_end_matches('\0');
                let filtered_address: String = filtered_address
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();

                match interval {
                    30 => {
                        TENDERMINT_VALIDATOR_30DAYS_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime as i64);
                        TENDERMINT_VALIDATOR_30DAYS_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as i64);
                        TENDERMINT_VALIDATOR_30DAYS_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as i64)
                    }
                    15 => {
                        TENDERMINT_VALIDATOR_15DAYS_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime as i64);
                        TENDERMINT_VALIDATOR_15DAYS_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as i64);
                        TENDERMINT_VALIDATOR_15DAYS_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as i64)
                    }
                    7 => {
                        TENDERMINT_VALIDATOR_7DAYS_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime as i64);
                        TENDERMINT_VALIDATOR_7DAYS_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as i64);
                        TENDERMINT_VALIDATOR_7DAYS_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as i64)
                    }
                    1 => {
                        TENDERMINT_VALIDATOR_1DAY_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime as i64);
                        TENDERMINT_VALIDATOR_1DAY_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as i64);
                        TENDERMINT_VALIDATOR_1DAY_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as i64)
                    }
                    _ => (),
                }
            }
        }
        Ok(())
    }

    async fn track_uptimes(&mut self) -> anyhow::Result<()> {
        if self.validators.len() == 0 {
            info!("(Namada Uptime Tracker) Obtaining validators");

            // Try to get validators from database first
            match self.get_validators().await {
                Ok(db_validators) => {
                    self.validators = db_validators.iter().map(|v| v.address.to_owned()).collect();
                }
                Err(_) => {
                    // If table doesn't exist or query fails, get from API
                    info!("(Namada Uptime Tracker) No validators in database, fetching from API");
                    let api_validators = self.get_validators_from_api().await?;
                    self.validators = api_validators.iter().map(|v| v.address.clone()).collect();

                    // Save validators to database
                    self.save_validators(self.validators.clone()).await?;
                }
            }
        }

        let current_height = self.get_current_epoch().await?;
        let mut last_processed = self
            .get_last_processed_height()
            .await?
            .unwrap_or(current_height - 1);

        while last_processed < current_height {
            last_processed += 1;
            self.process_height(last_processed).await?;
        }

        // Process first seen metrics
        let mut cursor = self
            .clickhouse_client
            .query(
                "SELECT
                    address,
                    minMerge(first_seen) AS first_seen
                FROM validator_first_seen
                WHERE chain_id = ?
                GROUP BY chain_id, address",
            )
            .bind(self.chain_id.to_string())
            .fetch::<NamadaFirstSeen<'_>>()?;

        while let Some(row) = cursor.next().await? {
            TENDERMINT_VALIDATOR_FIRST_TIME_SEEN
                .with_label_values(&[row.address, &self.chain_id.to_string(), &self.network])
                .set(row.first_seen.timestamp());
        }

        // Process uptime metrics for different intervals
        let uptime_intervals = vec![30, 15, 7, 1];

        for interval in uptime_intervals {
            let query = format!(
                "\n            SELECT\n                address,\n                missed,\n                total_blocks - missed AS signed_blocks,\n                100.0 * (total_blocks - missed) / total_blocks AS uptime\n            FROM validator_uptime_{}d\n            WHERE chain_id = ?",
                interval
            );

            let mut cursor = self
                .clickhouse_client
                .query(&query)
                .bind(self.chain_id.to_string())
                .fetch::<NamadaUptime<'_>>()?;

            while let Some(row) = cursor.next().await? {
                let filtered_address = row.address.trim_end_matches('\0');
                let filtered_address: String = filtered_address
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();

                match interval {
                    30 => {
                        TENDERMINT_VALIDATOR_30DAYS_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime as i64);
                        TENDERMINT_VALIDATOR_30DAYS_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as i64);
                        TENDERMINT_VALIDATOR_30DAYS_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as i64)
                    }
                    15 => {
                        TENDERMINT_VALIDATOR_15DAYS_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime as i64);
                        TENDERMINT_VALIDATOR_15DAYS_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as i64);
                        TENDERMINT_VALIDATOR_15DAYS_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as i64)
                    }
                    7 => {
                        TENDERMINT_VALIDATOR_7DAYS_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime as i64);
                        TENDERMINT_VALIDATOR_7DAYS_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as i64);
                        TENDERMINT_VALIDATOR_7DAYS_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as i64)
                    }
                    1 => {
                        TENDERMINT_VALIDATOR_1DAY_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime as i64);
                        TENDERMINT_VALIDATOR_1DAY_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as i64);
                        TENDERMINT_VALIDATOR_1DAY_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as i64)
                    }
                    _ => (),
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Task for NamadaUptimeTracker {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.track_uptimes().await
    }

    fn name(&self) -> &'static str {
        "Namada Uptime Tracker"
    }
}
