use lazy_static::lazy_static;
use prometheus::{IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref LOMBARD_LATEST_SESSION_ID: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_lombard_latest_session_id",
            "ID of the latest notary session"
        ),
        &["chain_id", "network"]
    )
    .unwrap();
    pub static ref LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_lombard_validator_signed_latest_session",
            "1 if validator signed in the notary session, 0 if not"
        ),
        &["validator", "session_id", "chain_id", "network", "latest_session"]
    )
    .unwrap();
}

pub fn lombard_custom_metrics() {
    REGISTRY
        .register(Box::new(LOMBARD_LATEST_SESSION_ID.clone()))
        .ok();
    REGISTRY
        .register(Box::new(LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION.clone()))
        .ok();
}
