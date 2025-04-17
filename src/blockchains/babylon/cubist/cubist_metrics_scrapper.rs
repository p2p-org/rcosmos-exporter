use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Method;
use tokio::sync::{mpsc::UnboundedSender, Notify};
use tracing::{error, info};

use crate::{
    blockchains::babylon::metrics::BABYLON_CUBE_SIGNER_SIGNATURES,
    core::{chain_id::ChainId, exporter::GracefulTask},
};

use super::{
    client::Client,
    types::{ActiveKeyRawData, ActiveKeysResponse, QueryParams},
};

pub struct BabylonCubistMetricScrapper {
    cubist_client: Client,
    chain_id: ChainId,
}

impl BabylonCubistMetricScrapper {
    pub fn new(cubist_client: Client, chain_id: ChainId) -> Self {
        Self {
            cubist_client,
            chain_id,
        }
    }

    pub async fn fetch_active_keys(&mut self) -> anyhow::Result<Vec<ActiveKeyRawData>> {
        info!("(Babylon Cubist Metric Scrapper) Obtaining active keys metrics");

        let params = QueryParams {
            metric_name: "ActiveKeys".to_owned(),
            // ~15 months -> 450 days * 24h
            start_time: (Utc::now() - chrono::Duration::days(450)).timestamp() as u64,
            raw_data: true,
        };

        let res = self
            .cubist_client
            .fetch(
                &format!(
                    "v0/org/{}/metrics",
                    // Important otherwhise we get 403's
                    urlencoding::encode(&self.cubist_client.session_manager.session.org_id)
                        .to_string()
                ),
                Method::POST,
                Some(params),
            )
            .await?;

        let res = serde_json::from_str::<ActiveKeysResponse>(&res)?;

        Ok(res.raw_data)
    }

    pub async fn process_active_keys(&mut self) {
        let active_keys = match self.fetch_active_keys().await {
            Ok(keys) => keys,
            Err(e) => {
                error!("(Babylon Cubist Metric Scrapper) Could not obtain active keys data");
                error!("Error: {}", e);
                return;
            }
        };

        for key in active_keys {
            let num_signatures = match key.num_signatures.parse::<i64>() {
                Ok(num_signatures) => num_signatures,
                Err(e) => {
                    error!(
                        "(Babylon Cubist Metric Scrapper) Could not parse key number of signatures"
                    );
                    error!("Error: {}", e);
                    continue;
                }
            };
            BABYLON_CUBE_SIGNER_SIGNATURES
                .with_label_values(&[&key.key_id, &self.chain_id.to_string()])
                .set(num_signatures);
        }
    }
}

#[async_trait]
impl GracefulTask for BabylonCubistMetricScrapper {
    async fn run_graceful(
        &mut self,
        delay: Duration,
        shutdown_notify: Arc<Notify>,
        sender: UnboundedSender<()>,
    ) {
        info!("Running Babylon Cubist (CubeSigner) Metrics Scrapper");
        loop {
            self.process_active_keys().await;

            tokio::select! {
                _ = tokio::time::sleep(delay) => {},
                _ = shutdown_notify.notified() => {
                    info!("Shutdown signal received while sleeping, stopping Babylon Cubist (CubeSigner) Metrics Scrapper");
                    break;
                }
            }
        }

        info!("Babylon Cubist Metric Scrapper) Saving session secret");
        match self
            .cubist_client
            .session_manager
            .write_session_secret()
            .await
        {
            Ok(_) => info!("(Babylon Cubist Metric Scrapper) Saved session secret"),
            Err(e) => {
                error!(
                    "(Babylon Cubist Metric Scrapper) couldnt save session secret: {}",
                    e
                )
            }
        }
        let _ = sender.send(());
        info!("Stopped Babylon Cubist (CubeSigner) Metrics Scrapper");
    }
}
