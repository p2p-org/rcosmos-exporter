use lazy_static::lazy_static;
use prometheus::{GaugeVec, Opts, Registry};

lazy_static! {
    pub static ref EXPORTER_REGISTRY: Registry = Registry::new();
    pub static ref EXPORTER_HTTP_REQUESTS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_exporter_http_request",
            "RCosmos Exporter HTTP requests"
        ),
        &["endpoint", "path", "status_code", "network"]
    )
    .unwrap();
    pub static ref EXPORTER_TASK_RUNS: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_exporter_task_run", "RCosmos Expoter task runs"),
        &["task", "network"]
    )
    .unwrap();
    pub static ref EXPORTER_TASK_ERRORS: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_exporter_task_error", "RCosmos Expoter task errors"),
        &["task", "network"]
    )
    .unwrap();
}

pub fn register_exporter_metrics() {
    EXPORTER_REGISTRY
        .register(Box::new(EXPORTER_HTTP_REQUESTS.clone()))
        .unwrap();
    EXPORTER_REGISTRY
        .register(Box::new(EXPORTER_TASK_RUNS.clone()))
        .unwrap();
    EXPORTER_REGISTRY
        .register(Box::new(EXPORTER_TASK_ERRORS.clone()))
        .unwrap();
}
