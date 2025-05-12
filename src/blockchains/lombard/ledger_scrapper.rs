use async_trait::async_trait;
use crate::blockchains::lombard::metrics::LOMBARD_VALIDATOR_SIGNATURE_MISSED;
use crate::blockchains::lombard::types::NotarySessionResponse;
use crate::core::exporter::Task;
use crate::core::clients::blockchain_client::BlockchainClient;
use tracing::info;
use std::sync::Arc;

pub struct LombardLedgerScrapper {
    client: Arc<BlockchainClient>,
    validator_operator_addresses: Vec<String>,
}

impl LombardLedgerScrapper {
    pub fn new(client: Arc<BlockchainClient>, validator_operator_addresses: Vec<String>, _network: String) -> Self {
        Self { client, validator_operator_addresses }
    }

    async fn process_ledger(&mut self) -> anyhow::Result<()> {
        info!("(Lombard Ledger Scrapper) Running: checking notary session signatures");
        let url = "lombard-finance/ledger/notary/list_notary_session?pagination.limit=10&pagination.reverse=true";
        let resp = self.client.with_rest().get(url).await?;
        let resp: NotarySessionResponse = serde_json::from_str(&resp)?;
        for session in resp.notary_sessions {
            let all_signatures_missing = session.signatures.iter().all(|sig| {
                match sig {
                    None => true,
                    Some(s) => s.is_empty(),
                }
            });
            if all_signatures_missing {
                info!("Session {}: all signatures missing, skipping", session.id);
                continue; // Notaries disagreed, skip
            }
            // Check if at least one signature is present
            let any_signature_present = session.signatures.iter().any(|sig| {
                match sig {
                    Some(s) if !s.is_empty() => true,
                    _ => false,
                }
            });
            for validator in &self.validator_operator_addresses {
                // Log the participants for debugging
                info!("(Lombard Ledger Scrapper) Session {}: participants: {:?}", session.id, session.val_set.participants.iter().map(|p| &p.operator).collect::<Vec<_>>());
                if let Some(idx) = session.val_set.participants.iter().position(|p| &p.operator == validator) {
                    let missed = session.signatures.get(idx).map_or(true, |sig| {
                        match sig {
                            None => true,
                            Some(s) => s.is_empty(),
                        }
                    });
                    // Only set missed=1 if at least one other validator signed and our validator missed
                    let metric_value = if missed && any_signature_present { 1 } else { 0 };
                    info!("(Lombard Ledger Scrapper) Session {}: validator {} missed? {} (idx={})", session.id, validator, metric_value == 1, idx);
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
}

#[async_trait]
impl Task for LombardLedgerScrapper {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_ledger().await
    }
    fn name(&self) -> &'static str {
        "Lombard Ledger Scrapper"
    }
}

