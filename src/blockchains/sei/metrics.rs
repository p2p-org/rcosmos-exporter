use prometheus::{register_counter_vec, register_gauge_vec, register_int_gauge_vec, CounterVec, GaugeVec, IntGaugeVec, Registry};

lazy_static::lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref COMETBFT_VALIDATOR: GaugeVec = register_gauge_vec!(
        "rcosmos_cometbft_validator",
        "CometBFT validator information",
        &["validator_address", "chain_id", "network", "alerts"]
    ).unwrap();

    pub static ref COMETBFT_VALIDATOR_VOTING_POWER: GaugeVec = register_gauge_vec!(
        "rcosmos_cometbft_validator_voting_power",
        "CometBFT validator voting power",
        &["validator_address", "chain_id", "network", "alerts"]
    ).unwrap();

    pub static ref COMETBFT_VALIDATOR_PROPOSER_PRIORITY: GaugeVec = register_gauge_vec!(
        "rcosmos_cometbft_validator_proposer_priority",
        "CometBFT validator proposer priority",
        &["validator_address", "chain_id", "network", "alerts"]
    ).unwrap();

    pub static ref COMETBFT_BLOCK_TXS: CounterVec = register_counter_vec!(
        "rcosmos_cometbft_block_txs_total",
        "Total number of transactions in CometBFT blocks",
        &["chain_id", "network"]
    ).unwrap();

    pub static ref COMETBFT_CURRENT_BLOCK_HEIGHT: IntGaugeVec = register_int_gauge_vec!(
        "rcosmos_cometbft_current_block_height",
        "Current CometBFT block height",
        &["chain_id", "network"]
    ).unwrap();
}

pub fn sei_custom_metrics() {
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COMETBFT_VALIDATOR: {}", e));
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_VOTING_POWER.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COMETBFT_VALIDATOR_VOTING_POWER: {}", e));
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_PROPOSER_PRIORITY.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COMETBFT_VALIDATOR_PROPOSER_PRIORITY: {}", e));
    REGISTRY
        .register(Box::new(COMETBFT_BLOCK_TXS.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COMETBFT_BLOCK_TXS: {}", e));
    REGISTRY
        .register(Box::new(COMETBFT_CURRENT_BLOCK_HEIGHT.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COMETBFT_CURRENT_BLOCK_HEIGHT: {}", e));
}
