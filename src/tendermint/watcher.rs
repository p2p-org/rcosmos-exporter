use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

use regex::Regex;
use serde::ser::StdError;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::Duration;

use crate::{
    config,
    config::Settings,
    MessageLog,
    tendermint::{
        rpc::RPC_CLIENT,
        rpc::RPC,
        rest::REST_CLIENT,
        rest::REST,
        types::TendermintBlockSignature,
        types::RpcBlockErrorResponse,
        metrics::{
            TENDERMINT_EXPORTER_LENGTH_SIGNATURES,
            TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS,
            TENDERMINT_EXPORTER_LENGTH_SIGNATURE_VECTOR,
            TENDERMINT_VALIDATOR_MISSED_BLOCKS,
            TENDERMINT_CURRENT_BLOCK_HEIGHT,
            TENDERMINT_CURRENT_BLOCK_TIME,
            TENDERMINT_CURRENT_VOTING_POWER,
        }
    }
};

pub static WATCHER_CLIENT: Mutex<Option<Arc<AsyncMutex<Watcher>>>> = Mutex::new(None);

#[derive(Debug, Clone)]
pub struct Watcher {
    // Configuration Fields
    pub validator_address: String,
    pub block_window: u16,

    // Client Instances
    rpc_client: Option<Arc<RPC>>,
    rest_client: Option<Arc<REST>>,

    // State Fields
    pub signatures: Arc<Mutex<VecDeque<(u64, Option<TendermintBlockSignature>)>>>,
    pub commited_height: u64,
    pub discovered_validators: Arc<Mutex<Vec<String>>>,
    pub avg_time_block: f64,
}

impl Watcher {
    pub async fn new(config: Arc<config::Settings>) -> Result<Self, Box<dyn std::error::Error>> {
        let signatures = Arc::new(Mutex::new(VecDeque::with_capacity(config.block_window.into())));
        let discovered_validators = Arc::new(Mutex::new(Vec::new()));

        let watcher = Watcher {
            rpc_client: RPC_CLIENT.lock().unwrap().clone(),
            rest_client: REST_CLIENT.lock().unwrap().clone(),
            validator_address: config.validator_address.clone(),
            signatures,
            commited_height: 0,
            block_window: config.block_window,
            discovered_validators,
            avg_time_block: 6.0,
        };

        Ok(watcher)
    }

    pub async fn update_active_validator_metrics(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if let Some(rest_client) = &self.rest_client {
            let rest_client = Arc::clone(rest_client);
            let active_validators = rest_client.get_active_validators().await?;
            if let Some(rpc_client) = &self.rpc_client {
                let rpc_client = Arc::clone(rpc_client);
                let rpc_validators = rpc_client.get_validators().await?;
                let pubkey_to_address: std::collections::HashMap<String, String> = rpc_validators
                    .into_iter()
                    .map(|validator| (validator.pub_key.value.clone(), validator.address))
                    .collect();
                for validator in active_validators {
                    let pub_key = &validator.consensus_pubkey.key;
                    let name = &validator.description.moniker;
                    let voting_power: f64 = validator.tokens.parse().unwrap_or(0.0);
                    if let Some(address) = pubkey_to_address.get(pub_key) {
                        TENDERMINT_CURRENT_VOTING_POWER
                            .with_label_values(&[address, name, pub_key])
                            .set(voting_power);
                    } else {
                        MessageLog!("INFO", "No matching address found for pub_key: {}", pub_key);
                    }
                }
            } else {
                MessageLog!("ERROR", "RPC client not initialized.");
            }
        } else {
            MessageLog!("ERROR", "REST client not initialized.");
        }
        Ok(())
    }

