use std::net::SocketAddr;

use crate::blockchains::babylon::metrics::REGISTRY as babylon_registry;
use crate::blockchains::cometbft::metrics::REGISTRY as cometbft_registry;
use crate::blockchains::coredao::metrics::REGISTRY as coredao_registry;
use crate::blockchains::lombard::metrics::REGISTRY as lombard_registry;
use crate::blockchains::tendermint::metrics::REGISTRY as tendermint_registry;

use hyper::{
    service::{make_service_fn, service_fn},
    Body, Response, Server,
};

use prometheus::{Encoder, TextEncoder};
use tracing::{debug, error, info};

use super::exporter_metrics::{register_exporter_metrics, EXPORTER_REGISTRY};

///
/// Serves blockchain and exporter metrics with Prometheus format.
///
pub async fn serve_metrics(prometheus_ip: String, prometheus_port: String, path: String) {
    let addr: SocketAddr = format!("{}:{}", prometheus_ip, prometheus_port)
        .parse()
        .expect("Unable to parse IP and port");

    info!(
        "Starting Prometheus metrics server on http://{}{}",
        addr, path
    );
    register_exporter_metrics();

    let make_svc = make_service_fn(move |_| {
        let path = path.clone();
        async move {
            let path = path.clone();
            Ok::<_, hyper::Error>(service_fn(move |req| {
                let path = path.clone();
                let req_path = req.uri().path().to_string();
                debug!("Metrics server received request: {}", req_path);
                async move {
                    if req_path == path {
                        let encoder = TextEncoder::new();
                        let mut buffer = Vec::new();

                        let mut metric_families = Vec::new();
                        metric_families.extend(cometbft_registry.gather());
                        metric_families.extend(tendermint_registry.gather());
                        metric_families.extend(coredao_registry.gather());
                        metric_families.extend(lombard_registry.gather());
                        metric_families.extend(babylon_registry.gather());
                        metric_families.extend(EXPORTER_REGISTRY.gather());

                        encoder.encode(&metric_families, &mut buffer).unwrap();
                        Ok::<_, hyper::Error>(
                            Response::builder()
                                .status(200)
                                .header("Content-Type", encoder.format_type())
                                .body(Body::from(buffer))
                                .unwrap(),
                        )
                    } else {
                        debug!("Metrics server 404 for path: {}", req_path);
                        Ok::<_, hyper::Error>(
                            Response::builder()
                                .status(404)
                                .body(Body::from("Not Found"))
                                .unwrap(),
                        )
                    }
                }
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);
    if let Err(e) = server.await {
        error!("Error initializing metrics server: {:?}", e);
    }
}
