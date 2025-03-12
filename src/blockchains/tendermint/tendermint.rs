use std::{collections::HashMap, sync::Arc};

use chrono::NaiveDateTime;
use serde_json::from_str;
use tokio::sync::Mutex;
use tracing::{debug, error, info};
use urlencoding::encode;

use crate::{
    blockchains::tendermint::types::{Proposal, TendermintProposalsResponse},
    core::{
        blockchain::{
            BlockHeight, BlockScrapper, BlockchainMetrics, BlockchainMonitor, NetworkScrapper,
        },
        blockchain_client::BlockchainClient,
        http_client::HTTPClientErrors,
    },
};

use super::{
    metrics::{
        TENDERMINT_CURRENT_BLOCK_HEIGHT, TENDERMINT_CURRENT_BLOCK_TIME, TENDERMINT_PROPOSALS,
        TENDERMINT_UPGRADE_STATUS, TENDERMINT_VALIDATOR_JAILED, TENDERMINT_VALIDATOR_MISSED_BLOCKS,
        TENDERMINT_VALIDATOR_PROPOSED_BLOCKS, TENDERMINT_VALIDATOR_PROPOSER_PRIORITY,
        TENDERMINT_VALIDATOR_TOKENS, TENDERMINT_VALIDATOR_VOTING_POWER,
    },
    types::{
        ProposalStatus, TendermintBlockResponse, TendermintRESTResponse, TendermintRESTValidator,
        TendermintStatusResponse, TendermintValidator, ValidatorsResponse,
    },
};

pub struct Tendermint {
    pub client: BlockchainClient,

    pub proccessed_height: i64,
    pub block_window: i64,
    pub chain_id: Option<String>,

    pub validators: HashMap<String, String>,
    pub proposals: Vec<String>,
}

impl Tendermint {
    pub fn new(client: BlockchainClient, block_window: i64) -> Self {
        Tendermint {
            client,
            proccessed_height: 0,
            block_window: block_window,
            chain_id: None,
            validators: HashMap::new(),
            proposals: Vec::new(),
        }
    }
}

impl BlockchainMonitor for Tendermint {
    async fn start_monitoring(self) {
        let self_arc = Arc::new(Mutex::new(self));

        tokio::spawn(async move {
            loop {
                let mut this = self_arc.lock().await;

                if this.get_chain_id().await {
                    this.process_validators().await;
                    this.process_block_window().await;
                    this.process_proposals().await;
                }
            }
        });
    }
}

impl BlockScrapper for Tendermint {
    type BlockResponse = TendermintBlockResponse;
    type Error = HTTPClientErrors;

    async fn get_chain_id(&mut self) -> bool {
        if self.chain_id.is_some() {
            return true;
        }

        info!("Getting chain_id");
        let res = match self.client.with_rpc().get("/status").await {
            Ok(res) => res,
            Err(e) => {
                error!("Error in the call to obtain chain_id: {:?}", e);
                return false;
            }
        };

        match from_str::<TendermintStatusResponse>(&res) {
            Ok(res) => self.chain_id = Some(res.result.node_info.network),
            Err(e) => {
                error!("Error deserializing JSON: {}", e);
                error!("Raw JSON: {}", res);
                return false;
            }
        }
        info!("Obtained chain_id: {}", &self.chain_id.as_ref().unwrap());
        true
    }

    async fn get_block(
        &mut self,
        height: BlockHeight,
    ) -> Result<TendermintBlockResponse, HTTPClientErrors> {
        let path = match height {
            BlockHeight::Height(h) => {
                info!("Obtaining block with height: {}", h);
                format!("/block?height={}", h)
            }
            BlockHeight::Latest => {
                info!("Obtaining latest block");
                "/block".to_string()
            }
        };

        let res = match self.client.with_rpc().get(&path).await {
            Ok(res) => res,
            Err(e) => return Err(e),
        };

        match from_str::<TendermintBlockResponse>(&res) {
            Ok(block_res) => Ok(block_res),
            Err(e) => {
                error!("Error deserializing block JSON: {}", e);
                error!("Raw JSON: {}", res);
                panic!("Could not obtain chain_id from JSON")
            }
        }
    }

