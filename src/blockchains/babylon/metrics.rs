use lazy_static::lazy_static;
use prometheus::{CounterVec, IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref BABYLON_CURRENT_EPOCH: IntGaugeVec = IntGaugeVec::new(
        Opts::new("rcosmos_babylon_current_epoch", "Babylon current epoch"),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref BABYLON_VALIDATOR_MISSING_BLS_VOTE: CounterVec = CounterVec::new(
        Opts::new(
            "rcosmos_babylon_validator_missing_bls_vote",
            "Babylon validators missing BLS vote"
        ),
        &["address", "chain_id", "network", "alerts"]
    )
    .unwrap();
}

pub fn babylon_custom_metrics() {
    REGISTRY
        .register(Box::new(BABYLON_CURRENT_EPOCH.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(BABYLON_VALIDATOR_MISSING_BLS_VOTE.clone()))
        .unwrap();
}
