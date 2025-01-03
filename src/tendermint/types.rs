use std::fmt;

use chrono::NaiveDateTime;
use serde::Deserialize;

pub const DEFAULT_ESTIMATED_BLOCK_TIME: f64 = 6.1;

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
    pub prevotes: Vec<String>,
    pub precommits: Vec<String>,
    #[serde(rename = "prevotes_bit_array")]
    pub prevotes_bit_array: String,
    #[serde(rename = "precommits_bit_array")]
    pub precommits_bit_array: String,
}

#[derive(Debug, Deserialize)]
pub struct ConsensusStateProposer {
    pub address: String,
    pub index: i32,
}

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
pub struct PubKey {
    #[serde(rename = "type")]
    pub key_type: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintValidator {
    pub address: String,
    pub pub_key: PubKey,
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
pub struct RpcBlockErrorResponse {
    pub jsonrpc: String,
    pub id: i64,
    pub error: RpcError,
}

#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<String>,
}

impl fmt::Display for RpcBlockErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RPC Error: code = {}, message = {}, data = {}",
            self.error.code,
            self.error.message,
            match &self.error.data {
                Some(data) => data.as_str(),
                None => "None",
            }
        )
    }
}

impl std::error::Error for RpcBlockErrorResponse {}

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
    pub chain_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintRESTResponse {
    pub validators: Vec<TendermintRESTValidator>,
    pub pagination: TendermintRESTPagination,
}

#[derive(Debug, Deserialize)]
pub struct TendermintRESTValidator {
    pub operator_address: String,
    pub consensus_pubkey: TendermintRESTConsensusPubKey,
    pub jailed: bool,
    pub status: String,
    pub tokens: String,
    pub delegator_shares: String,
    pub description: TendermintRESTDescription,
    pub unbonding_height: String,
    pub unbonding_time: String,
    pub commission: TendermintRESTCommission,
    pub min_self_delegation: String,
    pub unbonding_on_hold_ref_count: Option<String>,
    pub unbonding_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct TendermintRESTConsensusPubKey {
    #[serde(rename = "@type")]
    pub key_type: String,
    pub key: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintRESTDescription {
    pub moniker: String,
    pub identity: String,
    pub website: String,
    pub security_contact: String,
    pub details: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintRESTCommission {
    pub commission_rates: TendermintRESTCommissionRates,
    pub update_time: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintRESTCommissionRates {
    pub rate: String,
    pub max_rate: String,
    pub max_change_rate: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintRESTPagination {
    pub next_key: Option<String>,
    pub total: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintProposalsResponse {
    pub proposals: Vec<Proposal>,
    pub pagination: TendermintRESTPagination,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProposalStatus {
    ProposalStatusDepositPeriod,
    ProposalStatusVotingPeriod,
    ProposalStatusPassed,
    ProposalStatusRejected,
    ProposalStatusFailed,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub messages: Vec<ProposalMessage>,
    pub status: ProposalStatus,
    pub title: Option<String>,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalMessage {
    #[serde(rename = "@type")]
    pub msg_type: String,
    pub content: Option<ProposalContent>,
    pub authority: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalContent {
    #[serde(rename = "@type")]
    pub content_type: String,
    pub title: String,
    pub description: String,
    pub plan: Option<ProposalPlan>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalPlan {
    pub info: String,
    pub height: String,
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

#[derive(Debug)]
pub struct EndpointError(pub String);

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for EndpointError {}