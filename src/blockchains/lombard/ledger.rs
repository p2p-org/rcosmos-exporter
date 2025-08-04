use crate::blockchains::lombard::metrics::{
    LOMBARD_LATEST_SESSION_ID, LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION,
};
use crate::blockchains::lombard::types::NotarySessionResponse;
use crate::core::app_context::AppContext;
use crate::core::clients::path::Path;
use crate::core::exporter::RunnableModule;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;

pub struct Ledger {
    app_context: Arc<AppContext>,
}

impl Ledger {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn process_ledger(&self) -> anyhow::Result<()> {
        info!("(Lombard Ledger) Running: checking notary session signatures");
        let client = self.app_context.lcd.as_ref().unwrap();
        let validator_operator_addresses =
            &self.app_context.config.network.lombard.ledger.addresses;
        let network = &self.app_context.config.general.network;
        let chain_id = &self.app_context.chain_id;
        
        // Fetch only the latest session
        let url = "lombard-finance/ledger/notary/list_notary_session?pagination.limit=1&pagination.reverse=true";
        let resp = client.get(Path::from(url)).await?;
        let resp: NotarySessionResponse = serde_json::from_str(&resp)?;
        
        if let Some(latest_session) = resp.notary_sessions.first() {
            // Check if all signatures are missing (notaries disagreed)
            let all_signatures_missing = latest_session.signatures.iter().all(|sig| match sig {
                None => true,
                Some(s) => s.is_empty(),
            });
            
            if all_signatures_missing {
                info!(
                    "(Lombard Ledger) Session {}: all signatures missing, skipping",
                    latest_session.id
                );
                return Ok(());
            }

            // Set the latest session ID metric
            LOMBARD_LATEST_SESSION_ID
                .with_label_values(&[chain_id, network])
                .set(latest_session.id.parse::<i64>().unwrap_or(0));
            
            let current_session_id_num = latest_session.id.parse::<i64>().unwrap_or(0);
            
            for validator in validator_operator_addresses {
                info!(
                    "(Lombard Ledger) Processing validator {} for latest session {}",
                    validator, latest_session.id
                );
                
                // Clean up the previous session metric for this validator
                let previous_session_id = (current_session_id_num - 1).to_string();
                let _ = LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION
                    .remove_label_values(&[validator, &previous_session_id, chain_id, network]);
                
                // Process only the latest session
                if let Some(idx) = latest_session
                    .val_set
                    .participants
                    .iter()
                    .position(|p| &p.operator == validator)
                {
                    let signed = latest_session.signatures.get(idx).map_or(false, |sig| match sig {
                        Some(s) if !s.is_empty() => true,
                        _ => false,
                    });
                    info!(
                        "(Lombard Ledger) Session {}: validator {} signed? {} (idx={})",
                        latest_session.id, validator, signed, idx
                    );
                    LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION
                        .with_label_values(&[validator, &latest_session.id, chain_id, network])
                        .set(if signed { 1 } else { 0 });
                } else {
                    info!(
                        "(Lombard Ledger) Session {}: validator {} not found in participants",
                        latest_session.id, validator
                    );
                    LOMBARD_VALIDATOR_SIGNED_LATEST_SESSION
                        .with_label_values(&[validator, &latest_session.id, chain_id, network])
                        .set(0);
                }
            }
        } else {
            info!("(Lombard Ledger) No notary sessions found");
        }
        Ok(())
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.lcd.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    Ok(Box::new(Ledger::new(app_context)))
}

#[async_trait]
impl RunnableModule for Ledger {
    async fn run(&mut self) -> anyhow::Result<()> {
        self.process_ledger().await
    }
    fn name(&self) -> &'static str {
        "Lombard Ledger"
    }
    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.lombard.ledger.interval as u64,
        )
    }
}
