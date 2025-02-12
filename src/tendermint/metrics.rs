use prometheus::{Encoder, TextEncoder, IntCounter, IntGauge, IntGaugeVec, Registry, GaugeVec, Opts};
use lazy_static::lazy_static;
use hyper::{Body, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use std::net::SocketAddr;
use crate::{config::Settings, MessageLog};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    pub static ref TENDERMINT_CURRENT_BLOCK_HEIGHT: IntGaugeVec = IntGaugeVec::new(
        Opts::new("tendermint_current_block_height", "Current block height of the Tendermint node"),
        &["chain_id"]
    ).unwrap();
    
    pub static ref TENDERMINT_CURRENT_BLOCK_TIME: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_current_block_time", "Current block time of the Tendermint node"),
        &["chain_id"]
    ).unwrap();
    
    pub static ref TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_my_validator_missed_blocks", "Number of blocks missed by my validator"),
        &["address", "chain_id"]
    ).unwrap();
    
    pub static ref TENDERMINT_VALIDATOR_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_validator_missed_blocks", "Number of blocks missed by the validator"),
        &["address", "chain_id"]
    ).unwrap();
    
    pub static ref TENDERMINT_CURRENT_VOTING_POWER: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_current_voting_power", "Current voting power of the validator"),
        &["address", "name", "pub_key", "chain_id"]
    ).unwrap();
    
    pub static ref TENDERMINT_ACTIVE_PROPOSAL: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_active_proposal",
            "Current active proposals with voting period"
        ),
        &["id", "type", "title", "status", "height", "chain_id"]
    ).unwrap();
    
    pub static ref TENDERMINT_UPGRADE_STATUS: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "tendermint_upgrade_status",
            "Indicates whether an upgrade is in progress (1 for upgrade time, 0 otherwise)"
        ),
        &["chain_id"]
    ).unwrap();

    pub static ref TENDERMINT_EXPORTER_LENGTH_SIGNATURES: IntCounter = IntCounter::new(
        "tendermint_exporter_length_signatures_total",
        "Total number of blocks processed by exporter"
    ).unwrap();
    
    pub static ref TENDERMINT_EXPORTER_LENGTH_SIGNATURE_VECTOR: IntGauge = IntGauge::new(
        "tendermint_exporter_length_signature_vector",
        "Total number of blocks processed in vector"
    ).unwrap();
    
    pub static ref TENDERMINT_EXPORTER_RPC_FAILURES: GaugeVec = GaugeVec::new(
        Opts::new(
            "tendermint_exporter_rpc_failures",
            "Number of rpc failures for certain endpoint"
        ),
        &["endpoint"]
    ).unwrap();
}


pub fn register_custom_metrics() {
    REGISTRY.register(Box::new(TENDERMINT_CURRENT_BLOCK_HEIGHT.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_CURRENT_BLOCK_TIME.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_VALIDATOR_MISSED_BLOCKS.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_CURRENT_VOTING_POWER.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_ACTIVE_PROPOSAL.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_UPGRADE_STATUS.clone())).unwrap();

    REGISTRY.register(Box::new(TENDERMINT_EXPORTER_RPC_FAILURES.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_EXPORTER_LENGTH_SIGNATURE_VECTOR.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_EXPORTER_LENGTH_SIGNATURES.clone())).unwrap();
}

pub async fn serve_metrics() {
    let settings = Settings::new().expect("Failed to load settings from environment");

    let addr: SocketAddr = format!("{}:{}", settings.prometheus_ip, settings.prometheus_port)
        .parse()
        .expect("Unable to parse IP and port");

    let make_svc = make_service_fn(|_| async {
        Ok::<_, hyper::Error>(service_fn(|_| async {
            let encoder = TextEncoder::new();
            let metric_families = REGISTRY.gather();
            let mut buffer = Vec::new();
            encoder.encode(&metric_families, &mut buffer).unwrap();
            Ok::<_, hyper::Error>(Response::builder()
                .status(200)
                .header("Content-Type", encoder.format_type())
                .body(Body::from(buffer))
                .unwrap())
        }))
    });

    let server = Server::bind(&addr).serve(make_svc);

    MessageLog!("INFO","Prometheus metrics are being served at http://{}", addr);
    if let Err(e) = server.await {
        MessageLog!("ERROR", "Server error: {}", e);
    }
}
