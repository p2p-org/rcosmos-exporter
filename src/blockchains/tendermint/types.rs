#![allow(unused_variables)]
#![allow(dead_code)]

use chrono::NaiveDateTime;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ValidatorsResponse {
    pub validators: Vec<Validator>,
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorsResult {
    pub count: String,
    pub total: String,
    pub validators: Vec<ValidatorSimple>,
}

#[derive(Debug, Deserialize)]
pub struct PubKey {
    #[serde(rename = "type")]
    pub key_type: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorSimple {
    pub address: String,
    pub pub_key: PubKey,
    pub voting_power: String,
    pub proposer_priority: String,
}

#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    pub result: StatusResult,
}

#[derive(Debug, Deserialize)]
pub struct StatusResult {
    pub node_info: NodeInfo,
    pub sync_info: SyncInfo,
}

#[derive(Debug, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub version: String,
    pub network: String,
}

#[derive(Debug, Deserialize)]
pub struct SyncInfo {
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
pub struct BlockResponse {
    pub result: BlockResult,
}

#[derive(Debug, Deserialize)]
pub struct BlockResult {
    pub block: Block,
}

#[derive(Debug, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub data: BlockData,
    pub last_commit: BlockLastCommit,
}

#[derive(Debug, Deserialize)]
pub struct BlockData {
    pub txs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockLastCommit {
    pub height: String,
    pub signatures: Vec<BlockSignature>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockSignature {
    pub validator_address: String,
    pub signature: Option<String>,
    #[serde(with = "serde_naive_datetime")]
    pub timestamp: NaiveDateTime,
}

#[derive(Debug, Deserialize)]
pub struct BlockHeader {
    pub height: String,
    #[serde(with = "serde_naive_datetime")]
    pub time: NaiveDateTime,
    pub chain_id: String,
    pub proposer_address: String,
}

#[derive(Debug, Deserialize)]
pub struct Validator {
    pub operator_address: String,
    pub consensus_pubkey: ValidatorConsensusPubKey,
    pub jailed: bool,
    pub status: String,
    pub tokens: String,
    pub delegator_shares: String,
    pub description: ValidatorDescription,
    pub unbonding_height: String,
    pub unbonding_time: String,
    pub commission: ValidatorCommission,
    pub min_self_delegation: String,
    pub unbonding_on_hold_ref_count: Option<String>,
    pub unbonding_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorConsensusPubKey {
    #[serde(rename = "@type")]
    pub key_type: String,
    pub key: String,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorDescription {
    pub moniker: String,
    pub identity: String,
    pub website: String,
    pub security_contact: String,
    pub details: String,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorCommission {
    pub commission_rates: ValidatorCommissionRates,
    pub update_time: String,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorCommissionRates {
    pub rate: String,
    pub max_rate: String,
    pub max_change_rate: String,
}

#[derive(Debug, Deserialize)]
pub struct Pagination {
    pub next_key: Option<String>,
    pub total: String,
}

#[derive(Debug, Deserialize)]
pub struct StakingParamsResponse {
    pub params: StakingParams,
}

#[derive(Debug, Deserialize)]
pub struct StakingParams {
    pub unbonding_time: String,
    pub max_validators: u64,
    pub max_entries: u64,
    pub historical_entries: u64,
    pub bond_denom: String,
    pub min_commission_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProposalsResponse {
    pub proposals: Vec<Proposal>,
    pub pagination: Pagination,
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

    pub fn option<'de, D>(deserializer: D) -> Result<Option<NaiveDateTime>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<String>::deserialize(deserializer)?;
        match opt {
            Some(s) => NaiveDateTime::parse_from_str(&s, DATE_FORMAT)
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpgradePlanResponse {
    pub plan: Option<UpgradePlan>,
}

#[derive(Debug, Deserialize)]
pub struct UpgradePlan {
    pub name: String,
    pub height: String,
}

#[derive(Debug, Deserialize)]
pub struct TxResponse {
    pub result: TxResponseResult,
}

#[derive(Debug, Deserialize)]
pub struct TxResponseResult {
    pub txs: Vec<Tx>,
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

#[derive(Deserialize, Debug)]
pub struct DefaultNodeInfo {
    pub moniker: String,
    pub network: String,
}

#[derive(Debug, Deserialize)]
pub struct ApplicationVersion {
    pub name: String,
    pub app_name: String,
    pub version: String,
    pub cosmos_sdk_version: String,
    pub git_commit: String,
}

#[derive(Debug, Deserialize)]
pub struct NodeInfoResponse {
    pub default_node_info: DefaultNodeInfo,
    pub application_version: ApplicationVersion,
}

#[derive(Debug, Deserialize)]
pub struct DelegationResponse {
    pub delegation_responses: Vec<Delegation>,
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct Delegation {
    pub delegation: DelegationInfo,
    pub balance: Balance,
}

#[derive(Debug, Deserialize)]
pub struct DelegationInfo {
    pub delegator_address: String,
    pub validator_address: String,
    pub shares: String,
}

#[derive(Debug, Deserialize)]
pub struct Balance {
    pub denom: String,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
pub struct UnbondingDelegationResponse {
    pub unbonding_responses: Vec<UnbondingDelegation>,
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct UnbondingDelegation {
    pub delegator_address: String,
    pub validator_address: String,
    pub entries: Vec<UnbondingDelegationEntry>,
}

#[derive(Debug, Deserialize)]
pub struct CommissionResponse {
    pub commission: CommissionList,
}

#[derive(Debug, Deserialize)]
pub struct CommissionList {
    pub commission: Vec<Commission>,
}

#[derive(Debug, Deserialize)]
pub struct Commission {
    pub denom: String,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
pub struct RewardsResponse {
    pub rewards: RewardsList,
}

#[derive(Debug, Deserialize)]
pub struct RewardsList {
    pub rewards: Vec<Reward>,
}

#[derive(Debug, Deserialize)]
pub struct Reward {
    pub denom: String,
    pub amount: String,
}

#[derive(Deserialize, Debug)]
pub struct SelfBondRewardResponse {
    pub self_bond_rewards: Vec<Reward>,
}

#[derive(Debug, Deserialize)]
pub struct UnbondingDelegationEntry {
    pub creation_height: String,
    pub completion_time: String,
    pub initial_balance: String,
    pub balance: String,
}

#[derive(Debug, Deserialize)]
pub struct Slash {
    pub validator_period: String,
    pub fraction: String,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorSlashesResponse {
    pub slashes: Vec<Slash>,
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct PoolResponse {
    pub pool: Pool,
}

#[derive(Debug, Deserialize)]
pub struct Pool {
    pub not_bonded_tokens: String,
    pub bonded_tokens: String,
}

#[derive(Debug, Deserialize)]
pub struct SigningInfosResponse {
    pub info: Vec<SigningInfo>,
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct SigningInfo {
    pub address: String,
    pub start_height: String,
    pub index_offset: String,
    #[serde(with = "serde_naive_datetime")]
    pub jailed_until: NaiveDateTime,
    pub tombstoned: bool,
    pub missed_blocks_counter: String,
}

#[derive(Debug, Deserialize)]
pub struct SlashingParamsResponse {
    pub params: SlashingParams,
}

#[derive(Debug, Deserialize)]
pub struct SlashingParams {
    pub signed_blocks_window: String,
    pub min_signed_per_window: String,
    pub downtime_jail_duration: String,
    pub slash_fraction_double_sign: String,
    pub slash_fraction_downtime: String,
}

#[derive(Debug, Deserialize)]
pub struct BankBalance {
    pub denom: String,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
pub struct BankBalancesResponse {
    pub balances: Vec<BankBalance>,
}

#[derive(Debug, Deserialize)]
pub struct GovProposalsResponse {
    pub proposals: Vec<GovProposal>,
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct GovProposal {
    pub id: String,
    pub messages: Vec<GovMessage>,
    pub status: String,
    pub final_tally_result: GovTallyResult,
    pub submit_time: String,
    pub deposit_end_time: String,
    pub total_deposit: Vec<GovCoin>,
    #[serde(default, deserialize_with = "serde_naive_datetime::option")]
    pub voting_start_time: Option<NaiveDateTime>,
    #[serde(default, deserialize_with = "serde_naive_datetime::option")]
    pub voting_end_time: Option<NaiveDateTime>,
    pub metadata: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GovMessage {
    #[serde(rename = "@type")]
    pub msg_type: String,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub amount: Option<Vec<GovCoin>>,
}

#[derive(Debug, Deserialize)]
pub struct GovTallyResult {
    pub yes_count: String,
    pub abstain_count: String,
    pub no_count: String,
    pub no_with_veto_count: String,
}

#[derive(Debug, Deserialize)]
pub struct GovCoin {
    pub denom: String,
    pub amount: String,
}
