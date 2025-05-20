use std::{net::SocketAddr, sync::Arc};

use crate::{
    blockchains::babylon::metrics::REGISTRY as babylon_registry,
    blockchains::coredao::metrics::REGISTRY as coredao_registry,
    blockchains::tendermint::metrics::REGISTRY as tendermint_registry,
    core::blockchain::Blockchain,
};
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Response, Server,
};

use prometheus::{Encoder, TextEncoder};
use tracing::error;

use super::exporter_metrics::{register_exporter_metrics, EXPORTER_REGISTRY};

///
/// Serves blockchain and exporter metrics with Prometheus format.
///
pub async fn serve_metrics(prometheus_ip: String, prometheus_port: String, blockchain: Blockchain) {
    let addr: SocketAddr = format!("{}:{}", prometheus_ip, prometheus_port)
        .parse()
        .expect("Unable to parse IP and port");

    let blockchain_arc = Arc::new(blockchain);

    register_exporter_metrics();

    let make_svc = make_service_fn(|_| {
        let blockchain_type = blockchain_arc.clone();
        async move {
            Ok::<_, hyper::Error>(service_fn(move |_| {
                let blockchain_type = blockchain_type.clone();
                async move {
                    let encoder = TextEncoder::new();
                    let mut buffer = Vec::new();

                    let mut metric_families = match *blockchain_type {
                        Blockchain::Tendermint => tendermint_registry.gather(),
                        Blockchain::Mezo => tendermint_registry.gather(),
                        Blockchain::CoreDao => coredao_registry.gather(),
                        Blockchain::Namada => tendermint_registry.gather(),
                        Blockchain::Babylon => {
                            let mut families = Vec::new();
                            families.extend(babylon_registry.gather());
                            families.extend(tendermint_registry.gather());
                            families
                        }
                        Blockchain::Lombard => {
                            let mut families = Vec::new();
                            families.extend(tendermint_registry.gather());
                            families
                        }
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
