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
        AXELAR_EVM_BROADCASTER_ADDRESS_UNKNOWN, AXELAR_EVM_POLLS_LATEST_HEIGHT, AXELAR_EVM_POLLS_TOTAL,
        AXELAR_EVM_VOTES_LATE, AXELAR_EVM_VOTES_LATEST_HEIGHT, AXELAR_EVM_VOTES_MISSED,
        AXELAR_EVM_VOTES_NO, AXELAR_EVM_VOTES_TOTAL, AXELAR_EVM_VOTES_YES,
    },
    types::{EVMPollsResponse, GetValidatorsResponse, Vote},
};

pub struct Broadcaster {
    app_context: Arc<AppContext>,
    http_client: Client,
    processed_vote_ids: std::collections::HashSet<String>, // Track individual votes to avoid double-counting
    processed_poll_ids: std::collections::HashSet<String>, // Track processed polls to avoid double-counting
    processed_missed_vote_ids: std::collections::HashSet<String>, // Track missed votes to avoid double-counting (format: "poll_id-delegator_address")
    delegator_to_operator_cache: std::collections::HashMap<String, String>, // Cache delegator -> operator address mapping
    delegator_to_moniker_cache: std::collections::HashMap<String, String>, // Cache delegator -> moniker mapping
    operator_to_moniker_cache: std::collections::HashMap<String, String>,  // Cache operator -> moniker mapping
    operator_to_delegator_cache: std::collections::HashMap<String, Vec<String>>, // Cache operator -> list of delegator addresses (for reverse lookup)
    axelarscan_validators_loaded: bool, // Track if we've loaded validators from axelarscan API
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
            processed_missed_vote_ids: std::collections::HashSet::new(),
            delegator_to_operator_cache: std::collections::HashMap::new(),
            delegator_to_moniker_cache: std::collections::HashMap::new(),
            operator_to_moniker_cache: std::collections::HashMap::new(),
            operator_to_delegator_cache: std::collections::HashMap::new(),
            axelarscan_validators_loaded: false,
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

