use prometheus::{Encoder, TextEncoder, IntCounter, IntGauge, Registry, GaugeVec, Opts};
use lazy_static::lazy_static;
use hyper::{Body, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use std::net::SocketAddr;
use crate::{config::Settings, internal::logger::JsonLog, MessageLog};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    pub static ref TENDERMINT_CURRENT_BLOCK_HEIGHT: IntGauge = IntGauge::new("tendermint_current_block_height", "Current block height of the Tendermint node").unwrap();
    pub static ref TENDERMINT_CURRENT_BLOCK_TIME: IntGauge = IntGauge::new("tendermint_current_block_time", "Current block time of the Tendermint node").unwrap();
    pub static ref TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_my_validator_missed_blocks", "Number of blocks missed by my validator"),
        &["address"]
    ).unwrap();
    pub static ref TENDERMINT_VALIDATOR_MISSED_BLOCKS: GaugeVec = GaugeVec::new(
        Opts::new("tendermint_validator_missed_blocks", "Number of blocks missed by the validator"),
        &["address"]
    ).unwrap();
    // pub static ref TENDERMINT_CURRENT_VOTING_POWER: IntGauge = IntGauge::new("tendermint_current_voting_power", "Current voting power of the validator").unwrap();

    pub static ref TENDERMINT_EXPORTER_LENGTH_SIGNATURES: IntCounter = IntCounter::new("tendermint_exporter_length_signatures_total", "Total number of blocks processed by exporter").unwrap();
    pub static ref TENDERMINT_EXPORTER_LENGTH_SIGNATURE_VECTOR: IntGauge = IntGauge::new("tendermint_exporter_length_signature_vector", "Total number of blocks processed in vector").unwrap();
    pub static ref TENDERMINT_EXPORTER_HEALTH_CHECK_REQUESTS: IntCounter = IntCounter::new("tendermint_exporter_health_check_requests_total", "Total number of health check requests made").unwrap();
    pub static ref TENDERMINT_EXPORTER_HEALTH_CHECK_FAILURES: IntCounter = IntCounter::new("tendermint_exporter_health_check_failures_total", "Total number of health check failures").unwrap();
}

pub fn register_custom_metrics() {
    REGISTRY.register(Box::new(TENDERMINT_CURRENT_BLOCK_HEIGHT.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_CURRENT_BLOCK_TIME.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_MY_VALIDATOR_MISSED_BLOCKS.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_VALIDATOR_MISSED_BLOCKS.clone())).unwrap();

    REGISTRY.register(Box::new(TENDERMINT_EXPORTER_HEALTH_CHECK_REQUESTS.clone())).unwrap();
    REGISTRY.register(Box::new(TENDERMINT_EXPORTER_HEALTH_CHECK_FAILURES.clone())).unwrap();
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

    MessageLog!("Prometheus metrics are being served at http://{}", addr);
    if let Err(e) = server.await {
        MessageLog!("Server error: {}", e);
    }
}
