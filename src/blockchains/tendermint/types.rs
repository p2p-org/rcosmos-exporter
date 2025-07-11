#![allow(unused_variables)]
#![allow(dead_code)]

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
    pub proposer_priority: String,
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
    pub id: String,
    pub version: String,
    pub network: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintSyncInfo {
    pub catching_up: bool,
    pub latest_block_height: String,
    #[serde(with = "serde_naive_datetime")]
    pub latest_block_time: NaiveDateTime,
    pub latest_block_hash: String,
    pub earliest_block_hash: String,
    pub earliest_block_height: String,
    #[serde(with = "serde_naive_datetime")]
    pub earliest_block_time: NaiveDateTime,
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
    pub chain_id: String,
    pub proposer_address: String,
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

impl ToString for ProposalStatus {
    fn to_string(&self) -> String {
        match self {
            ProposalStatus::ProposalStatusDepositPeriod => {
                "PROPOSAL_STATUS_DEPOSIT_PERIOD".to_string()
            }
            ProposalStatus::ProposalStatusVotingPeriod => {
                "PROPOSAL_STATUS_VOTING_PERIOD".to_string()
            }
            ProposalStatus::ProposalStatusPassed => "PROPOSAL_STATUS_PASSED".to_string(),
            ProposalStatus::ProposalStatusRejected => "PROPOSAL_STATUS_REJECTED".to_string(),
            ProposalStatus::ProposalStatusFailed => "PROPOSAL_STATUS_FAILED".to_string(),
        }
    }
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
    pub plan: Option<ProposalPlan>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalContent {
    #[serde(rename = "@type")]
    pub content_type: String,
    pub title: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct TendermintUpgradePlanResponse {
    pub plan: Option<TendermintUpgradePlan>,
}

#[derive(Debug, Deserialize)]
pub struct TendermintUpgradePlan {
    pub name: String,
    pub height: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintTxResponse {
    pub result: TendermintTxResponseResult,
}

#[derive(Debug, Deserialize)]
pub struct TendermintTxResponseResult {
    pub txs: Vec<TendermintTx>,
}

#[derive(Deserialize, Debug)]
pub struct TendermintTx {
    pub tx_result: TendermintTxResult,
}

#[derive(Debug, Deserialize)]
pub struct TendermintTxResult {
    pub gas_wanted: String,
    pub gas_used: String,
}

#[derive(Deserialize, Debug)]
pub struct DefaultNodeInfo {
    pub moniker: String,
    pub network: String,
}

#[derive(Deserialize, Debug)]
pub struct TendermintApplicationVersion {
    pub name: String,
    pub app_name: String,
    pub version: String,
    pub cosmos_sdk_version: String,
    pub git_commit: String,
}

#[derive(Deserialize, Debug)]
pub struct TendermintNodeInfoResponse {
    pub default_node_info: DefaultNodeInfo,
    pub application_version: TendermintApplicationVersion,
}

#[derive(Debug, Deserialize)]
pub struct TendermintDelegationRESTResponse {
    pub delegation_responses: Vec<TendermintDelegationResponse>,
    pub pagination: TendermintRESTPagination,
}

#[derive(Debug, Deserialize)]
pub struct TendermintDelegationResponse {
    pub delegation: TendermintDelegation,
    pub balance: TendermintBalance,
}

#[derive(Debug, Deserialize)]
pub struct TendermintDelegation {
    pub delegator_address: String,
    pub validator_address: String,
    pub shares: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintBalance {
    pub denom: String,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintCommissionRESTResponse {
    pub commission: TendermintCommisionResponse,
}

#[derive(Debug, Deserialize)]
pub struct TendermintCommisionResponse {
    pub commission: Vec<TendermintCommision>,
}

#[derive(Debug, Deserialize)]
pub struct TendermintCommision {
    pub denom: String,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintRewardsRESTResponse {
    pub rewards: TendermintRewardsResponse,
}

#[derive(Debug, Deserialize)]
pub struct TendermintRewardsResponse {
    pub rewards: Vec<TendermintReward>,
}

#[derive(Debug, Deserialize)]
pub struct TendermintReward {
    pub denom: String,
    pub amount: String,
}

#[derive(Deserialize, Debug)]
pub struct TendermintSelfBondRewardResponse {
    pub self_bond_rewards: Vec<TendermintReward>,
}

#[derive(Debug, Deserialize)]
pub struct TendermintUnbondingDelegationEntry {
    pub creation_height: String,
    pub completion_time: String,
    pub initial_balance: String,
    pub balance: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintUnbondingDelegation {
    pub delegator_address: String,
    pub validator_address: String,
    pub entries: Vec<TendermintUnbondingDelegationEntry>,
}

#[derive(Debug, Deserialize)]
pub struct TendermintValidatorUnbondingDelegationsResponse {
    pub unbonding_responses: Vec<TendermintUnbondingDelegation>,
    pub pagination: TendermintRESTPagination,
}

#[derive(Debug, Deserialize)]
pub struct TendermintSlash {
    pub validator_period: String,
    pub fraction: String,
}

#[derive(Debug, Deserialize)]
pub struct TendermintValidatorSlashesResponse {
    pub slashes: Vec<TendermintSlash>,
    pub pagination: TendermintRESTPagination,
}
