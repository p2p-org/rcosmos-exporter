use chrono::NaiveDateTime;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ValidatorsResponse {
    pub result: Option<ValidatorsResult>,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorsResult {
    pub count: String,
    pub total: String,
    pub validators: Vec<Validator>,
}

#[derive(Debug, Deserialize)]
pub struct Validator {
    pub address: String,
    pub voting_power: String,
    pub proposer_priority: String,
}

#[derive(Debug, Deserialize)]
pub struct BlockHeader {
    pub height: String,
    #[serde(with = "serde_naive_datetime")]
    pub time: NaiveDateTime,
    pub proposer_address: String,
}

#[derive(Debug, Deserialize)]
pub struct BlockData {
    pub txs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockLastCommit {
    pub signatures: Vec<BlockSignature>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockSignature {
    pub validator_address: String,
}

#[derive(Debug, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub data: BlockData,
    pub last_commit: BlockLastCommit,
}

#[derive(Debug, Deserialize)]
pub struct BlockResponse {
    pub result: BlockResult,
}

#[derive(Debug, Deserialize)]
pub struct BlockResult {
    pub block: Block,
}

#[derive(Debug, Deserialize)]
pub struct TxResponse {
    pub result: TxResponseResult,
}

#[derive(Debug, Deserialize)]
pub struct TxResponseResult {
    pub txs: Vec<Tx>,
    #[serde(default)]
    pub total: Option<String>, // Total count of transactions (for pagination)
}

#[derive(Deserialize, Debug)]
pub struct Tx {
    pub tx_result: TxResult,
}

#[derive(Debug, Deserialize)]
pub struct TxResult {
    pub gas_wanted: String,
    pub gas_used: String,
}

#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    pub result: StatusResult,
}

#[derive(Debug, Deserialize)]
pub struct StatusResult {
    pub node_info: NodeInfo,
    pub sync_info: SyncInfo,
    #[serde(default)]
    #[allow(dead_code)]
    pub validator_info: Option<ValidatorInfo>,
}

#[derive(Debug, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub network: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub version: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub moniker: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub listen_addr: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub protocol_version: Option<ProtocolVersion>,
    #[serde(default)]
    #[allow(dead_code)]
    pub other: Option<NodeOther>,
}

#[derive(Debug, Deserialize)]
pub struct ProtocolVersion {
    #[serde(default)]
    #[allow(dead_code)]
    pub p2p: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub block: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub app: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NodeOther {
    #[serde(default)]
    #[allow(dead_code)]
    pub tx_index: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub rpc_address: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SyncInfo {
    pub catching_up: bool,
    pub latest_block_height: String,
    #[serde(with = "serde_naive_datetime")]
    pub latest_block_time: NaiveDateTime,
    pub earliest_block_height: String,
    #[serde(with = "serde_naive_datetime")]
    pub earliest_block_time: NaiveDateTime,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorInfo {
    #[allow(dead_code)]
    pub address: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub pub_key: Option<ValidatorPubKey>,
    #[serde(default)]
    #[allow(dead_code)]
    pub voting_power: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorPubKey {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub key_type: String,
    #[allow(dead_code)]
    pub value: String,
}

mod serde_naive_datetime {
    use chrono::NaiveDateTime;
    use serde::{self, Deserialize, Deserializer};
    const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.fZ";
    pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        NaiveDateTime::parse_from_str(&s, DATE_FORMAT).map_err(serde::de::Error::custom)
    }
}
