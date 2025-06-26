use std::{env, sync::Arc};

use crate::{
    blockchains::tendermint::{
        metrics::{
            TENDERMINT_VALIDATOR_15DAYS_MISSED_BLOCKS, TENDERMINT_VALIDATOR_15DAYS_SIGNED_BLOCKS,
            TENDERMINT_VALIDATOR_15DAYS_UPTIME, TENDERMINT_VALIDATOR_1DAY_MISSED_BLOCKS,
            TENDERMINT_VALIDATOR_1DAY_SIGNED_BLOCKS, TENDERMINT_VALIDATOR_1DAY_UPTIME,
            TENDERMINT_VALIDATOR_30DAYS_MISSED_BLOCKS, TENDERMINT_VALIDATOR_30DAYS_SIGNED_BLOCKS,
            TENDERMINT_VALIDATOR_30DAYS_UPTIME, TENDERMINT_VALIDATOR_7DAYS_MISSED_BLOCKS,
            TENDERMINT_VALIDATOR_7DAYS_SIGNED_BLOCKS, TENDERMINT_VALIDATOR_7DAYS_UPTIME,
            TENDERMINT_VALIDATOR_FIRST_TIME_SEEN,
        },
        types::{TendermintBlock, TendermintBlockResponse},
    },
    core::{block_height::BlockHeight, clients::path::Path},
};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Timelike, Utc};
use clickhouse::{sql::Identifier, Client, Row};
use serde::{Deserialize, Serialize};
use serde_json::from_str;
use tracing::{debug, info};

use crate::core::{
    chain_id::ChainId, clients::blockchain_client::BlockchainClient, exporter::Task,
};
use anyhow::Context;

#[derive(Debug, Row, Deserialize, Serialize)]
struct ValidatorSignature<'a> {
    chain_id: &'a str,
    height: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    timestamp: DateTime<Utc>,
    address: &'a str,
    signed: u8,
}

#[derive(Row, Deserialize, Serialize)]
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

fn read_env_var(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("{key} env variable should be set"))
}

const VALIDATORS_TABLE: &'static str = "validators";
const VALIDATORS_SIGNATURES_TABLE: &'static str = "validators_signatures";

pub struct TendermintUptimeTracker {
    client: Arc<BlockchainClient>,
    clickhouse_client: Client,
    chain_id: ChainId,
    network: String,
    validators: Vec<String>,
}

impl TendermintUptimeTracker {
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

    async fn get_validators(&self) -> anyhow::Result<Vec<Validator>> {
        let mut validators = vec![];
        let mut cursor = self
            .clickhouse_client
            .query("SELECT ?fields FROM ? WHERE chain_id = ?")
            .bind(Identifier(VALIDATORS_TABLE))
            .bind(self.chain_id.as_str())
            .fetch::<Validator<'_>>()?;

        while let Some(row) = cursor.next().await? {
            validators.push(row)
        }
        Ok(validators)
    }