    async fn process_block_window(&mut self) {
        let last_block_height = match self.get_block(BlockHeight::Latest).await {
            Ok(block) => block
                .result
                .block
                .header
                .height
                .parse::<i64>()
                .expect("Failed parsing block height"),
            Err(e) => {
                error!("Failed to obtain last_block_height");
                error!("{:?}", e);
                return;
            }
        };

        let mut height_to_process: i64;

        if self.proccessed_height == 0 {
            height_to_process = last_block_height - self.block_window;

            if height_to_process < 1 {
                height_to_process = 1;
            }
        } else {
            height_to_process = self.proccessed_height + 1;
        }

        while height_to_process < last_block_height {
            self.process_block(height_to_process).await;
            height_to_process += 1;
        }
    }

    async fn process_block(&mut self, height: i64) {
        let block = match self.get_block(BlockHeight::Height(height)).await {
            Ok(block) => block,
            Err(e) => {
                error!("Failed to process block at height {}", height);
                error!("{:?}", e);
                return;
            }
        };

        let block_height = block
            .result
            .block
            .header
            .height
            .parse::<i64>()
            .expect("Failed parsing block height");
        let block_time = block.result.block.header.time;
        let block_proposer = block.result.block.header.proposer_address;
        let block_signatures = block.result.block.last_commit.signatures;

        for sig in block_signatures.iter() {
            if !sig.validator_address.is_empty()
                && !self.validators.contains_key(&sig.validator_address)
            {
                self.validators
                    .insert(sig.validator_address.clone(), "Unknown".to_string());
                info!(
                    "Found signature for unknown validator: {}",
                    sig.validator_address
                )
            }
        }

        let validator_name = match self.validators.get(&block_proposer) {
            Some(name) => name,
            None => panic!("Block proposer is not on validator map"),
        };

        self.set_validator_proposed_blocks(&validator_name, &block_proposer);

        let validators_missing_block: Vec<String> = self
            .validators
            .keys()
            .filter(|validator| {
                block_signatures
                    .iter()
                    .all(|sig| sig.validator_address != **validator)
            })
            .cloned()
            .collect();

        for validator in validators_missing_block {
            let validator_name = match self.validators.get(&validator) {
                Some(name) => name,
                None => panic!("Block proposer is not on validator map"),
            };

            self.set_validator_missed_blocks(&validator_name, &validator);
        }

        self.set_current_block_height(block_height);
        self.set_current_block_time(block_time);

        self.proccessed_height = height
    }
}

impl BlockchainMetrics for Tendermint {
    fn set_current_block_height(&self, height: i64) {
        TENDERMINT_CURRENT_BLOCK_HEIGHT
            .with_label_values(&[&self.chain_id.as_ref().unwrap()])
            .set(height.try_into().unwrap());
    }

    fn set_current_block_time(&self, block_time: NaiveDateTime) {
        TENDERMINT_CURRENT_BLOCK_TIME
            .with_label_values(&[&self.chain_id.as_ref().unwrap()])
            .set(block_time.and_utc().timestamp() as f64);
    }

    fn set_validator_missed_blocks(&self, name: &str, validator_address: &str) {
        TENDERMINT_VALIDATOR_MISSED_BLOCKS
            .with_label_values(&[name, validator_address, &self.chain_id.as_ref().unwrap()])
            .inc();
    }

    fn set_validator_voting_power(&self, name: &str, validator_address: &str, voting_power: i64) {
        TENDERMINT_VALIDATOR_VOTING_POWER
            .with_label_values(&[name, validator_address, &self.chain_id.as_ref().unwrap()])
            .set(voting_power);
    }

    fn set_validator_proposed_blocks(&self, name: &str, validator_address: &str) {
        TENDERMINT_VALIDATOR_PROPOSED_BLOCKS
            .with_label_values(&[name, validator_address, &self.chain_id.as_ref().unwrap()])
            .inc();
    }

    fn set_validator_proposer_priority(&self, name: &str, validator_address: &str, priority: i64) {
        TENDERMINT_VALIDATOR_PROPOSER_PRIORITY
            .with_label_values(&[name, validator_address, &self.chain_id.as_ref().unwrap()])
            .set(priority);
    }

    fn set_validator_tokens(&self, name: &str, validator_address: &str, amount: f64) {
        TENDERMINT_VALIDATOR_TOKENS
            .with_label_values(&[name, validator_address, &self.chain_id.as_ref().unwrap()])
            .set(amount);
    }

    fn set_validator_jailed(&self, name: &str, validator_address: &str, jailed: bool) {
        TENDERMINT_VALIDATOR_JAILED
            .with_label_values(&[name, validator_address, &self.chain_id.as_ref().unwrap()])
            .set(if jailed { 1 } else { 0 });
    }

