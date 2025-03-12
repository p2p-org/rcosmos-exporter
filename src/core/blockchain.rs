use chrono::NaiveDateTime;

use crate::blockchains::{mezo::mezo::Mezo, tendermint::tendermint::Tendermint};

///
/// Different blockchains supported
///
pub enum Blockchain {
    Tendermint(Tendermint),
    Mezo(Mezo),
}

impl Blockchain {
    pub async fn start_monitoring(self) {
        match self {
            Blockchain::Tendermint(tendermint) => {
                tendermint.start_monitoring().await;
            }
            Blockchain::Mezo(mezo) => {
                mezo.start_monitoring().await;
            }
        }
    }
}

///
/// Blockchains to be read from the .env file
///
#[derive(Debug, Clone, Copy)]
pub enum BlockchainType {
    Tendermint,
    Mezo,
}

///
/// Blockchains to be read from the .env file
///
impl BlockchainType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tendermint" => Some(BlockchainType::Tendermint),
            "mezo" => Some(BlockchainType::Mezo),
            _ => None,
        }
    }
}

pub trait BlockchainMonitor {
    async fn start_monitoring(self);
}

pub enum BlockHeight {
    Height(i64),
    Latest,
}

pub trait BlockScrapper {
    type BlockResponse;
    type Error;

    async fn get_chain_id(&mut self) -> bool;
    async fn get_block(&mut self, height: BlockHeight) -> Result<Self::BlockResponse, Self::Error>;
    async fn process_block_window(&mut self);
    async fn process_block(&mut self, height: i64);
}

pub trait NetworkScrapper {
    type RpcValidator;
    type RestValidator;
    type Proposal;

    async fn get_rpc_validators(&self, path: &str) -> Vec<Self::RpcValidator>;
    async fn get_rest_validators(&self, path: &str) -> Vec<Self::RestValidator>;
    async fn process_validators(&mut self);
    async fn get_proposals(&mut self, path: &str) -> Vec<Self::Proposal>;
    async fn process_proposals(&mut self);
}

pub trait BlockchainMetrics {
    fn set_current_block_height(&self, height: i64);
    fn set_current_block_time(&self, block_time: NaiveDateTime);
    fn set_validator_missed_blocks(&self, name: &str, validator_address: &str);
    fn set_validator_voting_power(&self, name: &str, validator_address: &str, voting_power: i64);
    fn set_validator_proposer_priority(&self, name: &str, validator_address: &str, priority: i64);
    fn set_validator_proposed_blocks(&self, name: &str, validator_address: &str);
    fn set_validator_tokens(&self, name: &str, validator_address: &str, amount: f64);
    fn set_validator_jailed(&self, name: &str, validator_address: &str, jailed: bool);
    fn set_upgrade_proposal(
        &self,
        id: &str,
        proposal_type: &str,
        status: &str,
        height: i64,
        active: bool,
    );
    fn set_proposal(&self, id: &str, proposal_type: &str, title: &str, status: &str, height: &str);
}
