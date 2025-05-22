use async_trait::async_trait;
use crate::blockchains::lombard::metrics::{
    LOMBARD_LATEST_SESSION_ID,
    LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION,
};
use crate::blockchains::lombard::types::NotarySessionResponse;
use crate::core::clients::blockchain_client::BlockchainClient;
use crate::core::clients::path::Path;
use crate::core::exporter::Task;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;

pub struct LombardLedgerScrapper {
    client: Arc<BlockchainClient>,
    validator_operator_addresses: Vec<String>,
    network: String,
}

impl LombardLedgerScrapper {
    pub fn new(
        client: Arc<BlockchainClient>,
        validator_operator_addresses: Vec<String>,
        network: String,
    ) -> Self {
        Self {
            client,
            validator_operator_addresses,
            network,
        }
    }

    async fn process_ledger(&mut self) -> anyhow::Result<()> {
        info!("(Lombard Ledger Scrapper) Running: checking notary session signatures");
        let url = "lombard-finance/ledger/notary/list_notary_session?pagination.limit=1&pagination.reverse=true";
        let resp = self.client.with_rest().get(url).await?;
        let resp: NotarySessionResponse = serde_json::from_str(&resp)?;
        for session in resp.notary_sessions {
            let all_signatures_missing = session.signatures.iter().all(|sig| match sig {
                None => true,
                Some(s) => s.is_empty(),
            });
            if all_signatures_missing {
                info!("Session {}: all signatures missing, skipping", session.id);
                continue; // Notaries disagreed, skip
            }
            // Check if at least one signature is present
            let any_signature_present = session.signatures.iter().any(|sig| match sig {
                Some(s) if !s.is_empty() => true,
                _ => false,
            });
        if let Some(session) = resp.notary_sessions.first() {
            LOMBARD_LATEST_SESSION_ID
                .with_label_values(&[&self.network])
                .set(session.id.parse::<i64>().unwrap_or(0));

            let current_session_id = &session.id;
            let current_session_id_num = current_session_id.parse::<i64>().unwrap_or(0);

            for validator in &self.validator_operator_addresses {
                info!("(Lombard Ledger Scrapper) Latest session {}: participants: {:?}", session.id, session.val_set.participants.iter().map(|p| &p.operator).collect::<Vec<_>>());

                for sid in (current_session_id_num.saturating_sub(10))..=current_session_id_num {
                    let sid_str = sid.to_string();
                    if sid_str != *current_session_id {
                        let _ = LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION
                            .remove_label_values(&[validator, &sid_str, &self.network]);
                    }
                }

                if let Some(idx) = session.val_set.participants.iter().position(|p| &p.operator == validator) {
                    let signed = session.signatures.get(idx).map_or(false, |sig| {
                        match sig {
                            Some(s) if !s.is_empty() => true,
                            _ => false,
                        }
                    });
                    info!("(Lombard Ledger Scrapper) Latest session {}: validator {} signed? {} (idx={})", session.id, validator, signed, idx);
                    LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION
                        .with_label_values(&[validator, &session.id, &self.network])
                        .set(if signed { 1 } else { 0 });
                } else {
                    info!("Latest session {}: validator {} not found in participants", session.id, validator);
                    LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION
                        .with_label_values(&[validator, &session.id, &self.network])
                        .set(0);
                }
            }
        } else {
            info!("No notary sessions found");
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
