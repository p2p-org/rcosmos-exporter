use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct QueryParams {
    pub metric_name: String,
    pub start_time: u64,
    pub raw_data: bool,
}

#[derive(Deserialize)]
pub struct ActiveKeysResponse {
    period: usize,
    pub raw_data: Vec<ActiveKeyRawData>,
}

#[derive(Deserialize)]
pub struct ActiveKeyRawData {
    pub key_id: String,
    pub num_signatures: String,
    pub time_bin: String,
}
