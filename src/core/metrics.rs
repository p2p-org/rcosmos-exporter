use std::net::SocketAddr;

use hyper::{
    service::{make_service_fn, service_fn},
    Body, Response, Server,
};
use prometheus::{Encoder, TextEncoder};
use tracing::error;

use crate::blockchains::tendermint::metrics::REGISTRY as tendermint_registry;

use super::blockchains::BlockchainType;

///
/// Serves metrics with Prometheus format.
/// To add new Blockchain add its registry at the imports and gather the metrics
///
/// let metric_families = match blockchain_type {
///     BlockchainType::Tendermint => tendermint_registry.gather(),
///     BlockchainType::NEW_BLOCKCHAIN => NEWBLOCKCHAIN_REGISTRY.gather(),
/// };
///
pub async fn serve_metrics(
    prometheus_ip: String,
    prometheus_port: String,
    blockchain_type: BlockchainType,
) {
    let addr: SocketAddr = format!("{}:{}", prometheus_ip, prometheus_port)
        .parse()
        .expect("Unable to parse IP and port");

    let make_svc = make_service_fn(|_| async {
        Ok::<_, hyper::Error>(service_fn(|_| async {
            let encoder = TextEncoder::new();
            let mut buffer = Vec::new();

            let metric_families = match blockchain_type {
                BlockchainType::Tendermint => tendermint_registry.gather(),
            };

            encoder.encode(&metric_families, &mut buffer).unwrap();
            Ok::<_, hyper::Error>(
                Response::builder()
                    .status(200)
                    .header("Content-Type", encoder.format_type())
                    .body(Body::from(buffer))
                    .unwrap(),
            )
        }))
    });

    let server = Server::bind(&addr).serve(make_svc);
    if let Err(e) = server.await {
        error!("Error initializing metrics server: {:?}", e);
    }
}
