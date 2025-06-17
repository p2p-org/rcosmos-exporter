use lazy_static::lazy_static;
use prometheus::{CounterVec, IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref NAMADA_CURRENT_EPOCH: IntGaugeVec = IntGaugeVec::new(
        Opts::new("namada_current_epoch", "Namada current epoch"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref NAMADA_VALIDATOR_MISSING_VOTE: CounterVec = CounterVec::new(
        Opts::new(
            "namada_validator_missing_vote",
            "Namada validators missing vote"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref NAMADA_BLOCK_GAS_USED: IntGaugeVec = IntGaugeVec::new(
        Opts::new("namada_block_gas_used", "Namada block gas used"),
        &["chain_id", "network", "height"]
    )
    .unwrap();
    pub static ref NAMADA_BLOCK_GAS_WANTED: IntGaugeVec = IntGaugeVec::new(
        Opts::new("namada_block_gas_wanted", "Namada block gas wanted"),
        &["chain_id", "network", "height"]
    )
    .unwrap();
    pub static ref NAMADA_CURRENT_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new("namada_current_block_height", "Namada current block height"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref NAMADA_CURRENT_BLOCK_TIME: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "namada_current_block_time",
            "Namada current block time (unix)"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref NAMADA_VALIDATOR_MISSED_BLOCKS: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "namada_validator_missed_blocks",
            "Namada validator missed blocks"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref NAMADA_VALIDATOR_UPTIME: IntGaugeVec = IntGaugeVec::new(
        Opts::new("namada_validator_uptime", "Namada validator uptime"),
        &["address", "chain_id", "network"]
    )
    .unwrap();
}

pub fn register_custom_metrics() {
    REGISTRY
        .register(Box::new(NAMADA_CURRENT_EPOCH.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(NAMADA_VALIDATOR_MISSING_VOTE.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(NAMADA_BLOCK_GAS_USED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(NAMADA_BLOCK_GAS_WANTED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(NAMADA_CURRENT_BLOCK_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(NAMADA_CURRENT_BLOCK_TIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(NAMADA_VALIDATOR_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(NAMADA_VALIDATOR_UPTIME.clone()))
        .unwrap();
}
