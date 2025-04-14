use lazy_static::lazy_static;
use prometheus::{CounterVec, IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref BABYLON_CURRENT_EPOCH: IntGaugeVec = IntGaugeVec::new(
        Opts::new("babylon_current_epoch", "Babylon current epoch"),
        &["chain_id"]
    )
    .unwrap();
    pub static ref BABYLON_VALIDATOR_MISSING_BLS_VOTE: CounterVec = CounterVec::new(
        Opts::new(
            "babylon_validator_missing_bls_vote",
            "Babylon validators missing BLS vote"
        ),
        &["address", "chain_id"]
    )
    .unwrap();
    // pub static ref BABYLON_CUBE_SIGNER_SIGNATURES: IntGaugeVec = IntGaugeVec::new(
    //     Opts::new(
    //         "babylon_cube_signer_signatures",
    //         "Babylon validators missing BLS vote"
    //     ),
    //     &["key_id", "chain_id"]
    // )
    // .unwrap();
}

pub fn register_custom_metrics() {
    REGISTRY
        .register(Box::new(BABYLON_CURRENT_EPOCH.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(BABYLON_VALIDATOR_MISSING_BLS_VOTE.clone()))
        .unwrap();
    // REGISTRY
    //     .register(Box::new(BABYLON_CUBE_SIGNER_SIGNATURES.clone()))
    //     .unwrap();
}
