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

#[derive(Debug, Deserialize, Clone)]
pub struct ValidatorDescription {
    pub moniker: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub identity: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub website: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub security_contact: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub details: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AxelarscanValidator {
    pub operator_address: String,
    pub delegator_address: String,
    #[serde(default)]
    pub broadcaster_address: Option<String>,
    pub description: ValidatorDescription,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GetValidatorsResponse {
    pub data: Vec<AxelarscanValidator>,
}
