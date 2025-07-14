#![allow(unused_variables)]
#![allow(dead_code)]

use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize, Clone)]
pub struct BlockResponse {
    pub jsonrpc: String,
    pub id: i32,
    pub result: BlockResult,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BlockResult {
    pub block_id: BlockId,
    pub block: Block,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BlockId {
    pub hash: String,
    pub parts: BlockParts,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BlockParts {
    pub total: u32,
    pub hash: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Block {
    pub header: BlockHeader,
    pub data: BlockData,
    pub evidence: BlockEvidence,
    pub last_commit: LastCommit,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BlockHeader {
    pub version: BlockVersion,
    #[serde(rename = "chain_id")]
    pub chain_id: String,
    pub height: String,
    pub time: String,
    #[serde(rename = "last_block_id")]
    pub last_block_id: BlockId,
    #[serde(rename = "last_commit_hash")]
    pub last_commit_hash: String,
    #[serde(rename = "data_hash")]
    pub data_hash: String,
    #[serde(rename = "validators_hash")]
    pub validators_hash: String,
    #[serde(rename = "next_validators_hash")]
    pub next_validators_hash: String,
    #[serde(rename = "consensus_hash")]
    pub consensus_hash: String,
    #[serde(rename = "app_hash")]
    pub app_hash: String,
    #[serde(rename = "last_results_hash")]
    pub last_results_hash: String,
    #[serde(rename = "evidence_hash")]
    pub evidence_hash: String,
    #[serde(rename = "proposer_address")]
    pub proposer_address: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BlockVersion {
    pub block: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BlockData {
    pub txs: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BlockEvidence {
    pub evidence: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LastCommit {
    pub height: String,
    pub round: u32,
    #[serde(rename = "block_id")]
    pub block_id: BlockId,
    pub signatures: Vec<CommitSignature>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CommitSignature {
    #[serde(rename = "block_id_flag")]
    pub block_id_flag: u32,
    #[serde(rename = "validator_address")]
    pub validator_address: String,
    pub timestamp: String,
    pub signature: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ValidatorsResponse {
    pub jsonrpc: String,
    pub id: i32,
    pub result: ValidatorsResult,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ValidatorsResult {
    pub validators: Vec<Validator>,
    pub block_height: String,
    pub count: String,
    pub total: String,
}

fn option_u32_from_string_or_null<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    match opt {
        Some(serde_json::Value::Number(n)) => n
            .as_u64()
            .map(|v| v as u32)
            .ok_or_else(|| serde::de::Error::custom("Invalid number for rank"))
            .map(Some),
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
    #[serde(rename = "pub_key")]
    pub pub_key: ValidatorPubKey,
    #[serde(rename = "voting_power")]
    pub voting_power: String,
    #[serde(rename = "proposer_priority")]
    pub proposer_priority: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ValidatorPubKey {
    #[serde(rename = "type")]
    pub type_field: String,
    pub value: String,
}

// REST API Validator struct (used by block_scrapper and validator_info_scrapper)
#[derive(Debug, Deserialize, Clone)]
pub struct RestValidator {
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
