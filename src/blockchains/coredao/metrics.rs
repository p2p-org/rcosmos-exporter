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
        &["operator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_JAILED: IntGaugeVec = register_int_gauge_vec!(
        "rcosmos_coredao_validator_jailed",
        "CoreDAO validator jailed status (1=jailed, 0=not jailed)",
        &["operator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_SLASH_COUNT: IntGaugeVec = register_int_gauge_vec!(
        "rcosmos_coredao_validator_slash_count",
        "Number of times a CoreDAO validator has been slashed",
        &["operator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_SLASH_BLOCK: IntGaugeVec = register_int_gauge_vec!(
        "rcosmos_coredao_validator_slash_block",
        "Block height at which a CoreDAO validator was last slashed (0=never slashed)",
        &["operator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_PARTICIPATION: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validator_participation",
        "Percentage of expected blocks signed by each validator across 3 rotations (100% = 3 blocks, 33.3% = 1 block)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_RECENT_ACTIVITY: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validator_recent_activity",
        "Whether validator has signed at least one block in the last rotation (-1=not enough data yet, 0=no, 1=yes)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_RECENT_ACTIVITY_BLOCK: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validator_recent_activity_block",
        "Most recent block checked for validator activity",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_SIGNED_BLOCKS: CounterVec = register_counter_vec!(
        "rcosmos_coredao_validator_signed_blocks_total",
        "Total number of blocks signed by the target validator",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_UPTIME: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validator_uptime",
        "Historical uptime percentage over configurable block window",
        &["validator_address", "validator_name", "window", "chain_id", "network", "alerts"]
    )
    .unwrap();
    // CORE & BTC Staking Metrics
    pub static ref COREDAO_CORE_VALIDATOR_STAKE_SHARE: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_core_validator_stake_share",
        "Validator's CORE stake amount (absolute value)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_BTC_VALIDATOR_STAKE_SHARE: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_btc_validator_stake_share", 
        "Validator's BTC stake amount (absolute value)",
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
    // Protocol parameters and state
    pub static ref COREDAO_BLOCK_REWARD: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_block_reward",
        "Current block reward (CORE)",
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COREDAO_BLOCK_REWARD_INCENTIVE_PERCENT: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_block_reward_incentive_percent",
        "Block reward incentive percent (0-100)",
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATORSET_TOTAL_INCOME: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validatorset_total_income",
        "Total income pending distribution in ValidatorSet (CORE)",
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATORSET_ACTIVE_VALIDATORS: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_validatorset_active_validators",
        "Number of active validators in current set",
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COREDAO_ROUND_TAG: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_round_tag",
        "Current CoreDAO round tag (epoch index)",
        &["chain_id", "network"]
    )
    .unwrap();

    pub static ref COREDAO_STAKEHUB_SURPLUS: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_stakehub_surplus",
        "StakeHub surplus (CORE)",
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COREDAO_ASSET_STATE_AMOUNT: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_asset_state_amount",
        "StakeHub asset state amount (per asset)",
        &["asset", "chain_id", "network"]
    )
    .unwrap();
    pub static ref COREDAO_ASSET_STATE_FACTOR: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_asset_state_factor",
        "StakeHub asset state factor (per asset)",
        &["asset", "chain_id", "network"]
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
        &["validator_address", "validator_name", "top_delegator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_BTC_VALIDATOR_TOP1_SHARE: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_btc_validator_top1_share",
        "Largest delegator's share of validator's total BTC stake (percentage)",
        &["validator_address", "validator_name", "top_delegator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    
    // Current stake amounts for growth rate calculations (use in Prometheus queries)
    pub static ref COREDAO_CORE_VALIDATOR_CURRENT_STAKE: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_core_validator_current_stake",
        "Current CORE stake amount for validator (for growth rate calculations)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_BTC_VALIDATOR_CURRENT_STAKE: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_btc_validator_current_stake",
        "Current BTC stake amount for validator (for growth rate calculations)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    
    // Delegator APY Metrics (Reward Efficiency)
    pub static ref COREDAO_CORE_VALIDATOR_DELEGATOR_APY: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_core_validator_delegator_apy",
        "Delegator net APY after commission for CORE staking (percentage)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_BTC_VALIDATOR_DELEGATOR_APY: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_btc_validator_delegator_apy",
        "Delegator net APY after commission for BTC staking (percentage)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    
    // Unclaimed Reward Ratio Metrics
    pub static ref COREDAO_CORE_VALIDATOR_UNCLAIMED_REWARD_RATIO: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_core_validator_unclaimed_reward_ratio",
        "Ratio of unclaimed CORE rewards to total staked amount (percentage)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_BTC_VALIDATOR_UNCLAIMED_REWARD_RATIO: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_btc_validator_unclaimed_reward_ratio",
        "Ratio of unclaimed BTC rewards to total staked amount (percentage)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    
    // BTC stake expiration data (for Prometheus time-based calculations)
    pub static ref COREDAO_BTC_VALIDATOR_STAKE_EXPIRATION_TIMESTAMP: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_btc_validator_stake_expiration_timestamp",
        "Timestamp when BTC stake expires (Unix timestamp)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    
    // Event-based cumulative rewards per validator (sum of roundReward amounts)
    pub static ref COREDAO_CORE_VALIDATOR_ROUND_REWARD_TOTAL: CounterVec = register_counter_vec!(
        "rcosmos_coredao_core_validator_round_reward_total",
        "Cumulative CORE rewards distributed to validator for CORE asset (sum of StakeHub roundReward)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_BTC_VALIDATOR_ROUND_REWARD_TOTAL: CounterVec = register_counter_vec!(
        "rcosmos_coredao_btc_validator_round_reward_total",
        "Cumulative CORE rewards distributed to validator for BTC asset (sum of StakeHub roundReward)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    
    
    // Slashing event counter (for Prometheus time-based calculations)
    pub static ref COREDAO_VALIDATOR_SLASH_EVENTS_TOTAL: CounterVec = register_counter_vec!(
        "rcosmos_coredao_validator_slash_events_total",
        "Total number of slashing events for validator (counter for time-based queries)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COREDAO_VALIDATOR_PENALTY_AMOUNT_TOTAL: CounterVec = register_counter_vec!(
        "rcosmos_coredao_validator_penalty_amount_total",
        "Total penalty amount for validator (counter for time-based queries)",
        &["validator_address", "validator_name", "chain_id", "network", "alerts"]
    )
    .unwrap();

    // API Status Metrics
    pub static ref COREDAO_STAKING_API_LAST_UPDATE: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_staking_api_last_update",
        "Timestamp of last successful update from CoreDAO staking API (Unix timestamp)",
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COREDAO_STAKING_API_UP: GaugeVec = register_gauge_vec!(
        "rcosmos_coredao_staking_api_up",
        "Whether CoreDAO staking API is accessible (1=up, 0=down)",
        &["chain_id", "network"]
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
    REGISTRY
        .register(Box::new(COREDAO_BLOCK_REWARD.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BLOCK_REWARD: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_BLOCK_REWARD_INCENTIVE_PERCENT.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BLOCK_REWARD_INCENTIVE_PERCENT: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_VALIDATORSET_TOTAL_INCOME.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATORSET_TOTAL_INCOME: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_VALIDATORSET_ACTIVE_VALIDATORS.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATORSET_ACTIVE_VALIDATORS: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_ROUND_TAG.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_ROUND_TAG: {}", e));
    
    REGISTRY
        .register(Box::new(COREDAO_STAKEHUB_SURPLUS.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_STAKEHUB_SURPLUS: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_ASSET_STATE_AMOUNT.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_ASSET_STATE_AMOUNT: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_ASSET_STATE_FACTOR.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_ASSET_STATE_FACTOR: {}", e));
    
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
    
    // Register current stake metrics (for growth rate calculations)
    REGISTRY
        .register(Box::new(COREDAO_CORE_VALIDATOR_CURRENT_STAKE.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_CORE_VALIDATOR_CURRENT_STAKE: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_BTC_VALIDATOR_CURRENT_STAKE.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BTC_VALIDATOR_CURRENT_STAKE: {}", e));
    
    // Register delegator APY metrics
    REGISTRY
        .register(Box::new(COREDAO_CORE_VALIDATOR_DELEGATOR_APY.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_CORE_VALIDATOR_DELEGATOR_APY: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_BTC_VALIDATOR_DELEGATOR_APY.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BTC_VALIDATOR_DELEGATOR_APY: {}", e));
    
    // Register unclaimed reward ratio metrics
    REGISTRY
        .register(Box::new(COREDAO_CORE_VALIDATOR_UNCLAIMED_REWARD_RATIO.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_CORE_VALIDATOR_UNCLAIMED_REWARD_RATIO: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_BTC_VALIDATOR_UNCLAIMED_REWARD_RATIO.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BTC_VALIDATOR_UNCLAIMED_REWARD_RATIO: {}", e));
    
    // Register BTC expiration timestamp metrics
    REGISTRY
        .register(Box::new(COREDAO_BTC_VALIDATOR_STAKE_EXPIRATION_TIMESTAMP.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BTC_VALIDATOR_STAKE_EXPIRATION_TIMESTAMP: {}", e));
    
    // Register event-based reward metrics
    REGISTRY
        .register(Box::new(COREDAO_CORE_VALIDATOR_ROUND_REWARD_TOTAL.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_CORE_VALIDATOR_ROUND_REWARD_TOTAL: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_BTC_VALIDATOR_ROUND_REWARD_TOTAL.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_BTC_VALIDATOR_ROUND_REWARD_TOTAL: {}", e));
    
    // Register slashing/penalty counters
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_SLASH_EVENTS_TOTAL.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATOR_SLASH_EVENTS_TOTAL: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_VALIDATOR_PENALTY_AMOUNT_TOTAL.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_VALIDATOR_PENALTY_AMOUNT_TOTAL: {}", e));

    // Register API status metrics
    REGISTRY
        .register(Box::new(COREDAO_STAKING_API_LAST_UPDATE.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_STAKING_API_LAST_UPDATE: {}", e));
    REGISTRY
        .register(Box::new(COREDAO_STAKING_API_UP.clone()))
        .unwrap_or_else(|e| eprintln!("Error registering COREDAO_STAKING_API_UP: {}", e));
}
