use lazy_static::lazy_static;
use prometheus::{IntGaugeVec, Opts};
use crate::blockchains::tendermint::metrics::REGISTRY as TENDERMINT_REGISTRY;

lazy_static! {
    pub static ref LOMBARD_VALIDATOR_SIGNATURE_MISSED: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "lombard_validator_signature_missed",
            "1 if validator missed signature in notary session, 0 if not missed"
        ),
        &["validator", "session_id", "network"]
    ).unwrap();
}

pub fn register_ledger_metrics() {
    TENDERMINT_REGISTRY
        .register(Box::new(LOMBARD_VALIDATOR_SIGNATURE_MISSED.clone()))
        .ok();
}
