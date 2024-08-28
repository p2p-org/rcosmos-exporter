use prometheus::{Encoder, TextEncoder, IntCounter, IntGauge, Histogram, HistogramOpts, Registry};
use lazy_static::lazy_static;
use hyper::{Body, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use std::net::SocketAddr;
use crate::{
    config::Settings,
    internal::logger::JsonLog,
    MessageLog,
};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    
    // EndpointManager metrics
    pub static ref HEALTH_CHECK_TOTAL: IntCounter = IntCounter::new("health_check_total", "Total number of health checks run").unwrap();
    pub static ref HEALTHY_ENDPOINTS: IntGauge = IntGauge::new("healthy_endpoints", "Number of healthy endpoints").unwrap();
    pub static ref UNHEALTHY_ENDPOINTS: IntGauge = IntGauge::new("unhealthy_endpoints", "Number of unhealthy endpoints").unwrap();
    pub static ref HEALTH_CHECK_SUCCESS: IntCounter = IntCounter::new("health_check_success", "Number of successful health checks").unwrap();
    pub static ref HEALTH_CHECK_FAILURE: IntCounter = IntCounter::new("health_check_failure", "Number of failed health checks").unwrap();
    pub static ref HEALTH_CHECK_LATENCY: Histogram = Histogram::with_opts(HistogramOpts::new("health_check_latency", "Latency of health check requests")).unwrap();
    pub static ref LAST_HEALTH_CHECK_TIMESTAMP: IntGauge = IntGauge::new("last_health_check_timestamp", "Timestamp of the last health check").unwrap();
    
    // Watcher metrics
    pub static ref BLOCKS_PROCESSED: IntCounter = IntCounter::new("blocks_processed_total", "Total number of blocks processed by the watcher").unwrap();
    pub static ref SIGNATURES_PROCESSED: IntCounter = IntCounter::new("signatures_processed_total", "Total number of signatures processed by the watcher").unwrap();
    pub static ref SIGNATURE_WINDOW_SIZE: IntGauge = IntGauge::new("signature_window_size", "Current size of the signature window").unwrap();
    pub static ref WATCHER_UPTIME: IntGauge = IntGauge::new("watcher_uptime_percentage", "Uptime percentage based on valid signatures in the signature window").unwrap();
    pub static ref BLOCK_FETCH_ERRORS: IntCounter = IntCounter::new("block_fetch_errors_total", "Number of errors encountered while fetching blocks").unwrap();
    pub static ref WATCHER_COMMITTED_HEIGHT: IntGauge = IntGauge::new("watcher_committed_height", "Last committed block height processed by the watcher").unwrap();
}

pub fn register_custom_metrics() {
    REGISTRY.register(Box::new(HEALTH_CHECK_TOTAL.clone())).unwrap();
    REGISTRY.register(Box::new(HEALTHY_ENDPOINTS.clone())).unwrap();
    REGISTRY.register(Box::new(UNHEALTHY_ENDPOINTS.clone())).unwrap();
    REGISTRY.register(Box::new(HEALTH_CHECK_SUCCESS.clone())).unwrap();
    REGISTRY.register(Box::new(HEALTH_CHECK_FAILURE.clone())).unwrap();
    REGISTRY.register(Box::new(HEALTH_CHECK_LATENCY.clone())).unwrap();
    REGISTRY.register(Box::new(LAST_HEALTH_CHECK_TIMESTAMP.clone())).unwrap();

    // Register watcher metrics
    REGISTRY.register(Box::new(BLOCKS_PROCESSED.clone())).unwrap();
    REGISTRY.register(Box::new(SIGNATURES_PROCESSED.clone())).unwrap();
    REGISTRY.register(Box::new(SIGNATURE_WINDOW_SIZE.clone())).unwrap();
    REGISTRY.register(Box::new(WATCHER_UPTIME.clone())).unwrap();
    REGISTRY.register(Box::new(BLOCK_FETCH_ERRORS.clone())).unwrap();
    REGISTRY.register(Box::new(WATCHER_COMMITTED_HEIGHT.clone())).unwrap();
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
