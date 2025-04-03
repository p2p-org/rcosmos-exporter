use lazy_static::lazy_static;
use prometheus::{CounterVec, IntGaugeVec, Opts, Registry};

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