    pub async fn update_signatures(&mut self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if let Some(rpc_client) = &self.rpc_client {
            let rpc_client = Arc::clone(rpc_client);
            if self.commited_height == 0 {
                let latest_block = rpc_client.get_block(0).await?;
                let latest_block_height = latest_block
                    .result
                    .block
                    .header
                    .height
                    .parse::<u64>()
                    .map_err(|e| {
                        MessageLog!("ERROR", "Failed to parse the latest block height: {:?}", e);
                        e
                    })?;
                TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS
                .with_label_values(&[&self.validator_address])
                .set(0.0);
                TENDERMINT_CURRENT_BLOCK_TIME.set(
                    latest_block
                    .result
                    .block
                    .header
                    .time
                    .and_utc().timestamp() as f64
                );
                self.commited_height = latest_block_height;
            }
            let next_block_height = self.commited_height + 1;

            let prev_block_time = TENDERMINT_CURRENT_BLOCK_TIME.get();
            let specific_block = rpc_client.get_block(next_block_height.try_into().unwrap()).await?;
            let current_block_time = specific_block.result.block.header.time;
            let parsed_height = specific_block.result.block.header.height.parse::<u64>().map_err(|e| {
                MessageLog!("ERROR", "Failed to parse block height: {:?}", e);
                e
            })?;

            TENDERMINT_CURRENT_BLOCK_HEIGHT.set(parsed_height.try_into().unwrap());
            TENDERMINT_CURRENT_BLOCK_TIME.set(current_block_time.and_utc().timestamp() as f64);
            self.commited_height = parsed_height;

            self.avg_time_block = current_block_time.and_utc().timestamp() as f64 - prev_block_time;
            MessageLog!(
                "INFO",
                "Updated committed height to {} with average block time: {:.4}",
                self.commited_height,
                self.avg_time_block,
            );

            let all_signatures = specific_block.result.block.last_commit.signatures;
            let mut found_my_validator = false;
            let mut my_validator_signature: Option<TendermintBlockSignature> = None;
            {
                let mut signatures = self.signatures.lock().expect("Failed to acquire lock");
                let mut discovered_validators = self.discovered_validators.lock().expect("Failed to acquire lock on validators");
    
                for sig in &all_signatures {
                    let validator_address = &sig.validator_address;
                    if !validator_address.is_empty() && !discovered_validators.contains(validator_address) {
                        discovered_validators.push(validator_address.clone());
                        MessageLog!("INFO", "Discovered new validator: {}", validator_address);
                    }
                }
                for validator_address in discovered_validators.iter() {
                    let signed = all_signatures.iter().any(|sig| sig.validator_address == *validator_address);
                    if !signed {
                        MessageLog!(
                            "ERROR",
                            "No matching signature found for validator address: {}.",
                            validator_address
                        );
                        TENDERMINT_VALIDATOR_MISSED_BLOCKS.with_label_values(&[validator_address]).inc();
                    }
                }
                for sig in &all_signatures {
                    if &sig.validator_address == &self.validator_address {
                        found_my_validator = true;
                        my_validator_signature = Some(sig.clone());
                        break;
                    }
                }
                if !found_my_validator {
                    TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS
                        .with_label_values(&[&self.validator_address])
                        .inc();
                }
                if signatures.len() >= self.block_window as usize {
                    signatures.pop_front();
                }
                signatures.push_back((self.commited_height, my_validator_signature));
                TENDERMINT_EXPORTER_LENGTH_SIGNATURES.inc();
                TENDERMINT_EXPORTER_LENGTH_SIGNATURE_VECTOR.set(signatures.len().try_into().unwrap());
            }
        }
        Ok(())
    }    

    pub async fn start_rpc_watcher(watcher: Arc<AsyncMutex<Watcher>>) {
        loop {
            {
                let mut watcher_guard = watcher.lock().await;
                let result = watcher_guard.update_signatures().await;
                drop(watcher_guard);
                if let Err(err) = result {
                    if is_timeout_error(&err) {
                        MessageLog!("ERROR", "Unhealthy endpoint to update signatures: {:?}", err);
                    } else if check_block_err(&err) {
                        let avg_time_block = watcher.lock().await.avg_time_block;
                        MessageLog!("DEBUG", "No new blocks here, wait {:?}s", avg_time_block);
                        tokio::time::sleep(tokio::time::Duration::from_secs_f64(avg_time_block)).await;
                    } else {
                        MessageLog!("ERROR", "Failed to update signatures: {:?}", err);
                    }
                }
            }
        }
    }

    pub async fn start_rest_watcher(watcher: Arc<AsyncMutex<Watcher>>) {
        loop {
            {
                let watcher_guard = watcher.lock().await;
                let result = watcher_guard.update_active_validator_metrics().await;
                drop(watcher_guard);
                if let Err(err) = result {
                    MessageLog!("ERROR", "Failed update voting power of validators: {:?}", err);
                }
            }
            tokio::time::sleep(Duration::from_secs(100)).await;
        }
    }
}

pub fn is_timeout_error(error: &Box<dyn StdError + Send + Sync>) -> bool {
    if let Some(reqwest_error) = error.downcast_ref::<reqwest::Error>() {
        reqwest_error.is_timeout()
    } else {
        false
    }
}

pub fn check_block_err(err: &Box<dyn StdError + Send + Sync>) -> bool {
    let re = Regex::new(r"height (\d+) must be less than or equal to the current blockchain height (\d+)").unwrap();

    if let Some(rpc_error) = err.downcast_ref::<RpcBlockErrorResponse>() {
        if let Some(data) = &rpc_error.error.data {
            if let Some(captures) = re.captures(data) {
                if let (Some(requested_height_str), Some(current_height_str)) = (captures.get(1), captures.get(2)) {
                    let requested_height: i64 = requested_height_str.as_str().parse().unwrap_or(-1);
                    let current_height: i64 = current_height_str.as_str().parse().unwrap_or(-1);

                    return (requested_height - current_height) == 1;
                }
            } else {
                MessageLog!("DEBUG", "Regex pattern did not match the data field.");
            }
        } else {
            MessageLog!("DEBUG", "No data field in RpcError.");
        }
    } else {
        MessageLog!("DEBUG", "Error type: {}", err.to_string());
    }

    false
}

pub fn spawn_watcher(watcher: Arc<AsyncMutex<Watcher>>) {
    let rpc_watcher = Arc::clone(&watcher);
    let rest_watcher = Arc::clone(&watcher);

    tokio::spawn(async move {
        Watcher::start_rpc_watcher(rpc_watcher).await;
    });
    tokio::spawn(async move {
        Watcher::start_rest_watcher(rest_watcher).await;
    });
}

pub async fn initialize_watcher_client() -> Result<Arc<AsyncMutex<Watcher>>, Box<dyn std::error::Error>> {
    let config = Arc::new(config::Settings::new()?);
    let watcher_client = Arc::new(AsyncMutex::new(Watcher::new(config).await?));

    *WATCHER_CLIENT.lock().unwrap() = Some(watcher_client.clone());
    Ok(watcher_client)
}