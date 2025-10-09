use lazy_static::lazy_static;
use prometheus::{GaugeVec, IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref TENDERMINT_VALIDATOR: IntGaugeVec = IntGaugeVec::new(
        Opts::new("rcosmos_tendermint_validator", "Validator"),
        &["moniker", "address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_TOKENS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_tokens",
            "Number of tokens by validator"
        ),
        &["moniker", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_JAILED: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_jailed",
            "Jailed status by validator"
        ),
        &["moniker", "address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref TENDERMINT_PROPOSAL: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_proposal",
            "Proposals seen with voting period"
        ),
        &[
            "id",
            "status",
            "voting_start_time",
            "voting_end_time",
            "chain_id",
            "network"
        ]
    )
    .unwrap();
    pub static ref TENDERMINT_UPGRADE_PLAN: IntGaugeVec = IntGaugeVec::new(
        Opts::new("rcosmos_tendermint_upgrade_plan", "Upgrade plan"),
        &["name", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_MONIKER: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_tendermint_node_moniker", "Node moniker"),
        &["name", "chain_id", "network", "client", "moniker"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_APP_NAME: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_tendermint_node_app_name", "Node app name"),
        &["name", "chain_id", "network", "client", "app_name"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_APP_VERSION: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_tendermint_node_app_version", "Node app version"),
        &["name", "chain_id", "network", "client", "version"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_APP_COMMIT: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_tendermint_node_app_commit", "Node app commit"),
        &["name", "chain_id", "network", "client", "commit"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_COSMOS_SDK_VERSION: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_node_cosmos_sdk_version",
            "Node cosmos sdk version"
        ),
        &["name", "chain_id", "network", "client", "version"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_SLASHES: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_slashes",
            "Number of slashes of a validator"
        ),
        &["moniker", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_DELEGATOR_SHARES: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_delegator_share",
            "Delegators share on the validator"
        ),
        &["moniker", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_DELEGATIONS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_delegations",
            "Number of delegations on the validator"
        ),
        &["moniker", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_UNBONDING_DELEGATIONS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_unbonding_delegations",
            "Number of unbonding delegations on the validator"
        ),
        &["moniker", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_REWARDS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_rewards",
            "Rewards obtained by the validator"
        ),
        &["moniker", "address", "denom", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_COMMISSIONS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_commissions",
            "Commissions obtained by the validator"
        ),
        &["moniker", "address", "denom", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_COMMISSION_RATE: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_commission_rate",
            "Current commission rate of the validator"
        ),
        &["moniker", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_COMMISSION_MAX_RATE: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_commission_max_rate",
            "Validator commission max rate"
        ),
        &["moniker", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_COMMISSION_MAX_CHANGE_RATE: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_validator_commission_max_rate_change",
            "Validator commission max change rate"
        ),
        &["moniker", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_ADDRESS_BALANCE: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_address_balance",
            "Balance of monitored addresses"
        ),
        &["address", "denom", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_STAKING_PARAM_UNBONDING_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_staking_param_unbonding_time",
            "Staking param: unbonding_time (seconds)"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_STAKING_PARAM_MAX_VALIDATORS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_staking_param_max_validators",
            "Staking param: max_validators"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_STAKING_PARAM_MAX_ENTRIES: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_staking_param_max_entries",
            "Staking param: max_entries"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_STAKING_PARAM_HISTORICAL_ENTRIES: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_staking_param_historical_entries",
            "Staking param: historical_entries"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_STAKING_PARAM_BOND_DENOM: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_staking_param_bond_denom",
            "Staking param: bond_denom (as hash)"
        ),
        &["chain_id", "network", "bond_denom"]
    )
    .unwrap();
    pub static ref TENDERMINT_STAKING_PARAM_MIN_COMMISSION_RATE: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_staking_param_min_commission_rate",
            "Staking param: min_commission_rate"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_STAKING_POOL_BONDED_TOKENS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_staking_pool_bonded_tokens",
            "Staking pool: bonded_tokens"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_STAKING_POOL_NOT_BONDED_TOKENS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_staking_pool_not_bonded_tokens",
            "Staking pool: not_bonded_tokens"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_SLASHING_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_slashing_missed_blocks",
            "Missed blocks counter for validator (slashing)"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_SLASHING_TOMBSTONED: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_slashing_tombstoned",
            "Tombstoned status for validator (slashing)"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_SLASHING_JAILED_UNTIL: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_slashing_jailed_until",
            "Jailed until timestamp for validator (slashing)"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_SLASHING_START_HEIGHT: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_slashing_start_height",
            "Start height for validator (slashing)"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_SLASHING_INDEX_OFFSET: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_slashing_index_offset",
            "Index offset for validator (slashing)"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_SLASHING_PARAM_SIGNED_BLOCKS_WINDOW: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_slashing_param_signed_blocks_window",
            "Slashing param: signed_blocks_window"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_SLASHING_PARAM_MIN_SIGNED_PER_WINDOW: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_slashing_param_min_signed_per_window",
            "Slashing param: min_signed_per_window"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_SLASHING_PARAM_DOWNTIME_JAIL_DURATION: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_slashing_param_downtime_jail_duration",
            "Slashing param: downtime_jail_duration (seconds)"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_SLASHING_PARAM_SLASH_FRACTION_DOUBLE_SIGN: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_slashing_param_slash_fraction_double_sign",
            "Slashing param: slash_fraction_double_sign"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_SLASHING_PARAM_SLASH_FRACTION_DOWNTIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_tendermint_slashing_param_slash_fraction_downtime",
            "Slashing param: slash_fraction_downtime"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
}

pub fn tendermint_custom_metrics() {
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_TOKENS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_JAILED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_PROPOSAL.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_UPGRADE_PLAN.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_APP_NAME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_APP_VERSION.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_APP_COMMIT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_COSMOS_SDK_VERSION.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_MONIKER.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_SLASHES.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_DELEGATOR_SHARES.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_DELEGATIONS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_UNBONDING_DELEGATIONS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_REWARDS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_COMMISSIONS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_COMMISSION_RATE.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_COMMISSION_MAX_RATE.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(
            TENDERMINT_VALIDATOR_COMMISSION_MAX_CHANGE_RATE.clone(),
        ))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_ADDRESS_BALANCE.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_STAKING_PARAM_UNBONDING_TIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_STAKING_PARAM_MAX_VALIDATORS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_STAKING_PARAM_MAX_ENTRIES.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(
            TENDERMINT_STAKING_PARAM_HISTORICAL_ENTRIES.clone(),
        ))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_STAKING_PARAM_BOND_DENOM.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(
            TENDERMINT_STAKING_PARAM_MIN_COMMISSION_RATE.clone(),
        ))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_STAKING_POOL_BONDED_TOKENS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_STAKING_POOL_NOT_BONDED_TOKENS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_SLASHING_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_SLASHING_TOMBSTONED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_SLASHING_JAILED_UNTIL.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_SLASHING_START_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_SLASHING_INDEX_OFFSET.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(
            TENDERMINT_SLASHING_PARAM_SIGNED_BLOCKS_WINDOW.clone(),
        ))
        .unwrap();
    REGISTRY
        .register(Box::new(
            TENDERMINT_SLASHING_PARAM_MIN_SIGNED_PER_WINDOW.clone(),
        ))
        .unwrap();
    REGISTRY
        .register(Box::new(
            TENDERMINT_SLASHING_PARAM_DOWNTIME_JAIL_DURATION.clone(),
        ))
        .unwrap();
    REGISTRY
        .register(Box::new(
            TENDERMINT_SLASHING_PARAM_SLASH_FRACTION_DOUBLE_SIGN.clone(),
        ))
        .unwrap();
    REGISTRY
        .register(Box::new(
            TENDERMINT_SLASHING_PARAM_SLASH_FRACTION_DOWNTIME.clone(),
        ))
        .unwrap();
}
