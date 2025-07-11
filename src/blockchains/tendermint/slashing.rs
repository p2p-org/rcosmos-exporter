use std::sync::Arc;

use crate::core::clients::path::Path;
use async_trait::async_trait;
use serde_json::from_str;
use tracing::info;
use urlencoding::encode;

use crate::core::app_context::AppContext;
use crate::core::exporter::RunnableModule;
use anyhow::Context;

use crate::blockchains::tendermint::metrics::{
    TENDERMINT_SLASHING_INDEX_OFFSET, TENDERMINT_SLASHING_JAILED_UNTIL,
    TENDERMINT_SLASHING_MISSED_BLOCKS, TENDERMINT_SLASHING_PARAM_DOWNTIME_JAIL_DURATION,
    TENDERMINT_SLASHING_PARAM_MIN_SIGNED_PER_WINDOW,
    TENDERMINT_SLASHING_PARAM_SIGNED_BLOCKS_WINDOW,
    TENDERMINT_SLASHING_PARAM_SLASH_FRACTION_DOUBLE_SIGN,
    TENDERMINT_SLASHING_PARAM_SLASH_FRACTION_DOWNTIME, TENDERMINT_SLASHING_START_HEIGHT,
    TENDERMINT_SLASHING_TOMBSTONED,
};
use crate::blockchains::tendermint::types::SigningInfosResponse;

pub struct Slashing {
    app_context: Arc<AppContext>,
}

impl Slashing {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Self { app_context }
    }

    async fn get_slashing_info(&self) -> anyhow::Result<()> {
        info!("(Tendermint Slashing) Getting slashing info");
        let client = self.app_context.lcd.as_ref().unwrap();
        let network = &self.app_context.config.general.network;

        let mut pagination_key: Option<String> = None;
        let mut all_info = Vec::new();

        loop {
            let mut url = "/cosmos/slashing/v1beta1/signing_infos".to_string();
            if let Some(ref key) = pagination_key {
                url = format!("{}?pagination.key={}", url, encode(key));
            }
            let res = client.get(Path::from(url)).await?;
            let slashing_info = from_str::<SigningInfosResponse>(&res)
                .context("Could not deserialize slashing info response")?;
            pagination_key = slashing_info.pagination.next_key;
            all_info.extend(slashing_info.info);
            if pagination_key.is_none() {
                break;
            }
        }

        for info in all_info {
            let (_, hash) = bech32::decode(&info.address)
                .context("Could not decode validator address into bech32")?;

            let address = hex::encode_upper(&hash);

            let missed_blocks = info.missed_blocks_counter.parse::<f64>().unwrap_or(0.0);
            let start_height = info.start_height.parse::<f64>().unwrap_or(0.0);
            let index_offset = info.index_offset.parse::<f64>().unwrap_or(0.0);
            let jailed_until = info.jailed_until.and_utc().timestamp() as f64;
            let tombstoned = if info.tombstoned { 1 } else { 0 };

            TENDERMINT_SLASHING_MISSED_BLOCKS
                .with_label_values(&[&address, &self.app_context.chain_id, network])
                .set(missed_blocks);
            TENDERMINT_SLASHING_TOMBSTONED
                .with_label_values(&[&address, &self.app_context.chain_id, network])
                .set(tombstoned);
            TENDERMINT_SLASHING_JAILED_UNTIL
                .with_label_values(&[&address, &self.app_context.chain_id, network])
                .set(jailed_until);
            TENDERMINT_SLASHING_START_HEIGHT
                .with_label_values(&[&address, &self.app_context.chain_id, network])
                .set(start_height);
            TENDERMINT_SLASHING_INDEX_OFFSET
                .with_label_values(&[&address, &self.app_context.chain_id, network])
                .set(index_offset);
        }
        Ok(())
    }

    async fn get_slashing_params(&self) -> anyhow::Result<()> {
        info!("(Tendermint Slashing) Getting slashing params");
        let client = self.app_context.lcd.as_ref().unwrap();
        let network = &self.app_context.config.general.network;
        let res = client
            .get(Path::from("/cosmos/slashing/v1beta1/params"))
            .await?;
        let params =
            from_str::<crate::blockchains::tendermint::types::SlashingParamsResponse>(&res)
                .context("Could not deserialize slashing params response")?;
        let params = params.params;
        let signed_blocks_window = params.signed_blocks_window.parse::<f64>().unwrap_or(0.0);
        let min_signed_per_window = params.min_signed_per_window.parse::<f64>().unwrap_or(0.0);
        let downtime_jail_duration = params
            .downtime_jail_duration
            .trim_end_matches('s')
            .parse::<f64>()
            .unwrap_or(0.0);
        let slash_fraction_double_sign = params
            .slash_fraction_double_sign
            .parse::<f64>()
            .unwrap_or(0.0);
        let slash_fraction_downtime = params.slash_fraction_downtime.parse::<f64>().unwrap_or(0.0);
        TENDERMINT_SLASHING_PARAM_SIGNED_BLOCKS_WINDOW
            .with_label_values(&[&self.app_context.chain_id, network])
            .set(signed_blocks_window);
        TENDERMINT_SLASHING_PARAM_MIN_SIGNED_PER_WINDOW
            .with_label_values(&[&self.app_context.chain_id, network])
            .set(min_signed_per_window);
        TENDERMINT_SLASHING_PARAM_DOWNTIME_JAIL_DURATION
            .with_label_values(&[&self.app_context.chain_id, network])
            .set(downtime_jail_duration);
        TENDERMINT_SLASHING_PARAM_SLASH_FRACTION_DOUBLE_SIGN
            .with_label_values(&[&self.app_context.chain_id, network])
            .set(slash_fraction_double_sign);
        TENDERMINT_SLASHING_PARAM_SLASH_FRACTION_DOWNTIME
            .with_label_values(&[&self.app_context.chain_id, network])
            .set(slash_fraction_downtime);
        Ok(())
    }
}

#[async_trait]
impl RunnableModule for Slashing {
    async fn run(&mut self) -> anyhow::Result<()> {
        info!("(Tendermint Slashing) Getting slashing info");
        self.get_slashing_params()
            .await
            .context("Failed to get slashing params")?;
        self.get_slashing_info()
            .await
            .context("Failed to get slashing info")
    }

    fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.app_context.config.network.tendermint.slashing.interval as u64,
        )
    }

    fn name(&self) -> &'static str {
        "Tendermint Slashing"
    }
}

pub fn factory(app_context: Arc<AppContext>) -> anyhow::Result<Box<dyn RunnableModule>> {
    if app_context.lcd.is_none() {
        anyhow::bail!("Config is missing LCD node pool");
    }
    Ok(Box::new(Slashing::new(app_context)))
}
