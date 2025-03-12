use lazy_static::lazy_static;
use prometheus::{GaugeVec, IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref TENDERMINT_CURRENT_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_current_block_height",
            "Current block height of the Tendermint chain"
        ),
        &["chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_CURRENT_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_current_block_time",
            "Current block time of the Tendermint chain"
        ),
        &["chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_validator", "Active validators (rpc call)"),
        &["name", "address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_MISSED_BLOCKS: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_validator_missed_blocks",
            "Number of blocks missed by validator"
        ),
        &["name", "address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_PROPOSED_BLOCKS: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_validator_proposed_blocks",
            "Number of blocks proposed by validator"
        ),
        &["name", "address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_VOTING_POWER: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_validator_voting_power",
            "Voting power by validator"
        ),
        &["name", "address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_PROPOSER_PRIORITY: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_validator_proposer_priority",
            "Proposer priority by validator"
        ),
        &["name", "address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_TOKENS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_tokens",
            "Number of tokens by validator"
        ),
        &["name", "address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_JAILED: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_validator_jailed", "Jailed status by validator"),
        &["name", "address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_UPGRADE_STATUS: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_validator_jailed", "Chain upgrade status"),
        &["id", "type", "title", "status", "height", "chain_id"]
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
        .register(Box::new(TENDERMINT_VALIDATOR.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_PROPOSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_VOTING_POWER.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_PROPOSER_PRIORITY.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_TOKENS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_JAILED.clone()))
        .unwrap();
}
