use lazy_static::lazy_static;
use prometheus::{register_int_gauge_vec, IntGaugeVec, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref COREDAO_VALIDATORS: IntGaugeVec = register_int_gauge_vec!(
        "coredao_validators",
        "CoreDAO validator status (1=active, 0=inactive)",
        &["operator_address", "network"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_JAILED: IntGaugeVec = register_int_gauge_vec!(
        "coredao_validator_jailed",
        "CoreDAO validator jailed status (1=jailed, 0=not jailed)",
        &["operator_address", "network"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_SLASH_COUNT: IntGaugeVec = register_int_gauge_vec!(
        "coredao_validator_slash_count",
        "Number of times a CoreDAO validator has been slashed",
        &["operator_address", "network"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_SLASH_BLOCK: IntGaugeVec = register_int_gauge_vec!(
        "coredao_validator_slash_block",
        "Block height at which a CoreDAO validator was last slashed (0=never slashed)",
        &["operator_address", "network"]
    )
    .unwrap();
    pub static ref COREDAO_BLOCK_SIGNER: IntGaugeVec = register_int_gauge_vec!(
        "coredao_block_signer",
        "CoreDAO block signer information by block number and consensus address",
        &["block_number", "consensus_address", "network"]
    )
    .unwrap();
}

pub fn register_custom_metrics() {
    // ! TODO: Aggregate signing data from all blocks into one metric
    REGISTRY
        .register(Box::new(COREDAO_VALIDATORS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_JAILED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_SLASH_COUNT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_SLASH_BLOCK.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COREDAO_BLOCK_SIGNER.clone()))
        .unwrap();
}
