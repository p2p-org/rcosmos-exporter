use lazy_static::lazy_static;
use prometheus::{CounterVec, GaugeVec, IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref COMETBFT_VALIDATOR: IntGaugeVec = IntGaugeVec::new(
        Opts::new("rcosmos_cometbft_validator", "Validator on the network"),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_VOTING_POWER: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_voting_power",
            "Validator voting power on the network"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_PROPOSER_PRIORITY: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_proposer_priority",
            "Validator proposer priority on the network"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_BLOCK_TXS: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_cometbft_block_txs", "Block number of transactions"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_BLOCK_TX_SIZE: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_block_tx_size",
            "Block average transaction size"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_BLOCK_GAS_WANTED: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_cometbft_block_gas_wanted", "Block gas wanted"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_BLOCK_GAS_USED: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_cometbft_block_gas_used", "Block gas used"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_BLOCK_TX_GAS_WANTED: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_block_tx_gas_wanted",
            "Block tx average gas wanted"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_BLOCK_TX_GAS_USED: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_block_tx_gas_used",
            "Block tx average gas used"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_CURRENT_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_current_block_height",
            "Current block height of the CometBFT chain"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_CURRENT_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_current_block_time",
            "Current block time of the CometBFT chain"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_MISSED_BLOCKS: CounterVec = CounterVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_missed_blocks",
            "Number of blocks missed by validator"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_PROPOSED_BLOCKS: CounterVec = CounterVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_proposed_blocks",
            "Number of blocks proposed by validator"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_BLOCKWINDOW_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_uptime",
            "Uptime over block window"
        ),
        &["address", "window", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_1D_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_cometbft_validator_1d_uptime", "Uptime over 1 day"),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_7D_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_cometbft_validator_7d_uptime", "Uptime over 7 days"),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_15D_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_15d_uptime",
            "Uptime over 15 days"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_30D_UPTIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_30d_uptime",
            "Uptime over 30 days"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_1D_SIGNED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_1d_signed_blocks",
            "Number of blocks signed by validator in the last 1 day"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_7D_SIGNED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_7d_signed_blocks",
            "Number of blocks signed by validator in the last 7 days"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_15D_SIGNED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_15d_signed_blocks",
            "Number of blocks signed by validator in the last 15 days"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_30D_SIGNED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_30d_signed_blocks",
            "Number of blocks signed by validator in the last 30 days"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_1D_TOTAL_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_1d_total_blocks",
            "Number of blocks signed by validator in the last 1 day"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_7D_TOTAL_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_7d_total_blocks",
            "Number of blocks signed by validator in the last 7 days"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_15D_TOTAL_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_15d_total_blocks",
            "Number of blocks signed by validator in the last 15 days"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_30D_TOTAL_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_30d_total_blocks",
            "Number of blocks signed by validator in the last 30 days"
        ),
        &["address", "chain_id", "network"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_1D_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_1d_missed_blocks",
            "Number of blocks missed by validator in the last 1 day"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_7D_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_7d_missed_blocks",
            "Number of blocks missed by validator in the last 7 days"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_15D_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_15d_missed_blocks",
            "Number of blocks missed by validator in the last 15 days"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_VALIDATOR_30D_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_validator_30d_missed_blocks",
            "Number of blocks missed by validator in the last 30 days"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
    pub static ref COMETBFT_NODE_ID: IntGaugeVec = IntGaugeVec::new(
        Opts::new("rcosmos_cometbft_node_id", "CometBFT node ID"),
        &["name", "chain_id", "node_id", "network", "client"]
    )
    .unwrap();
    pub static ref COMETBFT_NODE_CATCHING_UP: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_node_catching_up",
            "CometBFT node catching up status"
        ),
        &["name", "chain_id", "network", "client"]
    )
    .unwrap();
    pub static ref COMETBFT_NODE_LATEST_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_node_latest_block_height",
            "CometBFT node latest block height"
        ),
        &["name", "chain_id", "network", "client"]
    )
    .unwrap();
    pub static ref COMETBFT_NODE_LATEST_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_node_latest_block_time",
            "CometBFT node latest block time"
        ),
        &["name", "chain_id", "network", "client"]
    )
    .unwrap();
    pub static ref COMETBFT_NODE_EARLIEST_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_node_earliest_block_height",
            "CometBFT node earliest block height"
        ),
        &["name", "chain_id", "network", "client"]
    )
    .unwrap();
    pub static ref COMETBFT_NODE_EARLIEST_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_cometbft_node_earliest_block_time",
            "CometBFT node earliest block time"
        ),
        &["name", "chain_id", "network", "client"]
    )
    .unwrap();
}

pub fn cometbft_custom_metrics() {
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_VOTING_POWER.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_PROPOSER_PRIORITY.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_BLOCK_TXS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_BLOCK_TX_SIZE.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_BLOCK_GAS_WANTED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_BLOCK_GAS_USED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_BLOCK_TX_GAS_WANTED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_BLOCK_TX_GAS_USED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_CURRENT_BLOCK_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_CURRENT_BLOCK_TIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_PROPOSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_BLOCKWINDOW_UPTIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_1D_UPTIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_7D_UPTIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_15D_UPTIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_30D_UPTIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_1D_SIGNED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_7D_SIGNED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_15D_SIGNED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_30D_SIGNED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_1D_TOTAL_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_7D_TOTAL_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_15D_TOTAL_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_30D_TOTAL_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_1D_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_7D_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_15D_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_VALIDATOR_30D_MISSED_BLOCKS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_NODE_ID.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_NODE_CATCHING_UP.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_NODE_LATEST_BLOCK_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_NODE_LATEST_BLOCK_TIME.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_NODE_EARLIEST_BLOCK_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(COMETBFT_NODE_EARLIEST_BLOCK_TIME.clone()))
        .unwrap();
}
