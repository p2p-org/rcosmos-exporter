use std::sync::Arc;

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
        types::{
            Block, BlockResponse, NamadaFirstSeen, NamadaUptime, NamadaValidator,
            NamadaValidatorSignature, ValidatorsResponse, NAMADA_VALIDATORS_SIGNATURES_TABLE,
            NAMADA_VALIDATORS_TABLE,
        },
    },
    core::{
        block_height::BlockHeight, chain_id::ChainId, clients::blockchain_client::BlockchainClient,
        clients::path::Path, exporter::Task,
    },
};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use clickhouse::{sql::Identifier, Client};
use serde_json::from_str;
use tracing::info;

use anyhow::Context;

fn read_env_var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} env variable should be set"))
}

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

    async fn get_validators(&self) -> anyhow::Result<Vec<NamadaValidator<'static>>> {
        let mut validators = vec![];
        let mut cursor = self
            .clickhouse_client
            .query("SELECT ?fields FROM ? WHERE chain_id = ?")
            .bind(Identifier(NAMADA_VALIDATORS_TABLE))
            .bind(self.chain_id.as_str())
            .fetch::<NamadaValidator<'static>>()?;

        while let Some(row) = cursor.next().await? {
            validators.push(row)
        }
        Ok(validators)
    }

    async fn save_validators(&self, addresses: Vec<String>) -> anyhow::Result<()> {
        let mut insert = self.clickhouse_client.insert(NAMADA_VALIDATORS_TABLE)?;
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
        timestamp: NaiveDateTime,
    ) -> anyhow::Result<()> {
        let mut insert = self
            .clickhouse_client
            .insert(NAMADA_VALIDATORS_SIGNATURES_TABLE)?;

        let timestamp = DateTime::<Utc>::from_naive_utc_and_offset(timestamp, Utc);

        for validator_signature in &validator_signatures {
            let signature = &NamadaValidatorSignature {
                height,
                chain_id: self.chain_id.as_str(),
                address: &validator_signature.0,
                timestamp,
                signed: if validator_signature.1 { 1u8 } else { 0u8 },
            };

            insert.write(signature).await?;
        }

        insert.end().await?;
        Ok(())
    }

    async fn get_block(&mut self, height: BlockHeight) -> anyhow::Result<Block> {
        let path = match height {
            BlockHeight::Height(h) => {
                info!("(Namada Uptime Tracker) Obtaining block with height: {}", h);
                format!("/block?height={}", h)
            }
            BlockHeight::Latest => {
                info!("(Namada Uptime Tracker) Obtaining latest block");
                "/block".to_string()
            }
        };

        let res = self
            .client
            .with_rpc()
            .get(Path::from(path.clone()))
            .await
            .context(format!("Could not fetch block {}", path))?;

        Ok(from_str::<BlockResponse>(&res)
            .context("Could not deserialize block response")?
            .result
            .block)
    }

    async fn get_validators_at_height(&self, height: u64) -> anyhow::Result<Vec<String>> {
        let res = self
            .client
            .with_rpc()
            .get(Path::from(format!("/validators?height={}", height)))
            .await
            .context(format!("Could not fetch validators at height {}", height))?;

        let validators_response: ValidatorsResponse = from_str(&res)?;
        Ok(validators_response
            .result
            .validators
            .into_iter()
            .map(|v| v.address)
            .collect())
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
            .bind(Identifier(NAMADA_VALIDATORS_SIGNATURES_TABLE))
            .bind(self.chain_id.as_str())
            .fetch::<NamadaValidatorSignature<'_>>()?;

        let row = cursor.next().await?;

        if let Some(signature) = row {
            return Ok(Some(signature.height));
        } else {
            return Ok(None);
        }
    }

    async fn process_block(&mut self, height: BlockHeight) -> anyhow::Result<()> {
        let block = self.get_block(height).await?;
        let block_height = block
            .header
            .height
            .parse::<u64>()
            .context("Could not parse block height")?;

        // Get validators at this block height
        let addresses = self.get_validators_at_height(block_height).await?;

        let new_addresses: Vec<String> = addresses
            .iter()
            .filter(|addr| !self.validators.iter().any(|v| v == *addr))
            .cloned()
            .collect();

        self.validators.extend(new_addresses.clone());

        self.save_validators(new_addresses).await?;

        // Extract validator signatures from the block's last_commit
        let signed_validators: Vec<String> = block
            .last_commit
            .signatures
            .iter()
            .map(|sig| sig.validator_address.clone())
            .collect();

        // For Namada, we consider a validator "signed" if they are present in the signatures
        let signatures: Vec<(String, bool)> = self
            .validators
            .clone()
            .into_iter()
            .map(|val| (val.clone(), signed_validators.contains(&val)))
            .collect();

        // Parse the block time string to NaiveDateTime
        let timestamp = NaiveDateTime::parse_from_str(&block.header.time, "%Y-%m-%dT%H:%M:%S")
            .or_else(|_| NaiveDateTime::parse_from_str(&block.header.time, "%Y-%m-%dT%H:%M:%S.%fZ"))
            .context("Could not parse block time")?;

        self.save_validators_signatures(signatures, block_height, timestamp)
            .await?;
        Ok(())
    }

    async fn track_uptimes(&mut self) -> anyhow::Result<()> {
        if self.validators.len() == 0 {
            info!("(Namada Uptime Tracker) Obtaining validators");
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
            .parse::<u64>()
            .context("Could not parse block height")?;

        let mut last_processed = self
            .get_last_processed_block_height()
            .await?
            .unwrap_or(last_height - 1);

        while last_processed < last_height {
            last_processed += 1;
            self.process_block(BlockHeight::Height(last_processed as usize))
                .await?;
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
                .set(row.first_seen.timestamp() as i64);
        }

        // Process uptime metrics for different intervals
        let uptime_intervals = vec![30, 15, 7, 1];

        for interval in uptime_intervals {
            let query = format!(
                r#"
                SELECT
                    address,
                    missed,
                    total_blocks - missed AS signed_blocks,
                    100.0 * (total_blocks - missed) / total_blocks AS uptime
                FROM validator_uptime_{}d
                WHERE chain_id = ?
                "#,
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