    fn set_upgrade_proposal(
        &self,
        id: &str,
        proposal_type: &str,
        status: &str,
        height: i64,
        active: bool,
    ) {
        TENDERMINT_UPGRADE_STATUS
            .with_label_values(&[id, proposal_type, status, &height.to_string()])
            .set(if active { 1 } else { 0 });
    }

    fn set_proposal(&self, id: &str, proposal_type: &str, title: &str, status: &str, height: &str) {
        TENDERMINT_PROPOSALS
            .with_label_values(&[
                &id,
                &proposal_type,
                &title,
                &status,
                &height,
                &self.chain_id.as_ref().unwrap(),
            ])
            .set(0);
    }
}

impl NetworkScrapper for Tendermint {
    type RpcValidator = TendermintValidator;
    type RestValidator = TendermintRESTValidator;
    type Proposal = Proposal;

    async fn get_rpc_validators(&self, path: &str) -> Vec<Self::RpcValidator> {
        info!("Fetching RPC validators");
        let mut validators: Vec<TendermintValidator> = Vec::new();

        let mut all_fetched = false;
        let mut page = 1;
        let mut fetched = 0;

        while !all_fetched {
            let res = match self
                .client
                .with_rpc()
                .get(&format!("{}?page={}", path, page))
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    error!("Error calling to RPC validators endpoint: {}", e);
                    break;
                }
            };

