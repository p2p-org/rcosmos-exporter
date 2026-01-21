use lazy_static::lazy_static;
use prometheus::{CounterVec, IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    /// Total number of EVM votes (Yes) cast by a validator across all polls
    pub static ref AXELAR_EVM_VOTES_YES: CounterVec = CounterVec::new(
        Opts::new(
            "rcosmos_axelar_evm_votes_yes",
            "Total number of EVM votes (Yes) cast by a validator"
        ),
        &["validator_address", "chain_id", "network", "alerts", "sender_chain", "recipient_chain"]
    )
    .unwrap();

    /// Total number of EVM votes (No) cast by a validator across all polls
    pub static ref AXELAR_EVM_VOTES_NO: CounterVec = CounterVec::new(
        Opts::new(
            "rcosmos_axelar_evm_votes_no",
            "Total number of EVM votes (No) cast by a validator"
        ),
        &["validator_address", "chain_id", "network", "alerts", "sender_chain", "recipient_chain"]
    )
    .unwrap();

    /// Total number of EVM polls a validator participated in
    pub static ref AXELAR_EVM_VOTES_TOTAL: CounterVec = CounterVec::new(
        Opts::new(
            "rcosmos_axelar_evm_votes_total",
            "Total number of EVM polls a validator participated in"
        ),
        &["validator_address", "chain_id", "network", "alerts", "sender_chain", "recipient_chain"]
    )
    .unwrap();

    /// Number of late EVM votes cast by a validator
    pub static ref AXELAR_EVM_VOTES_LATE: CounterVec = CounterVec::new(
        Opts::new(
            "rcosmos_axelar_evm_votes_late",
            "Number of late EVM votes cast by a validator"
        ),
        &["validator_address", "chain_id", "network", "alerts", "sender_chain", "recipient_chain"]
    )
    .unwrap();

    /// Latest poll height at which a validator voted (gauge, not counter - shows current state)
    pub static ref AXELAR_EVM_VOTES_LATEST_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_axelar_evm_votes_latest_height",
            "Latest poll height at which a validator cast a vote (baseline for tracking progress)"
        ),
        &["validator_address", "chain_id", "network", "alerts"]
    )
    .unwrap();

    /// Latest poll height processed
    pub static ref AXELAR_EVM_POLLS_LATEST_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_axelar_evm_polls_latest_height",
            "Latest EVM poll height processed"
        ),
        &["chain_id", "network"]
    )
    .unwrap();

    /// Total number of polls processed
    pub static ref AXELAR_EVM_POLLS_TOTAL: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_axelar_evm_polls_total",
            "Total number of EVM polls processed"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
}

pub fn axelar_custom_metrics() {
    REGISTRY
        .register(Box::new(AXELAR_EVM_VOTES_YES.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(AXELAR_EVM_VOTES_NO.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(AXELAR_EVM_VOTES_TOTAL.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(AXELAR_EVM_VOTES_LATE.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(AXELAR_EVM_POLLS_LATEST_HEIGHT.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(AXELAR_EVM_POLLS_TOTAL.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(AXELAR_EVM_VOTES_LATEST_HEIGHT.clone()))
        .unwrap();
}
