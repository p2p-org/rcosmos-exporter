#![allow(unused_variables)]
#![allow(dead_code)]

use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize, Clone)]
pub struct BlockResponse {
    pub block: Block,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Block {
    pub height: u64,
    pub hash: String,
    pub time: String,
    pub proposer_address: String,
    pub txs: Option<Vec<String>>, // base64-encoded txs
}

#[derive(Debug, Deserialize, Clone)]
pub struct ValidatorsResponse {
    pub validators: Vec<Validator>,
    pub block_height: u64,
}

fn option_u32_from_string_or_null<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    match opt {
        Some(serde_json::Value::Number(n)) => n.as_u64().map(|v| v as u32).ok_or_else(|| serde::de::Error::custom("Invalid number for rank")).map(Some),
        Some(serde_json::Value::String(s)) => {
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse::<u32>().map(Some).map_err(serde::de::Error::custom)
            }
        }
        Some(serde_json::Value::Null) | None => Ok(None),
        _ => Ok(None),
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Validator {
    pub address: String,
    #[serde(rename = "votingPower")]
    pub voting_power: Option<String>,
    #[serde(rename = "maxCommission")]
    pub max_commission: Option<String>,
    pub commission: Option<String>,
    pub state: Option<String>,
    pub name: Option<String>,
    pub email: Option<String>,
    pub website: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "discordHandle")]
    pub discord_handle: Option<String>,
    pub avatar: Option<String>,
    #[serde(rename = "validatorId")]
    pub validator_id: Option<String>,
    #[serde(deserialize_with = "option_u32_from_string_or_null")]
    pub rank: Option<u32>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EpochResponse {
    pub epoch: Epoch,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Epoch {
    pub epoch_number: u64,
    pub first_block_height: u64,
    pub last_block_time: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BlockTxsResponse {
    pub txs: Vec<Tx>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Tx {
    pub hash: String,
    pub height: u64,
    pub index: u32,
    pub code: u32,
    pub data: Option<String>,
    pub log: Option<String>,
    pub info: Option<String>,
    pub gas_wanted: Option<String>,
    pub gas_used: Option<String>,
    pub events: Option<Vec<TxEvent>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TxEvent {
    pub type_field: String,
    pub attributes: Vec<TxAttribute>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TxAttribute {
    pub key: String,
    pub value: String,
}
