use lazy_static::lazy_static;
use prometheus::{CounterVec, GaugeVec, IntGaugeVec, Opts, Registry};

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
    pub static ref TENDERMINT_VALIDATOR_MISSED_BLOCKS: CounterVec = CounterVec::new(
        Opts::new(
            "tendermint_validator_missed_blocks",
            "Number of blocks missed by validator"
        ),
        &["address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATORS: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_validators", "Validators on the network"),
        &["name", "address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_validator_uptime", "Uptime over block window"),
        &["address", "window", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_PROPOSED_BLOCKS: CounterVec = CounterVec::new(
        Opts::new(
            "tendermint_validator_proposed_blocks",
            "Number of blocks proposed by validator"
        ),
        &["address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_VOTING_POWER: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_validator_voting_power",
            "Voting power by validator"
        ),
        &["address", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_VALIDATOR_PROPOSER_PRIORITY: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_validator_proposer_priority",
            "Proposer priority by validator"
        ),
        &["address", "chain_id"]
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
        Opts::new(
            "tendermint_upgrade_status",
            "Indicates whether an upgrade is in progress (1 for upgrade time, 0 otherwise)"
        ),
        &["id", "type", "title", "status", "height", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_PROPOSALS: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_proposals", "Proposals seen with voting period"),
        &["id", "type", "title", "status", "height", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_UPGRADE_PLAN: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_upgrade_plan", "Upgrade plan"),
        &["name", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_ID: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_node_id", "Node id"),
        &["name", "chain_id", "id"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_CATCHING_UP: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_node_catching_up", "Node is catching up"),
        &["name", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_LATEST_BLOCK_HASH: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_node_latest_block_hash",
            "Node latest block hash"
        ),
        &["name", "chain_id", "hash"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_LATEST_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_node_latest_block_height",
            "Node latest block height"
        ),
        &["name", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_LATEST_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_node_latest_block_time",
            "Node latest block time"
        ),
        &["name", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_EARLIEST_BLOCK_HASH: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_node_earliest_block_hash",
            "Node latest block hash"
        ),
        &["name", "chain_id", "hash"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_EARLIEST_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_node_earliest_block_height",
            "Node earliest block height"
        ),
        &["name", "chain_id"]
    )
    .unwrap();
    pub static ref TENDERMINT_NODE_EARLIEST_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_node_earliest_block_time",
            "Node earliest block time"
        ),
        &["name", "chain_id"]
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
        .register(Box::new(TENDERMINT_NODE_LATEST_BLOCK_HASH.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_LATEST_BLOCK_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_LATEST_BLOCK_TIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_EARLIEST_BLOCK_HASH.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_EARLIEST_BLOCK_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(TENDERMINT_NODE_EARLIEST_BLOCK_TIME.clone()))
        .unwrap();
}
