use chrono::NaiveDateTime;

use crate::blockchains::tendermint::tendermint::Tendermint;

use super::blockchain_client::BlockchainClient;

///
/// Different blockchains supported
///
pub enum Blockchain {
    Tendermint(Tendermint),
}

impl Blockchain {
    pub async fn start_monitoring(self) {
        match self {
            Blockchain::Tendermint(tendermint) => {
                tendermint.start_monitoring().await;
            }
        }
    }
}

///
/// If you want to create a exporter for a new blockchain
/// the blockchain struct must implement this trait
///
/// i.e -> impl BlockchainMonitor for Tendermint
///
pub trait BlockchainMonitor: BlockScrapper + ValidatorMetrics {
    async fn start_monitoring(self);
}

pub enum BlockHeight {
    Height(i64),
    Latest,
}

pub trait BlockScrapper {
    type BlockResponse;
    type Error;

    async fn get_chain_id(&self, client: &mut BlockchainClient);
    async fn get_block(
        &self,
        client: &mut BlockchainClient,
        height: BlockHeight,
    ) -> Result<Self::BlockResponse, Self::Error>;
    async fn process_block_window(&self, client: &mut BlockchainClient);
    async fn process_block(&self, client: &mut BlockchainClient, height: i64);
}

pub trait NetworkScrapper {
    async fn get_validators(self, client: &mut BlockchainClient);
    async fn get_proposals(self, client: &mut BlockchainClient);
}

pub trait ValidatorMetrics {
    async fn set_current_block_height(&self, height: i64, chain_id: String);
    async fn set_current_block_time(&self, block_time: NaiveDateTime, chain_id: String);
    async fn set_my_validator_missed_blocks(&self, chain_id: String, validator_address: String);
    async fn set_my_validator_voting_power(&self, chain_id: String, validator_address: String);
    async fn set_my_validator_is_syncing(&self, chain_id: String, validator_address: String);
    async fn set_my_validator_is_jailed(&self, chain_id: String, validator_address: String);
}
