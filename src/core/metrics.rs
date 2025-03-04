use std::{net::SocketAddr, sync::Arc};

use crate::blockchains::tendermint::metrics::REGISTRY as tendermint_registry;
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Response, Server,
};
use lazy_static::lazy_static;
use prometheus::{CounterVec, Encoder, IntGaugeVec, Opts, Registry, TextEncoder};
use tracing::error;

use super::blockchain::BlockchainType;

lazy_static! {
    pub static ref EXPORTER_REGISTRY: Registry = Registry::new();
    pub static ref EXPORTER_HTTP_REQUESTS: CounterVec = CounterVec::new(
        Opts::new("http_requests", "rcosmos exporter http requests"),
        &["endpoint", "path", "status_code"]
    )
    .unwrap();
    pub static ref EXPORTER_BLOCK_WINDOW: IntGaugeVec = IntGaugeVec::new(
        Opts::new(
            "rcosmos_exporter_block_window",
            "rcosmos exporter block window"
        ),
        &[]
    )
    .unwrap();
}

pub fn register_exporter_metrics() {
    EXPORTER_REGISTRY
        .register(Box::new(EXPORTER_HTTP_REQUESTS.clone()))
        .unwrap();
    EXPORTER_REGISTRY
        .register(Box::new(EXPORTER_BLOCK_WINDOW.clone()))
        .unwrap();
}

///
/// Serves blockchain and exporter metrics with Prometheus format.
///
pub async fn serve_metrics(
    prometheus_ip: String,
    prometheus_port: String,
    blockchain_type: BlockchainType,
    block_window: i64,
) {
    let addr: SocketAddr = format!("{}:{}", prometheus_ip, prometheus_port)
        .parse()
        .expect("Unable to parse IP and port");

    let blockchain_arc = Arc::new(blockchain_type);

    register_exporter_metrics();
    EXPORTER_BLOCK_WINDOW
        .with_label_values(&[])
        .set(block_window);

    let make_svc = make_service_fn(|_| {
        let blockchain_type = blockchain_arc.clone();
        async move {
            Ok::<_, hyper::Error>(service_fn(move |_| {
                let blockchain_type = blockchain_type.clone();
                async move {
                    let encoder = TextEncoder::new();
                    let mut buffer = Vec::new();

                    let mut metric_families = match *blockchain_type {
                        BlockchainType::Tendermint => tendermint_registry.gather(),
                        BlockchainType::Mezo => tendermint_registry.gather(),
                    };

                    metric_families.extend(EXPORTER_REGISTRY.gather());

                    encoder.encode(&metric_families, &mut buffer).unwrap();
                    Ok::<_, hyper::Error>(
                        Response::builder()
                            .status(200)
                            .header("Content-Type", encoder.format_type())
                            .body(Body::from(buffer))
                            .unwrap(),
                    )
                }
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);
    if let Err(e) = server.await {
        error!("Error initializing metrics server: {:?}", e);
    }
}