    async fn save_validators(&self, addresses: Vec<String>) -> anyhow::Result<()> {
        let mut insert = self.clickhouse_client.insert(VALIDATORS_TABLE)?;
        for address in &addresses {
            insert
                .write(&Validator {
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
        timestamp: NaiveDateTime,
    ) -> anyhow::Result<()> {
        let mut insert = self.clickhouse_client.insert(VALIDATORS_SIGNATURES_TABLE)?;

        let timestamp = DateTime::<Utc>::from_utc(timestamp, Utc);

        for validator_signature in &validator_signatures {
            let signature = &ValidatorSignature {
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

    async fn get_block(&mut self, height: BlockHeight) -> anyhow::Result<TendermintBlock> {
        let path = match height {
            BlockHeight::Height(h) => {
                info!(
                    "(Tendermint Uptime Tracker) Obtaining block with height: {}",
                    h
                );
                format!("/block?height={}", h)
            }
            BlockHeight::Latest => {
                info!("(Tendermint Uptime Tracker) Obtaining latest block");
                "/block".to_string()
            }
        };

        let res = self
            .client
            .with_rpc()
            .get(Path::from(path.clone()))
            .await
            .context(format!("Could not fetch block {}", path))?;

        Ok(from_str::<TendermintBlockResponse>(&res)
            .context("Could not deserialize block response")?
            .result
            .block)
    }

    async fn get_last_processed_block_height(&self) -> anyhow::Result<Option<u64>> {
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
            return Ok(Some(signature.height));
        } else {
            return Ok(None);
        }
    }

    async fn process_block(&mut self, height: BlockHeight) -> anyhow::Result<()> {
        let block = self.get_block(height).await?;

        let height = block.header.height.parse::<u64>()?;

        let addresses: Vec<String> = block
            .last_commit
            .signatures
            .iter()
            .filter(|sig| sig.signature.is_some())
            .map(|sig| sig.validator_address.clone())
            .collect();

        let new_addresses: Vec<String> = addresses
            .iter()
            .filter(|addr| !self.validators.iter().any(|v| v == *addr))
            .cloned()
            .collect();

        self.validators.extend(new_addresses.clone());

        self.save_validators(new_addresses).await?;

        let signatures: Vec<(String, bool)> = self
            .validators
            .clone()
            .into_iter()
            .map(|val| (val.clone(), addresses.contains(&val)))
            .collect();

        self.save_validators_signatures(signatures, height, block.header.time)
            .await?;
        Ok(())
    }

    async fn track_uptimes(&mut self) -> anyhow::Result<()> {
        if self.validators.len() == 0 {
            info!("Obtaining validators");
            self.validators = self
                .get_validators()
                .await?
                .iter()
                .map(|v| v.address.to_owned())
                .collect();
        }

        let last_height = self
            .get_block(BlockHeight::Latest)
            .await?
            .header
            .height
            .parse::<u64>()?;

        let mut last_processed = self
            .get_last_processed_block_height()
            .await?
            .unwrap_or(last_height - 1);

        while last_processed < last_height {
            last_processed += 1;
            self.process_block(BlockHeight::Height(last_processed as usize))
                .await?;
        }

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
            .fetch::<FirstSeen<'_>>()?;

        while let Some(row) = cursor.next().await? {
            TENDERMINT_VALIDATOR_FIRST_TIME_SEEN
                .with_label_values(&[row.address, &self.chain_id.to_string(), &self.network])
                .set(row.first_seen.timestamp() as f64);
        }

        let uptime_intervals = vec![30, 15, 7, 1];

        for interval in uptime_intervals {
            let query = format!(
                "
            SELECT
                address,
                total_blocks,
                missed,
                total_blocks - missed AS signed_blocks,
                100.0 * (total_blocks - missed) / total_blocks AS uptime
            FROM validator_uptime_{}d
            WHERE chain_id = ?",
                interval
            );

            let mut cursor = self
                .clickhouse_client
                .query(&query)
                .bind(self.chain_id.to_string())
                .fetch::<Uptime<'_>>()?;

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
                            .set(row.uptime);
                        TENDERMINT_VALIDATOR_30DAYS_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as f64);
                        TENDERMINT_VALIDATOR_30DAYS_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as f64)
                    }
                    15 => {
                        TENDERMINT_VALIDATOR_15DAYS_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime);
                        TENDERMINT_VALIDATOR_15DAYS_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as f64);
                        TENDERMINT_VALIDATOR_15DAYS_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as f64)
                    }
                    7 => {
                        TENDERMINT_VALIDATOR_7DAYS_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime);
                        TENDERMINT_VALIDATOR_7DAYS_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as f64);
                        TENDERMINT_VALIDATOR_7DAYS_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as f64)
                    }
                    1 => {
                        TENDERMINT_VALIDATOR_1DAY_UPTIME
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.uptime);
                        TENDERMINT_VALIDATOR_1DAY_SIGNED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.signed_blocks as f64);
                        TENDERMINT_VALIDATOR_1DAY_MISSED_BLOCKS
                            .with_label_values(&[
                                &filtered_address,
                                &self.chain_id.to_string(),
                                &self.network,
                            ])
                            .set(row.missed as f64)
                    }
                    _ => (),
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Task for TendermintUptimeTracker {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.track_uptimes().await
    }

    fn name(&self) -> &'static str {
        "Tendermint Uptime Tracker"
    }
}