    /// Fetch validators from axelarscan API and build mappings for broadcaster/delegator -> operator -> moniker
    async fn load_validators_from_axelarscan(&mut self) -> anyhow::Result<()> {
        if self.axelarscan_validators_loaded {
            return Ok(()); // Already loaded
        }

        let api_url = &self.app_context.config.network.axelar.broadcaster.axelarscan_api;
        let url = format!("{}/api?method=getValidators", api_url);

        info!("(Axelar Broadcaster) Fetching validators from {}", url);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch validators from axelarscan API")?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Axelarscan API returned error status: {}",
                response.status()
            );
        }

        let text = response.text().await.context("Failed to read response body")?;
        let validators_response: GetValidatorsResponse = serde_json::from_str(&text)
            .context("Failed to parse validators response as JSON")?;

        let validators_count = validators_response.data.len();
        let mut broadcaster_mappings = 0;

        // Build mappings: broadcaster/delegator address -> (operator_address, moniker)
        for validator in &validators_response.data {
            let operator_address = validator.operator_address.clone();
            // Use moniker, or fallback to "unknown" if empty
            let moniker = if validator.description.moniker.trim().is_empty() {
                "unknown".to_string()
            } else {
                validator.description.moniker.clone()
            };

            // Map delegator_address -> operator -> moniker
            self.delegator_to_operator_cache
                .insert(validator.delegator_address.clone(), operator_address.clone());
            self.delegator_to_moniker_cache
                .insert(validator.delegator_address.clone(), moniker.clone());
            self.operator_to_moniker_cache
                .insert(operator_address.clone(), moniker.clone());

            // Build reverse mapping: operator -> list of delegator addresses
            self.operator_to_delegator_cache
                .entry(operator_address.clone())
                .or_insert_with(Vec::new)
                .push(validator.delegator_address.clone());

            // Map broadcaster_address -> operator -> moniker (if present)
            if let Some(broadcaster_address) = &validator.broadcaster_address {
                self.delegator_to_operator_cache
                    .insert(broadcaster_address.clone(), operator_address.clone());
                self.delegator_to_moniker_cache
                    .insert(broadcaster_address.clone(), moniker.clone());
                // Add broadcaster address to reverse mapping too
                self.operator_to_delegator_cache
                    .entry(operator_address.clone())
                    .or_insert_with(Vec::new)
                    .push(broadcaster_address.clone());
                broadcaster_mappings += 1;
            }
        }

        // Track operators with missing broadcaster_address for alerting addresses
        // This helps us gracefully handle cases where broadcaster_address is missing from API
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

        for alerting_address in &alerting_addresses {
            // Check if this address is in our cache (either as delegator or broadcaster)
            if let Some(operator_address) = self.delegator_to_operator_cache.get(alerting_address).cloned() {
                // Check if this operator has a broadcaster_address in the API
                let has_broadcaster = validators_response.data
                    .iter()
                    .find(|v| v.operator_address == operator_address)
                    .and_then(|v| v.broadcaster_address.as_ref())
                    .is_some();

                // If the alerting address is the delegator_address and broadcaster_address is missing,
                // we need to track this
                let is_delegator = validators_response.data
                    .iter()
                    .any(|v| v.operator_address == operator_address && v.delegator_address == *alerting_address);

                if is_delegator && !has_broadcaster {
                    // This operator has missing broadcaster_address and the alerting address is the delegator
                    // We'll learn the broadcaster_address from votes later
                    // For now, we'll track this in a metric
                    let moniker = self.operator_to_moniker_cache
                        .get(&operator_address)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());

                    AXELAR_EVM_BROADCASTER_ADDRESS_UNKNOWN
                        .with_label_values(&[
                            alerting_address,
                            &moniker,
                            &self.app_context.chain_id,
                            &self.app_context.config.general.network,
                            "true", // alerts=true since it's in alerting config
                        ])
                        .set(1);
                }
            }
        }

        self.axelarscan_validators_loaded = true;
        info!(
            "(Axelar Broadcaster) Loaded {} validators from axelarscan API ({} total address mappings, {} with broadcaster_address)",
            validators_count,
            self.delegator_to_moniker_cache.len(),
            broadcaster_mappings
        );

        // Log a sample of mappings for debugging (first 3 validators with non-empty monikers)
        let sample_count = validators_response.data
            .iter()
            .filter(|v| !v.description.moniker.trim().is_empty())
            .take(3)
            .count();
        if sample_count > 0 {
            let samples: Vec<_> = validators_response.data
                .iter()
                .filter(|v| !v.description.moniker.trim().is_empty())
                .take(3)
                .map(|v| format!("{} -> {}",
                    v.delegator_address,
                    v.description.moniker.trim()))
                .collect();
            tracing::debug!(
                "(Axelar Broadcaster) Sample moniker mappings: {:?}",
                samples
            );
        }

        Ok(())
    }

    async fn resolve_operator_and_moniker_for_delegator(
        &mut self,
        delegator_address: &str,
    ) -> (Option<String>, Option<String>) {
        // Check cache first (populated from axelarscan API)
        // Try to get both operator and moniker from cache
        let operator = self.delegator_to_operator_cache.get(delegator_address).cloned();
        let moniker = self.delegator_to_moniker_cache.get(delegator_address).cloned();

        if operator.is_some() && moniker.is_some() {
            return (operator, moniker);
        }

        // Check if there's a manual mapping in config (optional fallback)
        let operator_from_config = self
            .app_context
            .config
            .network
            .axelar
            .broadcaster
            .alerting
            .validators
            .get(delegator_address)
            .cloned();

        if let Some(operator_addr) = operator_from_config {
            // Use config mapping if available
            let operator = Some(operator_addr.clone());
            // Try to get moniker from cache (populated from axelarscan API)
            let moniker = self.operator_to_moniker_cache.get(&operator_addr).cloned();

            // Cache the results
            if let Some(op) = &operator {
                self.delegator_to_operator_cache
                    .insert(delegator_address.to_string(), op.clone());
            }
            if let Some(m) = &moniker {
                self.delegator_to_moniker_cache
                    .insert(delegator_address.to_string(), m.clone());
            }

            return (operator, moniker);
        }

        // Not found in cache or config - return None
        // The cache should have been populated from axelarscan API on startup
        // Log a debug message to help diagnose lookup failures
        if self.axelarscan_validators_loaded {
            tracing::debug!(
                "(Axelar Broadcaster) No moniker found for address {} (cache has {} entries)",
                delegator_address,
                self.delegator_to_moniker_cache.len()
            );
        }
        (None, None)
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
                        // Use the vote key (which is the voting address) as the key
                        // The voter field should match, but use the key as the source of truth
                        votes.insert(key.clone(), vote);
                    }
                }
            }
        }

        votes
    }

    async fn process_polls(&mut self) -> anyhow::Result<()> {
        // Load validators from axelarscan API on first run (if not already loaded)
        // Also retry if loading failed previously (to handle transient API issues)
        if !self.axelarscan_validators_loaded {
            match self.load_validators_from_axelarscan().await {
                Ok(()) => {
                    info!("(Axelar Broadcaster) Successfully loaded validators from axelarscan API");
                }
                Err(e) => {
                    tracing::warn!(
                        "(Axelar Broadcaster) Failed to load validators from axelarscan API: {}. Will retry on next interval. Monikers may show as 'unknown'.",
                        e
                    );
                    // Continue processing even if validator loading fails
                }
            }
        }

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

        // Alias: all configured broadcaster/delegator addresses we care about
        let delegator_addresses: std::collections::HashSet<String> = alerting_addresses.clone();

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

            // Extract votes from the poll (votes are keyed by axelar1... delegator addresses)
            let votes = self.extract_votes_from_poll(&poll_json);

            // Extract participants list (contains axelarvaloper1... operator addresses)
            let participants: std::collections::HashSet<String> = poll_json
                .get("participants")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            // Track which validators voted (by their axelar1... delegator/broadcaster address)
            // Votes are keyed by the voting address, and also have a "voter" field
            // We need to check both because the broadcaster_address might not be in the API response
            let mut validators_who_voted: std::collections::HashSet<String> = votes.keys().cloned().collect();
            // Also add voter addresses from vote objects (in case votes are keyed differently)
            for vote in votes.values() {
                validators_who_voted.insert(vote.voter.clone());
            }

            // Check for missed votes: For ALL validators in the participants list:
            // 1. The validator's operator address is in the participants list (they're expected to vote)
            // 2. The validator's delegator/broadcaster address doesn't have a vote in this poll
            // This detects missed votes for ALL validators, not just those in alerting config
            if !participants.is_empty() && is_new_poll {
                for operator_address in &participants {
                    // Get all delegator/broadcaster addresses for this operator
                    let delegator_addresses = self.operator_to_delegator_cache
                        .get(operator_address)
                        .cloned()
                        .unwrap_or_default();

                    // Check if any of the delegator/broadcaster addresses voted
                    let validator_voted = delegator_addresses.iter()
                        .any(|addr| validators_who_voted.contains(addr));

                    if !validator_voted && !delegator_addresses.is_empty() {
                        // Validator was expected to vote (operator in participants) but didn't
                        // Prefer broadcaster_address if available, otherwise use delegator_address
                        // This ensures we use the correct address for the metric label
                        let delegator_address = delegator_addresses.iter()
                            .find(|addr| alerting_addresses.contains(*addr))
                            .or_else(|| delegator_addresses.first())
                            .unwrap();

                        let missed_vote_id = format!("{}-{}", poll_id, delegator_address);

                        // Only count if we haven't already processed this missed vote
                        if !self.processed_missed_vote_ids.contains(&missed_vote_id) {
                            // Get moniker for this validator
                            let moniker = self.operator_to_moniker_cache
                                .get(operator_address)
                                .cloned()
                                .unwrap_or_else(|| "unknown".to_string());

                            // Determine if this validator is in alerting config
                            let fires_alerts = alerting_addresses.contains(delegator_address)
                                .to_string();

                            // Debug logging to help diagnose missed vote detection
                            tracing::warn!(
                                "(Axelar Broadcaster) Missed vote detected for operator {} (moniker: {}). \
                                Poll: {}, Checked addresses: {:?}, Total votes in poll: {}, \
                                Votes for this operator's addresses: {:?}",
                                operator_address,
                                moniker,
                                poll_id,
                                delegator_addresses,
                                validators_who_voted.len(),
                                validators_who_voted.iter()
                                    .filter(|addr| delegator_addresses.contains(*addr))
                                    .collect::<Vec<_>>()
                            );

                            AXELAR_EVM_VOTES_MISSED
                                .with_label_values(&[
                                    delegator_address,
                                    &moniker,
                                    &self.app_context.chain_id,
                                    &self.app_context.config.general.network,
                                    &fires_alerts,
                                    &sender_chain,
                                    &recipient_chain,
                                ])
                                .inc();

                            self.processed_missed_vote_ids.insert(missed_vote_id);
                        }
                    }
                }
            }

            // Process each vote individually
            for (validator_address, vote) in votes {
                // Create unique vote ID to avoid double-counting
                let vote_id = format!("{}-{}-{}", height, event_id, vote.id);

                // Skip if we've already processed this vote
                if self.processed_vote_ids.contains(&vote_id) {
                    continue;
                }

                // Try to resolve operator for this voting address
                // This might be a broadcaster_address that's not in the API response
                let (mut operator_address, mut moniker) = self
                    .resolve_operator_and_moniker_for_delegator(validator_address.as_str())
                    .await;

                // If we couldn't resolve it but it's in alerting.addresses, try to infer it
                // This handles cases where broadcaster_address is missing from API
                if operator_address.is_none() && alerting_addresses.contains(&validator_address) {
                    // Check if any operator in alerting config has missing broadcaster_address
                    // If so, this address might be the missing broadcaster_address
                    for alerting_addr in &alerting_addresses {
                        if let Some(op_addr) = self.delegator_to_operator_cache.get(alerting_addr).cloned() {
                            // Check if this operator's delegator_address matches the alerting address
                            // and if broadcaster_address is missing
                            let delegator_list = self.operator_to_delegator_cache
                                .get(&op_addr)
                                .cloned()
                                .unwrap_or_default();

                            // If the alerting address is in the delegator list and the voting address is not,
                            // and we can't resolve the voting address, it might be the missing broadcaster_address
                            if delegator_list.contains(alerting_addr) && !delegator_list.contains(&validator_address) {
                                // This is likely the missing broadcaster_address for this operator
                                operator_address = Some(op_addr.clone());
                                moniker = self.operator_to_moniker_cache
                                    .get(&op_addr)
                                    .cloned();

                                // Cache the results
                                self.delegator_to_operator_cache
                                    .insert(validator_address.clone(), op_addr.clone());
                                if let Some(m) = &moniker {
                                    self.delegator_to_moniker_cache
                                        .insert(validator_address.clone(), m.clone());
                                }

                                // Add to reverse mapping
                                self.operator_to_delegator_cache
                                    .entry(op_addr.clone())
                                    .or_insert_with(Vec::new)
                                    .push(validator_address.clone());

                                break;
                            }
                        }
                    }
                }

                // If we found an operator, ensure this address is in the reverse mapping
                // This handles cases where broadcaster_address is missing from API but appears in votes
                // This is critical: if broadcaster_address is missing from API but votes are keyed by it,
                // we need to add it to our cache so missed vote detection works correctly
                if let Some(op_addr) = &operator_address {
                    // Add this address to the operator's delegator list if not already present
                    let delegator_list = self.operator_to_delegator_cache
                        .entry(op_addr.clone())
                        .or_insert_with(Vec::new);
                    if !delegator_list.contains(&validator_address) {
                        delegator_list.push(validator_address.clone());
                    }
                }

                let fires_alerts = delegator_addresses.contains(&validator_address).to_string();
                let moniker = moniker.unwrap_or_else(|| "unknown".to_string());

                // Increment counters for this vote (with bridge/chain labels)
                AXELAR_EVM_VOTES_TOTAL
                    .with_label_values(&[
                        &validator_address,
                        &moniker,
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
                            &moniker,
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
                            &moniker,
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
                            &moniker,
                            &self.app_context.chain_id,
                            &self.app_context.config.general.network,
                            &fires_alerts,
                            &sender_chain,
                            &recipient_chain,
                        ])
                        .inc();
                }

                // Update latest height for this validator (baseline metric)
                // Use vote.height (the actual height at which the vote was cast) instead of poll height
                // This ensures the metric tracks the validator's actual voting progress linearly
                let vote_height = vote.height as i64;
                let gauge = AXELAR_EVM_VOTES_LATEST_HEIGHT
                    .with_label_values(&[
                        &validator_address,
                        &moniker,
                        &self.app_context.chain_id,
                        &self.app_context.config.general.network,
                        &fires_alerts,
                    ]);
                // Only update if this vote height is higher than the current value
                // This prevents the metric from going backwards if votes are processed out of order
                let current_value = gauge.get();
                if vote_height > current_value {
                    gauge.set(vote_height);
                }

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
