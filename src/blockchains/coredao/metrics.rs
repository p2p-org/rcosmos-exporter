use lazy_static::lazy_static;
use prometheus::{
    register_counter_vec, register_gauge_vec, register_int_gauge_vec, CounterVec, GaugeVec,
    IntGaugeVec, Registry,
};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref COREDAO_VALIDATORS: IntGaugeVec = register_int_gauge_vec!(
        "rcosmos_coredao_validators",
        "CoreDAO validator status (1=active, 0=inactive)",
        &["operator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_JAILED: IntGaugeVec = register_int_gauge_vec!(
        "rcosmos_coredao_validator_jailed",
        "CoreDAO validator jailed status (1=jailed, 0=not jailed)",
        &["operator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_SLASH_COUNT: IntGaugeVec = register_int_gauge_vec!(
        "rcosmos_coredao_validator_slash_count",
        "Number of times a CoreDAO validator has been slashed",
        &["operator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_SLASH_BLOCK: IntGaugeVec = register_int_gauge_vec!(
        "rcosmos_coredao_validator_slash_block",
        "Block height at which a CoreDAO validator was last slashed (0=never slashed)",
        &["operator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_PARTICIPATION: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validator_participation",
        "Percentage of expected blocks signed by each validator across 3 rotations (100% = 3 blocks, 33.3% = 1 block)",
        &["validator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_RECENT_ACTIVITY: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validator_recent_activity",
        "Whether validator has signed at least one block in the last rotation (-1=not enough data yet, 0=no, 1=yes)",
        &["validator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_RECENT_ACTIVITY_BLOCK: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validator_recent_activity_block",
        "Most recent block checked for validator activity",
        &["validator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_SIGNED_BLOCKS: CounterVec = register_counter_vec!(
        "rcosmos_coredao_validator_signed_blocks_total",
        "Total number of blocks signed by the target validator",
        &["validator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_UPTIME: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validator_uptime",
        "Historical uptime percentage over configurable block window",
        &["validator_address", "window", "chain_id", "network", "alerts"]
    )
    .unwrap();
    // CORE & BTC Staking Metrics
    pub static ref COREDAO_CORE_VALIDATOR_STAKE_SHARE: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_core_validator_stake_share",
        "Validator's CORE stake as percentage of total network CORE stake",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_BTC_VALIDATOR_STAKE_SHARE: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_btc_validator_stake_share", 
        "Validator's BTC stake as percentage of total network BTC stake",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_CORE_VALIDATOR_STAKE_IN: CounterVec = register_counter_vec!(
        "rcosmos_coredao_core_validator_stake_in_total",
        "Total CORE tokens staked to validator (cumulative counter)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_CORE_VALIDATOR_STAKE_OUT: CounterVec = register_counter_vec!(
        "rcosmos_coredao_core_validator_stake_out_total",
        "Total CORE tokens unstaked from validator (cumulative counter)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_BTC_VALIDATOR_STAKE_IN: CounterVec = register_counter_vec!(
        "rcosmos_coredao_btc_validator_stake_in_total",
        "Total BTC staked to validator (cumulative counter)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_BTC_VALIDATOR_STAKE_OUT: CounterVec = register_counter_vec!(
        "rcosmos_coredao_btc_validator_stake_out_total",
        "Total BTC unstaked from validator (cumulative counter)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_TOTAL_CORE_STAKED: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_total_core_staked",
        "Total CORE tokens staked across all validators",
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COREDAO_TOTAL_BTC_STAKED: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_total_btc_staked",
        "Total BTC staked across all validators",
        &["chain_id", "network"]
    )
    .unwrap();
    // Commission & Competition Metrics
    pub static ref COREDAO_VALIDATOR_COMMISSION: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validator_commission",
        "Validator commission rate (percentage)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_COMMISSION_PEER_MEDIAN: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validator_commission_peer_median",
        "Median commission rate across all active validators (percentage)",
        &["chain_id", "network"]
    )
    .unwrap();
    // Delegator Concentration Metrics
    pub static ref COREDAO_CORE_VALIDATOR_TOP1_SHARE: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_core_validator_top1_share",
        "Largest delegator's share of validator's total CORE stake (percentage)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_BTC_VALIDATOR_TOP1_SHARE: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_btc_validator_top1_share",
        "Largest delegator's share of validator's total BTC stake (percentage)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
}

pub fn coredao_custom_metrics() {
    REGISTRY
        .register(Box::new(COREDAO_VALIDATORS.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATORS: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_JAILED.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATOR_JAILED: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_SLASH_COUNT.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATOR_SLASH_COUNT: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_SLASH_BLOCK.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATOR_SLASH_BLOCK: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_PARTICIPATION.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATOR_PARTICIPATION: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_RECENT_ACTIVITY.clone()))
        .unwrap_or_else(|e| {
            eprintln!("Error registering COREDAO_VALIDATOR_RECENT_ACTIVITY: {}", e)
        });
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_RECENT_ACTIVITY_BLOCK.clone()))
        .unwrap_or_else(|e| {
            eprintln!(
                "Error registering COREDAO_VALIDATOR_RECENT_ACTIVITY_BLOCK: {}",
                e
            )
        });
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_SIGNED_BLOCKS.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATOR_SIGNED_BLOCKS: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_UPTIME.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATOR_UPTIME: {}", e));

    // Register staking metrics
    REGISTRY
        .register(Box::new(COREDAO_CORE_VALIDATOR_STAKE_SHARE.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_CORE_VALIDATOR_STAKE_SHARE: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_BTC_VALIDATOR_STAKE_SHARE.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BTC_VALIDATOR_STAKE_SHARE: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_CORE_VALIDATOR_STAKE_IN.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_CORE_VALIDATOR_STAKE_IN: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_CORE_VALIDATOR_STAKE_OUT.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_CORE_VALIDATOR_STAKE_OUT: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_BTC_VALIDATOR_STAKE_IN.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BTC_VALIDATOR_STAKE_IN: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_BTC_VALIDATOR_STAKE_OUT.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BTC_VALIDATOR_STAKE_OUT: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_TOTAL_CORE_STAKED.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_TOTAL_CORE_STAKED: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_TOTAL_BTC_STAKED.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_TOTAL_BTC_STAKED: {}", e));
    
    // Register commission and concentration metrics
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_COMMISSION.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATOR_COMMISSION: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_COMMISSION_PEER_MEDIAN.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATOR_COMMISSION_PEER_MEDIAN: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_CORE_VALIDATOR_TOP1_SHARE.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_CORE_VALIDATOR_TOP1_SHARE: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_BTC_VALIDATOR_TOP1_SHARE.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BTC_VALIDATOR_TOP1_SHARE: {}", e));
}
