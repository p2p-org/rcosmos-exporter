use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use lazy_static::lazy_static;

use serde::ser::StdError;

use crate::{
    config,
    MessageLog,
    internal::logger::JsonLog,
    tendermint::types::*,
    tendermint::rpc::*,
};


#[derive(Debug, Clone)]
pub struct Watcher {
    rpc_client: Option<Arc<RPC>>,
    pub validator_address: String,
    pub signatures: Arc<Mutex<VecDeque<(u64, Option<TendermintBlockSignature>)>>>,
    pub commited_height: u64,
}

lazy_static! {
    pub static ref WATCHER_CLIENT: Mutex<Option<Arc<Watcher>>> = Mutex::new(None);
}
pub const BLOCK_WINDOW: i64 = 500;

pub async fn initialize_watcher_client() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Settings::new()?;
    let watcher = Watcher::new(config.into()).await?;
    let watcher_client = Arc::new(watcher);

    let mut watcher_client_guard = WATCHER_CLIENT.lock().unwrap();
    *watcher_client_guard = Some(watcher_client);

    Ok(())
}

impl Watcher {
    pub async fn new(config: Arc<config::Settings>) -> Result<Self, Box<dyn StdError>> {
        let signatures = Arc::new(Mutex::new(VecDeque::with_capacity(BLOCK_WINDOW.try_into().unwrap())));
        let watcher = Watcher {
            rpc_client: RPC_CLIENT.lock().unwrap().clone(),
            validator_address: config.validator_address.clone(),
            signatures: Arc::clone(&signatures),
            commited_height: 0,
        };
        let mut watcher_clone = watcher.clone();

        tokio::spawn(async move {
                watcher_clone.update_signatures().await;
        });

        Ok(watcher)
    }

    pub async fn update_signatures(&mut self) {
        loop {
            if let Some(rpc_client) = &self.rpc_client {
                let rpc_client = Arc::clone(rpc_client);
                match rpc_client.get_block(0).await {
                    Err(_) => {
                        MessageLog!("Error: Failed to get last block.");
                    },
                    Ok(block) => {
                        if let Ok(commited_height) = block.result.block.last_commit.height.parse::<u64>() {
                            if commited_height - self.commited_height >= 1 {
                                self.commited_height = commited_height;
                            }
                            let signature = block.result.block.last_commit.signatures.into_iter()
                                    .find(|sig| sig.validator_address == self.validator_address);
                            let mut signatures = self.signatures.lock().expect("Failed to acquire lock");
                            let len = signatures.len();
                            if len >= BLOCK_WINDOW.try_into().unwrap() {
                                MessageLog!("I have achieved a window lock: {}", len);
                                signatures.pop_front();
                            }
                            MessageLog!("Commited height: {}, current length of signatures: {}", commited_height, len);
                            signatures.push_back((self.commited_height, signature));
                            drop(signatures);
                        }
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(TIMEOUT)).await;
        }
    }

    pub fn get_uptime(&self) -> f64 {
        let signatures = self.signatures.lock().expect("Failed to acquire lock");
        let len = signatures.len();
        MessageLog!("Signatures: {:?}", len);
        let mut count = 0;
        for (_, signature) in &*signatures {
            if signature.is_some() {
                count += 1;
            }
        }
        drop(signatures);
        if len > 0 {
            count as f64 / len as f64 * 100.0
        } else {
            0.0
        }
    }

}