use lazy_static::lazy_static;
use prometheus::{CounterVec, GaugeVec, IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref TENDERMINT_CURRENT_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_current_block_height",
            "Current block height of the Tendermint chain"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_CURRENT_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_current_block_time",
            "Current block time of the Tendermint chain"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_MISSED_BLOCKS: CounterVec = CounterVec::new(
        Opts::new(
            "tendermint_validator_missed_blocks",
            "Number of blocks missed by validator"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATORS: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_validators", "Validators on the network"),
        &["name", "address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_validator_uptime", "Uptime over block window"),
        &["address", "window", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_PROPOSED_BLOCKS: CounterVec = CounterVec::new(
        Opts::new(
            "tendermint_validator_proposed_blocks",
            "Number of blocks proposed by validator"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_VOTING_POWER: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_validator_voting_power",
            "Voting power by validator"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_PROPOSER_PRIORITY: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_validator_proposer_priority",
            "Proposer priority by validator"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_TOKENS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_tokens",
            "Number of tokens by validator"
        ),
        &["name", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_JAILED: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_validator_jailed", "Jailed status by validator"),
        &["name", "address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref TENDERMINT_UPGRADE_STATUS: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_upgrade_status",
            "Indicates whether an upgrade is in progress (1 for upgrade time, 0 otherwise)"
        ),
        &["id", "type", "title", "status", "height", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_PROPOSALS: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_proposals", "Proposals seen with voting period"),
        &["id", "type", "title", "status", "height", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_UPGRADE_PLAN: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_upgrade_plan", "Upgrade plan"),
        &["name", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_ID: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_node_id", "Node id"),
        &["name", "chain_id", "id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_CATCHING_UP: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_node_catching_up", "Node is catching up"),
        &["name", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_LATEST_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_node_latest_block_height",
            "Node latest block height"
        ),
        &["name", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_LATEST_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_node_latest_block_time",
            "Node latest block time"
        ),
        &["name", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_EARLIEST_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_node_earliest_block_height",
            "Node earliest block height"
        ),
        &["name", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_EARLIEST_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_node_earliest_block_time",
            "Node earliest block time"
        ),
        &["name", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_BLOCK_TXS: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_block_txs", "Block number of transactions"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_BLOCK_TX_SIZE: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_block_tx_size", "Block average transaction size"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_BLOCK_GAS_WANTED: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_block_gas_wanted", "Block gas wanted"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_BLOCK_GAS_USED: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_block_gas_used", "Block gas used"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_BLOCK_TX_GAS_WANTED: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_block_tx_gas_wanted",
            "Block tx average gas wanted"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_BLOCK_TX_GAS_USED: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_block_tx_gas_used", "Block tx average gas used"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_APP_NAME: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_node_app_name", "Node app name"),
        &["name", "chain_id", "network", "app_name"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_APP_VERSION: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_node_app_version", "Node app version"),
        &["name", "chain_id", "network", "version"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_APP_COMMIT: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_node_app_commit", "Node app commit"),
        &["name", "chain_id", "network", "commit"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_COSMOS_SDK_VERSION: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_node_cosmos_sdk_version",
            "Node cosmos sdk version"
        ),
        &["name", "chain_id", "network", "version"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_MONIKER: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_node_moniker", "Node moniker"),
        &["name", "chain_id", "network", "moniker"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_SLASHES: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_slashes",
            "Number of slashes of a validator"
        ),
        &["name", "address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_DELEGATOR_SHARES: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_delegator_share",
            "Delegators share on the validator"
        ),
        &["name", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_DELEGATIONS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_delegations",
            "Number of delegations on the validator"
        ),
        &["name", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_UNBONDING_DELEGATIONS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_unbonding_delegations",
            "Number of unbonding delegations on the validator"
        ),
        &["name", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_REWARDS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_rewards",
            "Rewards obtained by the validator"
        ),
        &["name", "address", "denom", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_COMMISSIONS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_commissions",
            "Commissions obtained by the validator"
        ),
        &["name", "address", "denom", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_COMMISSION_RATE: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_commission_rate",
            "Current commission rate of the validator"
        ),
        &["name", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_COMMISSION_MAX_RATE: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_commission_max_rate",
            "Validator commission max rate"
        ),
        &["name", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_COMMISSION_MAX_CHANGE_RATE: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_commission_max_rate_change",
            "Validator commission max change rate"
        ),
        &["name", "address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_ADDRESS_BALANCE: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_address_balance",
            "Balance of monitored addresses"
        ),
        &["address", "denom", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_FIRST_TIME_SEEN: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_first_time_seen",
            "Validator commission max change rate"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_30DAYS_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_30days_uptime",
            "Validator uptime for 30 days"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_15DAYS_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_15days_uptime",
            "Validator uptime for 15 days"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_7DAYS_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_7days_uptime",
            "Validator uptime for 7 days"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_1DAY_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_1day_uptime",
            "Validator uptime for 1 days"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_30DAYS_SIGNED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_30days_signed_blocks",
            "Validator 30 days signed blocks"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_15DAYS_SIGNED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_15days_signed_blocks",
            "Validator 15 days signed blocks"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_7DAYS_SIGNED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_7days_signed_blocks",
            "Validator 7 days signed blocks"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_1DAY_SIGNED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_1day_signed_blocks",
            "Validator 1 days signed blocks"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_30DAYS_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_30days_missed_blocks",
            "Validator 30 days missed blocks"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_15DAYS_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_15days_missed_blocks",
            "Validator 15 days missed blocks"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_7DAYS_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_7days_missed_blocks",
            "Validator 7 days missed blocks"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_1DAY_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_validator_1day_missed_blocks",
            "Validator 1 days missed blocks"
        ),
        &["address", "chain_id", "network"]
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
        .register(Box::new(TENDERMINT_VALIDATOR_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_UPTIME.clone()))
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
    REGISTRY
        .register(Box::new(TENDERMINT_PROPOSALS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATORS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_UPGRADE_PLAN.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_ID.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_CATCHING_UP.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_LATEST_BLOCK_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_LATEST_BLOCK_TIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_EARLIEST_BLOCK_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_EARLIEST_BLOCK_TIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_BLOCK_TXS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_BLOCK_TX_SIZE.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_BLOCK_GAS_WANTED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_BLOCK_GAS_USED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_BLOCK_TX_GAS_WANTED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_BLOCK_TX_GAS_USED.clone()))
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
        .register(Box::new(TENDERMINT_VALIDATOR_FIRST_TIME_SEEN.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_30DAYS_UPTIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_15DAYS_UPTIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_7DAYS_UPTIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_1DAY_UPTIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_30DAYS_SIGNED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_15DAYS_SIGNED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_7DAYS_SIGNED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_1DAY_SIGNED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_30DAYS_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_15DAYS_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_7DAYS_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_VALIDATOR_1DAY_MISSED_BLOCKS.clone()))
        .unwrap();
}
