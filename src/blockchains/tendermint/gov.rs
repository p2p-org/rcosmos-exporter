use crate::blockchains::tendermint::metrics::TENDERMINT_PROPOSAL;
use crate::blockchains::tendermint::types::{GovProposal, GovProposalsResponse};
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;
use urlencoding::encode;

pub struct Gov {
    app_context: Arc<AppContext>,
}

impl Gov {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn fetch_proposals(&self) -> Result<Vec<GovProposal>> {
        let client = self.app_context.lcd.as_ref().unwrap();
        let mut proposals = Vec::new();
        let mut pagination_key: Option<String> = None;
        let path = "/cosmos/gov/v1/proposals";
        loop {
            let mut url = path.to_string();
            if let Some(key) = &pagination_key {
                url = format!("{}?pagination.key={}", path, encode(key));
            }
            let res = client
                .get(Path::from(url))
                .await
                .context("Failed to fetch proposals from LCD")?;
            let resp: GovProposalsResponse =
                serde_json::from_str(&res).context("Failed to parse proposals response")?;
            proposals.extend(resp.proposals);
            pagination_key = resp.pagination.next_key;
            if pagination_key.is_none() {
                break;
            }
        }
        Ok(proposals)
    }

    async fn process_proposals(&self) -> Result<()> {
        let proposals = self.fetch_proposals().await?;
        info!(
            "Fetched {} proposals from Tendermint Gov endpoint",
            proposals.len()
        );
        for proposal in &proposals {
            info!("Proposal ID: {} Status: {}", proposal.id, proposal.status);
            // Fill metric labels: id, status, voting_start_time, voting_end_time
            let voting_start_time = proposal
                .voting_start_time
                .map(|dt| dt.and_utc().timestamp().to_string())
                .unwrap_or_else(|| "".to_string());
            let voting_end_time = proposal
                .voting_end_time
                .map(|dt| dt.and_utc().timestamp().to_string())
                .unwrap_or_else(|| "".to_string());
            TENDERMINT_PROPOSAL
                .with_label_values(&[
                    &proposal.id,
                    &proposal.status,
                    &voting_start_time,
                    &voting_end_time,
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(0);
        }
        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.lcd.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    Ok(Box::new(Gov::new(app_context)))
}

#[async_trait]
impl RunnableModule for Gov {
    async fn run(&mut self) -> Result<()> {
        self.process_proposals().await
    }
    fn name(&self) -> &'static str {
        "Tendermint Gov"
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.tendermint.gov.interval as u64,
        )
    }
}
