use chrono::NaiveDateTime;
use serde::Deserialize;

pub const TIMEOUT: u64 = 5;

#[derive(Debug, Deserialize)]
pub struct ConsensusStateResponse {
    pub result: Option<ConsensusStateResult>,
}

#[derive(Debug, Deserialize)]
pub struct ConsensusStateResult {
    pub round_state: Option<ConsensusStateRoundState>,
}

#[derive(Debug, Deserialize)]
pub struct ConsensusStateRoundState {
    #[serde(rename = "height/round/step")]
    pub height_round_step: String,
    #[serde(with = "serde_naive_datetime")]
    pub start_time: NaiveDateTime,
    pub height_vote_set: Vec<ConsensusHeightVoteSet>,
    pub proposer: ConsensusStateProposer,
}

#[derive(Debug, Deserialize)]
pub struct ConsensusHeightVoteSet {
    pub round: i32,
    pub prevotes: Vec<ConsensusVote>,
    pub precommits: Vec<ConsensusVote>,
    #[serde(rename = "prevotes_bit_array")]
    pub prevotes_bit_array: ConsensusVoteBitArray,
    #[serde(rename = "precommits_bit_array")]
    pub precommits_bit_array: ConsensusVoteBitArray,
}

#[derive(Debug, Deserialize)]
pub struct ConsensusStateProposer {
    pub address: String,
    pub index: i32,
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct ConsensusVote(String);

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct ConsensusVoteBitArray(String);

#[derive(Debug, Deserialize)]
pub struct ValidatorsResponse {
    pub result: Option<ValidatorsResult>,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorsResult {
    pub count: String,
    pub total: String,
    pub validators: Vec<TendermintValidator>,
}

#[derive(Debug, Deserialize)]
pub struct TendermintValidator {
    pub address: String,
    pub voting_power: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintStatusResponse {
    pub result: TendermintStatusResult,
}

#[derive(Debug, Deserialize)]
pub struct TendermintStatusResult {
    pub node_info: TendermintNodeInfo,
    pub sync_info: TendermintSyncInfo,
}

#[derive(Debug, Deserialize)]
pub struct TendermintNodeInfo {
    pub version: String,
    pub network: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintSyncInfo {
    pub latest_block_height: i64,
    pub latest_block_time: BlockTime,
    pub catching_up: bool,
}

#[derive(Debug, Deserialize)]
pub struct BlockTime(String);
impl BlockTime {
    pub fn timestamp(&self) -> Result<i64, chrono::ParseError> {
        let naive_datetime = NaiveDateTime::parse_from_str(&self.0, "%Y-%m-%dT%H:%M:%S")?;
        Ok(naive_datetime.and_utc().timestamp())
    }
}

#[derive(Debug, Deserialize)]
pub struct TendermintBlockResponse {
    pub result: TendermintBlockResult,
}

#[derive(Debug, Deserialize)]
pub struct TendermintBlockResult {
    pub block: TendermintBlock,
}

#[derive(Debug, Deserialize)]
pub struct TendermintBlock {
    pub header: TendermintBlockHeader,
    pub data: TendermintBlockData,
    pub last_commit: TendermintBlockLastCommit,
}

#[derive(Debug, Deserialize)]
pub struct TendermintBlockData {
    pub txs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TendermintBlockLastCommit {
    pub height: String,
    pub signatures: Vec<TendermintBlockSignature>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TendermintBlockSignature {
    pub validator_address: String,
    pub signature: Option<String>,
    #[serde(with = "serde_naive_datetime")]
    pub timestamp: NaiveDateTime,
}

#[derive(Debug, Deserialize)]
pub struct TendermintBlockHeader {
    pub height: String,
    #[serde(with = "serde_naive_datetime")]
    pub time: NaiveDateTime,
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