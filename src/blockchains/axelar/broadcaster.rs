use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use tracing::info;

use crate::core::app_context::AppContext;
use crate::core::exporter::RunnableModule;

use super::{
    metrics::{
        AXELAR_EVM_POLLS_LATEST_HEIGHT, AXELAR_EVM_POLLS_TOTAL, AXELAR_EVM_VOTES_LATE,
        AXELAR_EVM_VOTES_LATEST_HEIGHT, AXELAR_EVM_VOTES_NO, AXELAR_EVM_VOTES_TOTAL,
        AXELAR_EVM_VOTES_YES,
    },
    types::{EVMPollsResponse, Vote},
};

pub struct Broadcaster {
    app_context: Arc<AppContext>,
    http_client: Client,
    processed_vote_ids: std::collections::HashSet<String>, // Track individual votes to avoid double-counting
    processed_poll_ids: std::collections::HashSet<String>, // Track processed polls to avoid double-counting
}

impl Broadcaster {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        // Create HTTP client with timeout (30 seconds default, or use config timeout)
        let timeout = std::time::Duration::from_secs(
            app_context.config.general.rpc_timeout_seconds as u64
        );
        let http_client = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new()); // Fallback to default client if builder fails

        Self {
            app_context,
            http_client,
            processed_vote_ids: std::collections::HashSet::new(),
            processed_poll_ids: std::collections::HashSet::new(),
        }
    }

    async fn fetch_evm_polls(&self) -> anyhow::Result<EVMPollsResponse> {
        let api_url = &self.app_context.config.network.axelar.broadcaster.axelarscan_api;
        let url = format!("{}/validator/searchEVMPolls", api_url);

        info!("(Axelar Broadcaster) Fetching EVM polls from {}", url);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch EVM polls from axelarscan API")?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Axelarscan API returned error status: {}",
                response.status()
            );
        }

        let text = response.text().await.context("Failed to read response body")?;
        let polls_response: EVMPollsResponse = serde_json::from_str(&text)
            .context("Failed to parse EVM polls response as JSON")?;

        Ok(polls_response)
    }

    fn extract_votes_from_poll(&self, poll: &serde_json::Value) -> HashMap<String, Vote> {
        let mut votes = HashMap::new();

        // Iterate through all keys in the poll object
        if let Some(obj) = poll.as_object() {
            for (key, value) in obj {
                // Check if this key looks like a validator address (starts with "axelar1")
                if key.starts_with("axelar1") && value.is_object() {
                    // Try to deserialize as Vote
                    if let Ok(vote) = serde_json::from_value::<Vote>(value.clone()) {
                        votes.insert(key.clone(), vote);
                    }
                }
            }
        }

        votes
    }

    async fn process_polls(&mut self) -> anyhow::Result<()> {
        let polls_response = match self.fetch_evm_polls().await {
            Ok(response) => response,
            Err(e) => {
                tracing::warn!("(Axelar Broadcaster) Failed to fetch EVM polls: {}. Will retry on next interval.", e);
                return Ok(()); // Return Ok to avoid crashing the module, it will retry on next interval
            }
        };
        let alerting_addresses: std::collections::HashSet<String> = self
            .app_context
            .config
            .network
            .axelar
            .broadcaster
            .alerting
            .addresses
            .iter()
            .cloned()
            .collect();

        let mut latest_height: i64 = 0;
        let mut new_polls_count = 0;
        let mut new_votes_count = 0;

        // Parse each poll (each poll is a Value/JSON object)
        for poll_json in polls_response.data {
            // Extract height and event_id from the poll JSON
            let height = poll_json
                .get("height")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let event_id = poll_json
                .get("event_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let poll_id = format!("{}-{}", height, event_id);

            // Track latest height
            latest_height = latest_height.max(height as i64);

            // Check if we've already processed this poll
            let is_new_poll = !self.processed_poll_ids.contains(&poll_id);

            // Extract bridge/chain information from the poll
            let sender_chain = poll_json
                .get("sender_chain")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let recipient_chain = poll_json
                .get("recipient_chain")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            // Extract votes from the poll
            let votes = self.extract_votes_from_poll(&poll_json);

            // Process each vote individually
            for (validator_address, vote) in votes {
                // Create unique vote ID to avoid double-counting
                let vote_id = format!("{}-{}-{}", height, event_id, vote.id);

                // Skip if we've already processed this vote
                if self.processed_vote_ids.contains(&vote_id) {
                    continue;
                }

                let fires_alerts = alerting_addresses.contains(&validator_address).to_string();

                // Increment counters for this vote (with bridge/chain labels)
                AXELAR_EVM_VOTES_TOTAL
                    .with_label_values(&[
                        &validator_address,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        &fires_alerts,
                        &sender_chain,
                        &recipient_chain,
                    ])
                    .inc();

                if vote.vote {
                    AXELAR_EVM_VOTES_YES
                        .with_label_values(&[
                            &validator_address,
                            &self.app_context.chain_id,
                            &self.app_context.config.general.network,
                            &fires_alerts,
                            &sender_chain,
                            &recipient_chain,
                        ])
                        .inc();
                } else {
                    AXELAR_EVM_VOTES_NO
                        .with_label_values(&[
                            &validator_address,
                            &self.app_context.chain_id,
                            &self.app_context.config.general.network,
                            &fires_alerts,
                            &sender_chain,
                            &recipient_chain,
                        ])
                        .inc();
                }

                if vote.late {
                    AXELAR_EVM_VOTES_LATE
                        .with_label_values(&[
                            &validator_address,
                            &self.app_context.chain_id,
                            &self.app_context.config.general.network,
                            &fires_alerts,
                            &sender_chain,
                            &recipient_chain,
                        ])
                        .inc();
                }

                // Update latest height for this validator (baseline metric)
                AXELAR_EVM_VOTES_LATEST_HEIGHT
                    .with_label_values(&[
                        &validator_address,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        &fires_alerts,
                    ])
                    .set(height as i64);

                self.processed_vote_ids.insert(vote_id);
                new_votes_count += 1;
            }

            // Mark poll as processed and increment counter if it's new
            if is_new_poll {
                self.processed_poll_ids.insert(poll_id);
                AXELAR_EVM_POLLS_TOTAL
                    .with_label_values(&[
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                    ])
                    .inc();
                new_polls_count += 1;
            }
        }

        // Update poll-level metrics
        if latest_height > 0 {
            AXELAR_EVM_POLLS_LATEST_HEIGHT
                .with_label_values(&[
                    &self.app_context.chain_id,
                    &self.app_context.config.general.network,
                ])
                .set(latest_height);
        }

        if new_votes_count > 0 || new_polls_count > 0 {
            info!(
                "(Axelar Broadcaster) Processed {} new polls, {} new votes, latest height: {}",
                new_polls_count,
                new_votes_count,
                latest_height
            );
        }

        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    Ok(Box::new(Broadcaster::new(app_context)))
}

#[async_trait]
impl RunnableModule for Broadcaster {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_polls()
            .await
            .context("Could not process EVM polls")
    }

    fn name(&self) -> &'static str {
        "Axelar Broadcaster"
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.axelar.broadcaster.interval as u64,
        )
    }
}
