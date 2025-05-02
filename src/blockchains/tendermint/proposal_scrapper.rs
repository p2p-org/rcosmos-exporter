use std::{sync::Arc, usize};

use anyhow::Context;
use async_trait::async_trait;
use serde_json::from_str;
use tracing::{info, warn};
use urlencoding::encode;

use crate::{
    blockchains::tendermint::types::TendermintProposalsResponse,
    core::{chain_id::ChainId, clients::blockchain_client::BlockchainClient, exporter::Task},
};

use super::{
    metrics::{TENDERMINT_PROPOSALS, TENDERMINT_UPGRADE_STATUS},
    types::{Proposal, ProposalStatus, TendermintBlockResponse},
};

pub struct TendermintProposalScrapper {
    client: Arc<BlockchainClient>,
    proposals: Vec<String>,
    chain_id: ChainId,
    network: String,
}

impl TendermintProposalScrapper {
    pub fn new(client: Arc<BlockchainClient>, chain_id: ChainId, network: String) -> Self {
        Self {
            client,
            proposals: Vec::new(),
            chain_id,
            network,
        }
    }

    async fn get_last_block_height(&mut self) -> anyhow::Result<usize> {
        info!("Tendermint Proposal Scrapper) Getting last block height");
        let res = self
            .client
            .with_rpc()
            .get("/block")
            .await
            .context("Could not fetch last block")?;

        from_str::<TendermintBlockResponse>(&res)
            .context("Could not parse block response")?
            .result
            .block
            .header
            .height
            .parse::<usize>()
            .context("Could not parse last block height")
    }

    async fn get_proposals(&mut self, path: &str) -> anyhow::Result<Vec<Proposal>> {
        info!("(Tendermint Proposal Scrapper) Fetching proposals");
        let mut pagination_key: Option<String> = None;
        let mut proposals: Vec<Proposal> = Vec::new();

        loop {
            let mut url = path.to_string();
            if let Some(key) = &pagination_key {
                let encoded_key = encode(key);
                url = format!("{}?pagination.key={}", path, encoded_key);
            }

            let res = self.client.with_rest().get(&url).await?;

            let proposal_response = from_str::<TendermintProposalsResponse>(&res)?;

            pagination_key = proposal_response.pagination.next_key;
            proposals.extend(proposal_response.proposals);
            if pagination_key.is_none() {
                break;
            }
        }
        Ok(proposals)
    }

    async fn process_proposals(&mut self) -> anyhow::Result<()> {
        let last_block_height = self.get_last_block_height().await?;

        let proposals = self
            .get_proposals("/cosmos/gov/v1/proposals")
            .await
            .context("Could not obtain proposals")?;

        info!("(Tendermint Proposal Scrapper) Searching for upgrade proposals");
        for proposal in proposals.iter() {
            if proposal.status != ProposalStatus::ProposalStatusPassed {
                continue;
            }

            let first_message = match proposal.messages.get(0) {
                Some(message) => message,
                None => {
                    warn!("(Tendermint Proposal Scrapper) Could not read message from proposal");
                    continue;
                }
            };

            if !first_message.msg_type.to_lowercase().contains("upgrade") {
                continue;
            }

            let content = match &first_message.content {
                Some(content) => content,
                None => {
                    warn!("(Tendermint Proposal Scrapper) Could not read content from proposal message");
                    continue;
                }
            };

            let plan = match &content.plan {
                Some(plan) => plan,
                None => {
                    warn!("(Tendermint Proposal Scrapper) Could not read plan from proposal");
                    continue;
                }
            };

            let height = match plan.height.parse::<u64>() {
                Ok(h) => h,
                Err(_) => {
                    warn!("(Tendermint Proposal Scrapper) Could not parse proposal height");
                    continue;
                }
            };

            TENDERMINT_UPGRADE_STATUS
                .with_label_values(&[
                    &proposal.id,
                    &content.content_type,
                    &proposal.status.to_string(),
                    &height.to_string(),
                    &self.network,
                ])
                .set(if height > last_block_height as u64 {
                    1
                } else {
                    0
                });
        }

        let active_proposals = proposals
            .iter()
            .filter(|proposal| proposal.status == ProposalStatus::ProposalStatusVotingPeriod);

        for proposal in active_proposals {
            self.proposals.push(proposal.id.clone());
        }

        info!("(Tendermint Proposal Scrapper) Processing all proposals");
        for proposal in proposals {
            if !self.proposals.contains(&proposal.id) {
                continue;
            }

            let first_message = match proposal.messages.get(0) {
                Some(message) => message,
                None => {
                    warn!("(Tendermint Proposal Scrapper) Could not read message from proposal");
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

            TENDERMINT_PROPOSALS
                .with_label_values(&[
                    &proposal.id,
                    &proposal_type,
                    &title,
                    &proposal.status.to_string(),
                    &height,
                    &self.chain_id.to_string(),
                    &self.network,
                ])
                .set(0);
        }
        Ok(())
    }
}

#[async_trait]
impl Task for TendermintProposalScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_proposals()
            .await
            .context("Failed to process proposals")
    }

    fn name(&self) -> &'static str {
        "Tendermint Proposal Scrapper"
    }
}
