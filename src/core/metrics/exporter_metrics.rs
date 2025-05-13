use std::time::Duration;

use chrono::Utc;
use lazy_static::lazy_static;
use prometheus::{GaugeVec, IntGaugeVec, Opts, Registry};

lazy_static! {
    pub static ref EXPORTER_REGISTRY: Registry = Registry::new();
    pub static ref EXPORTER_HTTP_REQUESTS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_exporter_http_request",
            "RCosmos Exporter HTTP requests"
        ),
        &["endpoint", "status_code", "network"]
    )
    .unwrap();
    pub static ref EXPORTER_TASK_RUNS: GaugeVec = GaugeVec::new(
        Opts::new("rcosmos_exporter_task_run", "RCosmos Exporter task runs"),
        &["task", "network"]
    )
    .unwrap();
    pub static ref EXPORTER_TASK_ERRORS: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_exporter_task_error",
            "RCosmos Exporter task errors"
        ),
        &["task", "network"]
    )
    .unwrap();
    pub static ref EXPORTER_HEARTBEAT: IntGaugeVec = IntGaugeVec::new(
        Opts::new("rcosmos_exporter_heartbeat", "RCosmos Exporter heartbeat"),
        &["network"]
    )
    .unwrap();
    pub static ref EXPORTER_APP_VERSION_INFO: GaugeVec = GaugeVec::new(
        Opts::new(
            "rcosmos_exporter_version_info",
            "RCosmos Exporter version info"
        ),
        &["version", "commit", "build_date", "network"]
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
        .register(Box::new(EXPORTER_HEARTBEAT.clone()))
        .unwrap();
    EXPORTER_REGISTRY
        .register(Box::new(EXPORTER_APP_VERSION_INFO.clone()))
        .unwrap();
}

pub async fn start_heartbeat(network: String) {
    // Start heartbeat
    tokio::spawn(async move {
        loop {
            EXPORTER_HEARTBEAT
                .with_label_values(&[&network])
                .set(Utc::now().timestamp());
            std::thread::sleep(Duration::from_secs(10));
        }
    });
}

pub fn register_app_version_info(network: String) {
    let version = env!("CARGO_PKG_VERSION");
    let commit = option_env!("GIT_COMMIT_HASH").unwrap_or("unknown");
    let build_date = option_env!("BUILD_DATE").unwrap_or("unknown");

    EXPORTER_APP_VERSION_INFO
        .with_label_values(&[version, commit, build_date, &network])
        .set(0.0);
}
