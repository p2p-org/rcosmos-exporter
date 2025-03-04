use lazy_static::lazy_static;
use prometheus::{GaugeVec, IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref TENDERMINT_CURRENT_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_current_block_height",
            "Current block height of the Tendermint node"
        ),
        &["chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_CURRENT_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_current_block_time",
            "Current block time of the Tendermint node"
        ),
        &["chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_my_validator_missed_blocks",
            "Number of blocks missed by my validator"
        ),
        &["address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_MY_VALIDATOR_VOTING_POWER: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_current_voting_power",
            "Voting power of my validator"
        ),
        &["address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_MY_VALIDATOR_PROPOSER_PRIORITY: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_my_validator_proposer_priority",
            "Proposer priority of my validator"
        ),
        &["address", "chain_id"]
    )
    .unwrap();
}

pub fn register_custom_metrics() {
    REGISTRY
        .register(Box::new(TENDERMINT_CURRENT_BLOCK_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_CURRENT_BLOCK_TIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_MY_VALIDATOR_VOTING_POWER.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_MY_VALIDATOR_PROPOSER_PRIORITY.clone()))
        .unwrap();
}