            let fetched_validators: Vec<TendermintValidator> =
                match from_str::<ValidatorsResponse>(&res) {
                    Ok(res) => {
                        if let Some(res) = res.result {
                            if res.count.parse::<usize>().unwrap() + fetched
                                == res.total.parse::<usize>().unwrap()
                            {
                                all_fetched = true;
                            } else {
                                fetched += res.count.parse::<usize>().unwrap();
                                page += 1;
                            }

                            res.validators
                        } else {
                            error!("Result key not present at validators rpc endpoint response");
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error deserializing JSON: {}", e);
                        error!("Raw JSON: {}", res);
                        break;
                    }
                };

            validators.extend(fetched_validators);
        }
        validators
    }

    async fn get_rest_validators(&self, path: &str) -> Vec<Self::RestValidator> {
        info!("Fetching REST validators");

        let mut pagination_key: Option<String> = None;
        let mut validators: Vec<TendermintRESTValidator> = Vec::new();

        loop {
            let mut url = path.to_string();
            if let Some(key) = &pagination_key {
                let encoded_key = encode(key);
                url = format!("{}?pagination.key={}", path, encoded_key);
            }

            let res = match self.client.with_rest().get(&url).await {
                Ok(res) => res,
                Err(e) => {
                    error!("Error calling to REST validators endpoint: {:?}", e);
                    break;
                }
            };

            let fetched_validators: Vec<TendermintRESTValidator> =
                match from_str::<TendermintRESTResponse>(&res) {
                    Ok(res) => {
                        pagination_key = res.pagination.next_key;
                        res.validators
                    }
                    Err(e) => {
                        error!(
                            "Error deserializing JSON from REST validator endpoint: {}",
                            e
                        );
                        error!("Raw JSON: {}", res);
                        break;
                    }
                };

            validators.extend(fetched_validators);
            if pagination_key.is_none() {
                break;
            }
        }
        validators
    }

    async fn process_validators(&mut self) {
        let rpc_validators = self.get_rpc_validators("/validators").await;
        let rest_validators = self
            .get_rest_validators("/cosmos/staking/v1beta1/validators")
            .await;

        let pub_keys: HashMap<String, (String, String, String)> = rpc_validators
            .into_iter()
            .map(|validator| {
                (
                    validator.pub_key.value.clone(),
                    (
                        validator.address,
                        validator.voting_power,
                        validator.proposer_priority,
                    ),
                )
            })
            .collect();

        for validator in rest_validators {
            let pub_key = &validator.consensus_pubkey.key;
            let name = &validator.description.moniker;
            let tokens: f64 = validator.tokens.parse().unwrap_or(0.0);

            if let Some((address, voting_power, proposer_priority)) =
                pub_keys.get(&validator.consensus_pubkey.key)
            {
                self.validators
                    .insert(address.to_string(), name.to_string());
                self.set_validator_tokens(name, address, tokens);
                self.set_validator_jailed(name, address, validator.jailed);
                self.set_validator_voting_power(
                    name,
                    address,
                    voting_power.parse::<i64>().unwrap(),
                );
                self.set_validator_proposer_priority(
                    name,
                    address,
                    proposer_priority.parse::<i64>().unwrap(),
                );
            } else {
                debug!("No matching address found for pub_key: {}", pub_key);
            }
        }
    }

    async fn get_proposals(&mut self, path: &str) -> Vec<Self::Proposal> {
        info!("Fetching proposals");
        let mut pagination_key: Option<String> = None;
        let mut proposals: Vec<Proposal> = Vec::new();

        loop {
            let mut url = path.to_string();
            if let Some(key) = &pagination_key {
                let encoded_key = encode(key);
                url = format!("{}?pagination.key={}", path, encoded_key);
            }

            let res = match self.client.with_rest().get(&url).await {
                Ok(res) => res,
                Err(e) => {
                    error!("Error calling to REST validators endpoint: {:?}", e);
                    break;
                }
            };

            let fetched_proposals: Vec<Proposal> =
                match from_str::<TendermintProposalsResponse>(&res) {
                    Ok(res) => {
                        pagination_key = res.pagination.next_key;
                        res.proposals
                    }
                    Err(e) => {
                        error!(
                            "Error deserializing JSON from REST validator endpoint: {}",
                            e
                        );
                        error!("Raw JSON: {}", res);
                        break;
                    }
                };

            proposals.extend(fetched_proposals);
            if pagination_key.is_none() {
                break;
            }
        }
        proposals
    }

    async fn process_proposals(&mut self) {
        let proposals = self.get_proposals("/cosmos/gov/v1/proposals").await;

        for proposal in proposals.iter() {
            if proposal.status != ProposalStatus::ProposalStatusPassed {
                continue;
            }

            let first_message = match proposal.messages.get(0) {
                Some(message) => message,
                None => {
                    debug!("Could not read message from proposal");
                    continue;
                }
            };

            if !first_message.msg_type.to_lowercase().contains("upgrade") {
                continue;
            }

            let content = match &first_message.content {
                Some(content) => content,
                None => {
                    debug!("Could not read content from proposal message");
                    continue;
                }
            };

            let plan = match &content.plan {
                Some(plan) => plan,
                None => {
                    debug!("Could not read plan from proposal");
                    continue;
                }
            };

            let height = match plan.height.parse::<u64>() {
                Ok(h) => h,
                Err(_) => {
                    debug!("Could not parse proposal height");
                    continue;
                }
            };

            self.set_upgrade_proposal(
                &proposal.id,
                &content.content_type,
                &proposal.status.to_string(),
                height.try_into().unwrap(),
                height > self.proccessed_height as u64,
            );
        }

        let active_proposals = proposals
            .iter()
            .filter(|proposal| proposal.status == ProposalStatus::ProposalStatusVotingPeriod);

        for proposal in active_proposals {
            self.proposals.push(proposal.id.clone());
        }

        for proposal in proposals {
            if !self.proposals.contains(&proposal.id) {
                continue;
            }

            let first_message = match proposal.messages.get(0) {
                Some(message) => message,
                None => {
                    debug!("Could not read message from proposal");
                    continue;
                }
            };

            let mut proposal_type = "Not found".to_string();
            let mut title = proposal
                .title
                .clone()
                .unwrap_or_else(|| "Not found".to_string());
            let mut height = "0".to_string();

            match &first_message.content {
                Some(content) => {
                    title = content.title.clone().unwrap_or_else(|| {
                        proposal
                            .title
                            .clone()
                            .unwrap_or_else(|| "Not Found".to_string())
                    });
                    proposal_type = content.content_type.clone();
                    if let Some(plan) = &content.plan {
                        height = plan.height.clone();
                    }
                }
                None => {
                    if let Some(legacy_content) = &first_message.plan {
                        title = proposal
                            .title
                            .clone()
                            .unwrap_or_else(|| "Not Found".to_string());

                        height = legacy_content.height.clone();
                    }
                    if title == "Not Found" {
                        if let Some(summary) = &proposal.summary {
                            title = summary.clone();
                        }
                    }
                }
            };

            self.set_proposal(
                &proposal.id,
                &proposal_type,
                &title,
                &proposal.status.to_string(),
                &height,
            );
        }
    }
}
