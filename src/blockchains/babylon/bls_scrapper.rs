use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use base64::{engine::general_purpose, Engine};
use serde_json::from_str;
use tokio::time::sleep;
use tracing::{error, info};

use crate::{
    blockchains::{
        babylon::types::{CurrentEpoch, GetEpochResponse},
        tendermint::types::{TendermintValidator, ValidatorsResponse},
    },
    core::{
        chain_id::ChainId, clients::blockchain_client::BlockchainClient, exporter::Task,
        network::Network,
    },
};

use super::{
    metrics::{BABYLON_CURRENT_EPOCH, BABYLON_VALIDATOR_MISSING_BLS_VOTE},
    types::{BlockTxs, Epoch, Tx},
};

pub struct BabylonBlsScrapper {
    client: Arc<BlockchainClient>,
    processed_epoch: usize,
    chain_id: ChainId,
    network: Network,
    validator_alert_addresses: Vec<String>,
}

impl BabylonBlsScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        chain_id: ChainId,
        network: Network,
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

    async fn get_rpc_validators(&self, path: &str) -> Vec<TendermintValidator> {
        info!("(Babylon BLS Scrapper) Fetching RPC validators");
        let mut validators: Vec<TendermintValidator> = Vec::new();

        let mut all_fetched = false;
        let mut page = 1;
        let mut fetched = 0;

        while !all_fetched {
            let res = match self
                .client
                .with_rpc()
                .get(&format!("{}&page={}", path, page))
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    error!("Error calling to RPC validators endpoint: {}", e);
                    break;
                }
            };

            let fetched_validators: Vec<TendermintValidator> =
                match from_str::<ValidatorsResponse>(&res) {
                    Ok(res) => {
                        if let Some(res) = res.result {
                            if res.count.parse::<usize>().unwrap() + fetched
                                == res.total.parse::<usize>().unwrap()
                            {
                                all_fetched = true;
                            } else {
                                fetched += res.count.parse::<usize>().unwrap();
                                page += 1;
                            }

                            res.validators
                        } else {
                            error!("Result key not present at validators rpc endpoint response");
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error deserializing JSON: {}", e);
                        error!("Raw JSON: {}", res);
                        break;
                    }
                };

            validators.extend(fetched_validators);
        }
        validators
    }

    async fn get_current_epoch(&self) -> anyhow::Result<usize> {
        info!("(Babylon BLS Scrapper) Getting current epoch");
        let res = self
            .client
            .with_rest()
            .get("/babylon/epoching/v1/current_epoch")
            .await?;

        match from_str::<CurrentEpoch>(&res) {
            Ok(res) => Ok(res.current_epoch.parse::<usize>().unwrap()),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_epoch(&self, epoch: usize) -> anyhow::Result<Epoch> {
        info!("(Babylon BLS Scrapper) Getting epoch {}", epoch);
        let res = self
            .client
            .with_rest()
            .get(&format!("/babylon/epoching/v1/epochs/{}", epoch))
            .await?;

        match from_str::<GetEpochResponse>(&res) {
            Ok(res) => Ok(res.epoch),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_block_txs(&self, block: usize) -> anyhow::Result<Vec<Tx>> {
        info!("(Babylon BLS Scrapper) Getting block {} txs", block);
        let res = self
            .client
            .with_rest()
            .get(&format!(
                "/cosmos/tx/v1beta1/txs/block/{}?pagination.limit=1",
                block
            ))
            .await?;

        match from_str::<BlockTxs>(&res) {
            Ok(res) => Ok(res.txs),
            Err(e) => Err(e.into()),
        }
    }

    async fn process_bls(&mut self) {
        let current_epoch = match self.get_current_epoch().await {
            Ok(epoch) => epoch,
            Err(e) => {
                error!("Babylon BLS Scrapper) Could not obtain current epoch");
                error!("Error: {}", e);
                return;
            }
        };

        BABYLON_CURRENT_EPOCH
            .with_label_values(&[&self.chain_id.to_string(), &self.network.to_string()])
            .set(current_epoch as i64);

        let epoch_to_process = current_epoch - 1;

        if epoch_to_process == self.processed_epoch {
            info!("(Babylon BLS Scrapper) Epoch to be processed: {}, has been already processed. Skipping... ", epoch_to_process);
            return;
        }

        info!(
            "(Babylon BLS Scrapper) Processing epoch: {}",
            epoch_to_process
        );

        let last_finalized_epoch = match self.get_epoch(epoch_to_process).await {
            Ok(epoch) => epoch,
            Err(e) => {
                error!(
                    "(Babylon BLS Scrapper) Could not obtain last finalized epoch num {}",
                    epoch_to_process
                );
                error!("Error: {}", e);
                return;
            }
        };

        let epoch_first_block = match last_finalized_epoch.first_block_height.parse::<usize>() {
            Ok(u) => u,
            Err(e) => {
                error!(
                    "Babylon BLS Scrapper) Could not parse usize from epoch first block {}",
                    last_finalized_epoch.first_block_height
                );
                error!("Error: {}", e);
                return;
            }
        };

        let validators = self
            .get_rpc_validators(&format!("/validators?height={}", epoch_first_block))
            .await;

        let block_txs = match self.get_block_txs(epoch_first_block).await {
            Ok(txs) => txs,
            Err(e) => {
                error!(
                    "(Babylon BLS Scrapper) Could not obtain block txs for block: {}",
                    epoch_first_block
                );
                error!("Error: {}", e);
                return;
            }
        };

        let mut validators_missing_block = Vec::new();
        for tx in block_txs {
            for message in tx.body.messages {
                for vote in message.extended_commit_info.votes {
                    if vote.extension_signature.is_none() {
                        let address_bytes = match general_purpose::STANDARD
                            .decode(vote.validator.address)
                        {
                            Ok(b) => b,
                            Err(e) => {
                                error!("(Babylon BLS Scrapper) Could not decode validator addres");
                                error!("Error: {}", e);
                                return;
                            }
                        };
                        let address: String = address_bytes
                            .iter()
                            .map(|byte| format!("{:02x}", byte))
                            .collect();
                        let address = address.to_uppercase();
                        validators_missing_block.push(address);
                    }
                }
            }
        }

        for validator in validators {
            let fires_alerts = self
                .validator_alert_addresses
                .contains(&validator.address)
                .to_string();

            if validators_missing_block.contains(&validator.address) {
                info!(
                    "(Babylon BLS Scrapper) Found validator missing checkpoint: {}",
                    &validator.address
                );
                BABYLON_VALIDATOR_MISSING_BLS_VOTE
                    .with_label_values(&[
                        &validator.address,
                        &self.chain_id.to_string(),
                        &self.network.to_string(),
                        &fires_alerts,
                    ])
                    .inc();
            }
        }

        self.processed_epoch = epoch_to_process;
    }
}

#[async_trait]
impl Task for BabylonBlsScrapper {
    async fn run(&mut self, delay: Duration) {
        info!("Running Babylon BLS Scrapper");

        loop {
            self.process_bls().await;

            sleep(delay).await
        }
    }
}
