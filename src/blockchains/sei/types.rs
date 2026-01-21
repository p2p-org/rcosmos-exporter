use chrono::NaiveDateTime;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SeiValidatorsResponse {
    pub count: String,
    pub total: String,
    pub validators: Vec<SeiValidator>,
}

#[derive(Debug, Deserialize)]
pub struct SeiValidator {
    pub address: String,
    pub voting_power: String,
    pub proposer_priority: String,
}

// ========== Blocks ==========

#[derive(Debug, Deserialize)]
pub struct SeiBlockResponse {
    pub result: SeiBlockResult,
}

#[derive(Debug, Deserialize)]
pub struct SeiBlockResult {
    pub block: SeiBlock,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeiBlock {
    pub header: SeiHeader,
    pub data: SeiBlockData,
    pub last_commit: SeiLastCommit,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeiHeader {
    pub height: String,
    #[serde(with = "serde_naive_datetime")]
    pub time: NaiveDateTime,
    pub proposer_address: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeiBlockData {
    pub txs: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeiLastCommit {
    pub signatures: Vec<SeiSignature>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeiSignature {
    pub validator_address: String,
}

// Some Sei endpoints may return the block directly without a top-level "result"
#[derive(Debug, Deserialize)]
pub struct SeiBlockDirect {
    pub block: SeiBlock,
}

// ========== Tx Search ==========

// Sei tx_search can return two formats:
// 1. With result wrapper: {"result": {"txs": [], "total": "0"}}
// 2. Direct format: {"txs": [], "total_count": "0"}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SeiTxResponse {
    WithResult {
        result: SeiTxResult,
    },
    Direct {
        txs: Vec<SeiTx>,
        #[serde(rename = "total_count")]
        total: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
pub struct SeiTxResult {
    pub txs: Vec<SeiTx>,
    #[serde(default)]
    pub total: Option<String>, // Total count of transactions (for pagination)
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeiTx {
    pub tx_result: Option<SeiTxResultFields>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeiTxResultFields {
    pub gas_wanted: Option<String>,
    pub gas_used: Option<String>,
}

// Match CometBFT time format parsing (e.g., 2025-09-17T12:25:42.140536099Z)
mod serde_naive_datetime {
    use chrono::NaiveDateTime;
    use serde::{self, Deserialize, Deserializer};
    // Accept up to nanoseconds with trailing Z
    const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.fZ";
    pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        NaiveDateTime::parse_from_str(&s, DATE_FORMAT).map_err(serde::de::Error::custom)
    }
}
