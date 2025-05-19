use async_trait::async_trait;
use crate::blockchains::lombard::metrics::{
    LOMBARD_LATEST_SESSION_ID,
    LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION,
};
use crate::blockchains::lombard::types::NotarySessionResponse;
use crate::core::exporter::Task;
use crate::core::clients::blockchain_client::BlockchainClient;
use tracing::info;
use std::sync::Arc;

pub struct LombardLedgerScrapper {
    client: Arc<BlockchainClient>,
    validator_operator_addresses: Vec<String>,
    network: String,
}

impl LombardLedgerScrapper {
    pub fn new(client: Arc<BlockchainClient>, validator_operator_addresses: Vec<String>, network: String) -> Self {
        Self { client, validator_operator_addresses, network }
    }

    async fn process_ledger(&mut self) -> anyhow::Result<()> {
        info!("(Lombard Ledger Scrapper) Running: checking notary session signatures");
        let url = "lombard-finance/ledger/notary/list_notary_session?pagination.limit=1&pagination.reverse=true";
        let resp = self.client.with_rest().get(url).await?;
        let resp: NotarySessionResponse = serde_json::from_str(&resp)?;
        if let Some(session) = resp.notary_sessions.first() {
            LOMBARD_LATEST_SESSION_ID
                .with_label_values(&[&self.network])
                .set(session.id.parse::<i64>().unwrap_or(0));


            for validator in &self.validator_operator_addresses {
                info!("(Lombard Ledger Scrapper) Latest session {}: participants: {:?}", session.id, session.val_set.participants.iter().map(|p| &p.operator).collect::<Vec<_>>());
                if let Some(idx) = session.val_set.participants.iter().position(|p| &p.operator == validator) {
                    let signed = session.signatures.get(idx).map_or(false, |sig| {
                        match sig {
                            Some(s) if !s.is_empty() => true,
                            _ => false,
                        }
                    });
                    info!("(Lombard Ledger Scrapper) Latest session {}: validator {} signed? {} (idx={})", session.id, validator, signed, idx);
                    LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION
                        .with_label_values(&[validator, &self.network])
                        .set(if signed { 1 } else { 0 });
                } else {
                    info!("Latest session {}: validator {} not found in participants", session.id, validator);
                    LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION
                        .with_label_values(&[validator, &self.network])
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
