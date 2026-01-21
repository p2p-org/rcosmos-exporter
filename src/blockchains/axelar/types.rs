use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EVMPollsResponse {
    pub data: Vec<Value>, // Use Value to handle dynamic validator address keys
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Vote {
    pub late: bool,
    pub created_at: u64,
    pub id: String,
    pub voter: String,
    #[serde(rename = "type")]
    pub vote_type: String,
    pub vote: bool,
    pub height: u64,
    pub confirmed: Option<bool>,
}
