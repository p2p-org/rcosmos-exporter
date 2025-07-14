use std::{sync::Arc, time::Duration};

use crate::blockchains::babylon::bls;
use crate::blockchains::cometbft::block::block;
use crate::blockchains::cometbft::status;
use crate::blockchains::cometbft::validators;
use crate::blockchains::coredao::block as coredao_block;
use crate::blockchains::coredao::validator;
use crate::blockchains::lombard::ledger;
use crate::blockchains::mezo::poa;
use crate::blockchains::namada::account;
use crate::blockchains::namada::pos;
use crate::blockchains::tendermint::bank;
use crate::blockchains::tendermint::distribution;
use crate::blockchains::tendermint::gov;
use crate::blockchains::tendermint::node_info;
use crate::blockchains::tendermint::slashing;
use crate::blockchains::tendermint::staking;
use crate::blockchains::tendermint::upgrade;
use crate::core::app_context::AppContext;
use crate::core::metrics::exporter_metrics::{EXPORTER_ERROR, EXPORTER_RUN};
use anyhow::Context;
use async_trait::async_trait;
use tokio::sync::{mpsc::UnboundedSender, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[async_trait]
pub trait RunnableModule: Send {
    async fn run(&mut self) -> anyhow::Result<()>;
    fn interval(&self) -> Duration;
    fn name(&self) -> &'static str;
}

pub struct BlockchainExporter {
    app_context: Arc<AppContext>,
    modules: Vec<Arc<Mutex<Box<dyn RunnableModule>>>>,
}

impl BlockchainExporter {
    pub fn new(app_context: Arc<AppContext>, modules: Vec<Box<dyn RunnableModule>>) -> Self {
        Self {
            app_context,
            modules: modules
                .into_iter()
                .map(|m| Arc::new(Mutex::new(m)))
                .collect(),
        }
    }

    /// Start running modules
    pub fn start(&self, cancellation_token: CancellationToken, sender: UnboundedSender<()>) {
        let network = self.app_context.config.general.network.clone();
        for module in self.modules.iter() {
            let module = Arc::clone(&module);
            let sender = sender.clone();
            let token = cancellation_token.clone();
            let network = network.clone();

            tokio::spawn(async move {
                let mut module = module.lock().await;

                EXPORTER_ERROR
                    .with_label_values(&[module.name(), &network])
                    .set(0.0);

                loop {
                    // Always allow the module to run to completion
                    EXPORTER_RUN
                        .with_label_values(&[module.name(), &network])
                        .inc();
                    match module.run().await {
                        Ok(_) => {}
                        Err(e) => {
                            EXPORTER_ERROR
                                .with_label_values(&[module.name(), &network])
                                .inc();
                            error!("Module: {} errored.\n{:?}", module.name(), e);
                        }
                    }

                    // After run completes, wait for either delay or shutdown
                    tokio::select! {
                        _ = tokio::time::sleep(module.interval()) => {
                            // Continue to next iteration
                        }
                        _ = token.cancelled() => {
                            // Exit loop after current run
                            break;
                        }
                    }
                }

                let _ = sender.send(());
                info!("Stopped module: {}", module.name());
            });
        }
    }

    pub fn number_of_modules(&self) -> usize {
        self.modules.len()
    }
}

/// Returns the list of modules to run in network mode
pub fn network_mode_modules(
    app_context: Arc<AppContext>,
) -> anyhow::Result<Vec<Box<dyn RunnableModule>>> {
    let mut modules: Vec<Box<dyn RunnableModule>> = Vec::new();

    // --- CometBFT  ---
    let cometbft = &app_context.config.network.cometbft;
    if cometbft.validators.enabled {
        let module = validators::factory(app_context.clone())
            .context("❌ Failed to create CometBFT Validators module")?;
        modules.push(module);
        info!("✅ CometBFT Validators module created");
    }
    if cometbft.block.enabled {
        let module = block::factory(app_context.clone())
            .context("❌ Failed to create CometBFT Block module")?;
        modules.push(module);
        info!("✅ CometBFT Block module created");
    }

    // --- Tendermint ---
    let tendermint = &app_context.config.network.tendermint;
    if tendermint.bank.enabled {
        let module = bank::factory(app_context.clone())
            .context("❌ Failed to create Tendermint Bank module")?;
        modules.push(module);
        info!("✅ Tendermint Bank module created");
    }
    if tendermint.distribution.enabled {
        let module = distribution::factory(app_context.clone())
            .context("❌ Failed to create Tendermint Distribution module")?;
        modules.push(module);
        info!("✅ Tendermint Distribution module created");
    }
    if tendermint.gov.enabled {
        let module = gov::factory(app_context.clone())
            .context("❌ Failed to create Tendermint Gov module")?;
        modules.push(module);
        info!("✅ Tendermint Gov module created");
    }
    if tendermint.staking.enabled {
        let module = staking::factory(app_context.clone())
            .context("❌ Failed to create Tendermint Staking module")?;
        modules.push(module);
        info!("✅ Tendermint Staking module created");
    }
    if tendermint.slashing.enabled {
        let module = slashing::factory(app_context.clone())
            .context("❌ Failed to create Tendermint Slashing module")?;
        modules.push(module);
        info!("✅ Tendermint Slashing module created");
    }
    if tendermint.upgrade.enabled {
        let module = upgrade::factory(app_context.clone())
            .context("❌ Failed to create Tendermint Upgrade module")?;
        modules.push(module);
        info!("✅ Tendermint Upgrade module created");
    }

    // --- Mezo ---
    if app_context.config.network.mezo.poa.enabled {
        let module =
            poa::factory(app_context.clone()).context("❌ Failed to create Mezo POA module")?;
        modules.push(module);
        info!("✅ Mezo POA module created");
    }

    // --- Babylon ---
    if app_context.config.network.babylon.bls.enabled {
        let module =
            bls::factory(app_context.clone()).context("❌ Failed to create Babylon BLS module")?;
        modules.push(module);
        info!("✅ Babylon BLS module created");
    }

    // --- Lombard ---
    if app_context.config.network.lombard.ledger.enabled {
        let module = ledger::factory(app_context.clone())
            .context("❌ Failed to create Lombard Ledger module")?;
        modules.push(module);
        info!("✅ Lombard Ledger module created");
    }

    // --- Namada ---
    if app_context.config.network.namada.account.enabled {
        let module = account::factory(app_context.clone())
            .context("❌ Failed to create Namada Account module")?;
        modules.push(module);
        info!("✅ Namada Account module created");
    }
    if app_context.config.network.namada.pos.enabled {
        let module =
            pos::factory(app_context.clone()).context("❌ Failed to create Namada Pos module")?;
        modules.push(module);
        info!("✅ Namada Pos module created");
    }

    // --- Core DAO ---
    if app_context.config.network.coredao.block.enabled {
        let module = coredao_block::factory(app_context.clone())
            .context("❌ Failed to create Core DAO Block module")?;
        modules.push(module);
        info!("✅ Core DAO Block module created");
    }
    if app_context.config.network.coredao.validator.enabled {
        let module = validator::factory(app_context.clone())
            .context("❌ Failed to create Core DAO Validator module")?;
        modules.push(module);
        info!("✅ Core DAO Validator module created");
    }

    Ok(modules)
}

pub fn node_mode_modules(
    app_context: Arc<AppContext>,
) -> anyhow::Result<Vec<Box<dyn RunnableModule>>> {
    let mut modules: Vec<Box<dyn RunnableModule>> = Vec::new();

    // --- CometBFT ---
    if app_context.config.node.cometbft.status.enabled {
        let module = status::factory(app_context.clone())
            .context("❌ Failed to create CometBFT Status module")?;
        modules.push(module);
        info!("✅ CometBFT Status module created");
    }

    if app_context.config.node.tendermint.node_info.enabled {
        let module = node_info::factory(app_context.clone())
            .context("❌ Failed to create Tendermint Node Info module")?;
        modules.push(module);
        info!("✅ Tendermint Node Info module created");
    }

    Ok(modules)
}
