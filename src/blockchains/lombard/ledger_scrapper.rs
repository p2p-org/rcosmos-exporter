use async_trait::async_trait;
use crate::blockchains::lombard::metrics::LOMBARD_VALIDATOR_SIGNATURE_MISSED;
use crate::blockchains::lombard::types::NotarySessionResponse;
use crate::core::exporter::Task;
use crate::core::clients::http_client::HttpClient;
use tracing::info;
use std::sync::Arc;

pub struct LombardLedgerScrapper {
    rest_client: Arc<HttpClient>,
    validator_operator_addresses: Vec<String>,
}

impl LombardLedgerScrapper {
    pub fn new(rest_client: Arc<HttpClient>, validator_operator_addresses: Vec<String>, _network: String) -> Self {
        Self { rest_client, validator_operator_addresses }
    }
}

#[async_trait]
impl Task for LombardLedgerScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        info!("LombardLedgerScrapper running: checking notary session signatures");
        let mut all_sessions = Vec::new();
        let mut pagination_key: Option<String> = None;
        loop {
            let mut url = "lombard-finance/ledger/notary/list_notary_session".to_string();
            if let Some(ref key) = pagination_key {
                url = format!("{}?pagination.key={}", url, urlencoding::encode(key));
            }
            let resp = self.rest_client.get(&url).await?;
            let resp: NotarySessionResponse = serde_json::from_str(&resp)?;
            all_sessions.extend(resp.notary_sessions);
            match resp.pagination.and_then(|p| p.next_key) {
                Some(next) if !next.is_empty() => pagination_key = Some(next),
                _ => break,
            }
        }
        // Sort sessions by id (as u64), descending, and take the last 10
        all_sessions.sort_by(|a, b| b.id.parse::<u64>().unwrap_or(0).cmp(&a.id.parse::<u64>().unwrap_or(0)));
        let last_sessions = all_sessions.into_iter().take(10);
        for session in last_sessions {
            let all_signatures_missing = session.signatures.iter().all(|sig| sig.is_none() || sig.as_ref().unwrap().is_empty());
            if all_signatures_missing {
                info!("Session {}: all signatures missing, skipping", session.id);
                continue; // Notaries disagreed, skip
            }
            // Check if at least one signature is present
            let any_signature_present = session.signatures.iter().any(|sig| sig.is_some() && !sig.as_ref().unwrap().is_empty());
            for validator in &self.validator_operator_addresses {
                // Log the participants for debugging
                info!("Session {}: participants: {:?}", session.id, session.val_set.participants.iter().map(|p| &p.operator).collect::<Vec<_>>());
                if let Some(idx) = session.val_set.participants.iter().position(|p| &p.operator == validator) {
                    let missed = session.signatures.get(idx).map_or(true, |sig| sig.is_none() || sig.as_ref().unwrap().is_empty());
                    // Only set missed=1 if at least one other validator signed and our validator missed
                    let metric_value = if missed && any_signature_present { 1 } else { 0 };
                    info!("Session {}: validator {} missed? {} (idx={})", session.id, validator, metric_value == 1, idx);
                    LOMBARD_VALIDATOR_SIGNATURE_MISSED
                        .with_label_values(&[validator, &session.id])
                        .set(metric_value);
                } else {
                    info!("Session {}: validator {} not found in participants", session.id, validator);
                }
            }
        }
        Ok(())
    }
    fn name(&self) -> &'static str {
        "Lombard Ledger Scrapper"
    }
}

