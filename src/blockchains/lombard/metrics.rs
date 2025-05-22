use crate::blockchains::tendermint::metrics::REGISTRY as TENDERMINT_REGISTRY;
use lazy_static::lazy_static;
use prometheus::{IntGaugeVec, Opts};

lazy_static! {
    pub static ref LOMBARD_LATEST_SESSION_ID: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "lombard_latest_session_id",
            "ID of the latest notary session"
        ),
        &["network"]
    )
    .unwrap();
    pub static ref LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "lombard_validator_signed_latest_session",
            "1 if validator signed in the latest notary session, 0 if not"
        ),
        &["validator", "session_id", "network"]
    )
    .unwrap();
}

pub fn register_ledger_metrics() {
    TENDERMINT_REGISTRY
        .register(Box::new(LOMBARD_LATEST_SESSION_ID.clone()))
        .ok();
    TENDERMINT_REGISTRY
        .register(Box::new(LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION.clone()))
        .ok();
}
