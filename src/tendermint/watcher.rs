use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use tokio::sync::Mutex as AsyncMutex;
use crate::tendermint::rpc::RPC;
use crate::tendermint::types::TendermintBlockSignature;
use crate::config;
use crate::MessageLog;
use crate::internal::logger::JsonLog;
use crate::tendermint::rpc::RPC_CLIENT;

use crate::tendermint::metrics::{
    TENDERMINT_EXPORTER_LENGTH_SIGNATURES,
    TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS,
    TENDERMINT_EXPORTER_LENGTH_SIGNATURE_VECTOR,
    TENDERMINT_VALIDATOR_MISSED_BLOCKS,
    TENDERMINT_CURRENT_BLOCK_HEIGHT,
    TENDERMINT_CURRENT_BLOCK_TIME,
};

#[derive(Debug, Clone)]
pub struct Watcher {
    rpc_client: Option<Arc<RPC>>,
    pub validator_address: String,
    pub signatures: Arc<Mutex<VecDeque<(u64, Option<TendermintBlockSignature>)>>>,
    pub commited_height: u64,
    pub block_window: u16,
    pub discovered_validators: Arc<Mutex<Vec<String>>>,
}

impl Watcher {
    pub async fn new(config: Arc<config::Settings>) -> Result<Self, Box<dyn std::error::Error>> {
        let signatures = Arc::new(Mutex::new(VecDeque::with_capacity(config.block_window.into())));
        let discovered_validators = Arc::new(Mutex::new(Vec::new()));

        let watcher = Watcher {
            rpc_client: RPC_CLIENT.lock().unwrap().clone(),
            validator_address: config.validator_address.clone(),
            signatures,
            commited_height: 0,
            block_window: config.block_window,
            discovered_validators,
        };

        Ok(watcher)
    }

    pub async fn update_signatures(&mut self) -> Result<(), Box<dyn std::error::Error>> {
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
                        MessageLog!("Error: Failed to parse the latest block height: {:?}", e);
                        e
                    })?;
                TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS
                .with_label_values(&[&self.validator_address])
                .set(0.0);
                self.commited_height = latest_block_height;
            }
            let next_block_height = self.commited_height + 1;

            let block = rpc_client.get_block(next_block_height.try_into().unwrap()).await?;
            let current_block_time = block.result.block.header.time;
            let commited_height = block.result.block.header.height.parse::<u64>().map_err(|e| {
                MessageLog!("Error: Failed to parse block height: {:?}", e);
                e
            })?;

            TENDERMINT_CURRENT_BLOCK_HEIGHT.set(commited_height.try_into().unwrap());
            TENDERMINT_CURRENT_BLOCK_TIME.set(current_block_time.and_utc().timestamp());

            let all_signatures = block.result.block.last_commit.signatures;
            let mut found_my_validator = false;
            let mut my_validator_signature: Option<TendermintBlockSignature> = None;

            {
                let mut signatures = self.signatures.lock().expect("Failed to acquire lock");
                let mut discovered_validators = self.discovered_validators.lock().expect("Failed to acquire lock on validators");

                for sig in &all_signatures {
                    let validator_address = &sig.validator_address;
                    if !validator_address.is_empty() && !discovered_validators.contains(validator_address) {
                        discovered_validators.push(validator_address.clone());
                        MessageLog!("Discovered new validator: {}", validator_address);
                    }
                }
                for validator_address in discovered_validators.iter() {
                    let signed = all_signatures.iter().any(|sig| {
                        sig.validator_address == *validator_address
                    });
    
                    if !signed {
                        MessageLog!(
                            "No matching signature found for validator address: {}.",
                            validator_address
                        );
    
                        TENDERMINT_VALIDATOR_MISSED_BLOCKS
                            .with_label_values(&[validator_address])
                            .inc();
                    }
                }
                for sig in &all_signatures {
                    let validator_address = &sig.validator_address;
                    if validator_address == &self.validator_address {
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

                let len = signatures.len();
                if len >= self.block_window.try_into().unwrap() {
                    signatures.pop_front();
                }
                MessageLog!(
                    "Commited height: {}, current length of signatures: {}",
                    commited_height,
                    len
                );
                signatures.push_back((self.commited_height, my_validator_signature));
                TENDERMINT_EXPORTER_LENGTH_SIGNATURES.inc();
                TENDERMINT_EXPORTER_LENGTH_SIGNATURE_VECTOR.set(signatures.len().try_into().unwrap());
            }

            MessageLog!(
                "Processed block at height {}. Waiting for the next update...",
                commited_height
            );
        }

        self.commited_height += 1;
        Ok(())
    }

    pub async fn start_watcher(watcher: Arc<AsyncMutex<Watcher>>) {
        loop {
            {
                let mut watcher_guard = watcher.lock().await;
                match watcher_guard.update_signatures().await {
                    Ok(_) => {}
                    Err(err) => {
                        MessageLog!("Error updating signatures: {:?}", err);
                    }
                }
            }

            // let delay = tokio::time::Duration::from_secs(5);
            // tokio::time::sleep(delay).await;
        }
    }
}

pub fn spawn_watcher(watcher: Arc<AsyncMutex<Watcher>>) {
    tokio::spawn(async move {
        Watcher::start_watcher(watcher).await;
    });
}

pub async fn initialize_watcher_client() -> Result<Arc<AsyncMutex<Watcher>>, Box<dyn std::error::Error>> {
    let config = Arc::new(config::Settings::new()?);
    let watcher = Watcher::new(config).await?;
    let watcher_arc = Arc::new(AsyncMutex::new(watcher));

    Ok(watcher_arc)
}