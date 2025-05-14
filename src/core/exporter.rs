use std::{fmt::Display, sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::sync::{mpsc::UnboundedSender, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::core::metrics::exporter_metrics::EXPORTER_TASK_RUNS;

use super::metrics::exporter_metrics::EXPORTER_TASK_ERRORS;

pub enum Mode {
    Network,
    Node,
}

impl Mode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "network" => Some(Mode::Network),
            "node" => Some(Mode::Node),
            _ => None,
        }
    }
}

impl Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Mode::Network => "Network",
            Mode::Node => "Node",
        };
        write!(f, "{}", s)
    }
}

#[async_trait]
pub trait Task: Send {
    async fn run(&mut self) -> anyhow::Result<()>;
    fn name(&self) -> &'static str;
}

#[derive(Clone)]
pub struct ExporterTask {
    task: Arc<Mutex<Box<dyn Task>>>,
    delay: Duration,
}

impl ExporterTask {
    pub fn new(task: Box<dyn Task>, delay: Duration) -> Self {
        ExporterTask {
            task: Arc::new(Mutex::new(task)),
            delay,
        }
    }
}

pub struct BlockchainExporter {
    tasks: Vec<ExporterTask>,
}

impl BlockchainExporter {
    /// Creates a blockchain exporter without tasks
    pub fn new() -> Self {
        Self {
            tasks: Vec::default(),
        }
    }

    /// Add a new task to the blockchain exporter
    pub fn add_task(mut self, task: ExporterTask) -> Self {
        self.tasks.push(task);
        self
    }

    /// Start running tasks
    pub fn start(
        &self,
        cancellation_token: CancellationToken,
        sender: UnboundedSender<()>,
        network: String,
    ) {
        for scheduled_task in self.tasks.iter() {
            let task_clone = Arc::clone(&scheduled_task.task);
            let delay = scheduled_task.delay;
            let sender = sender.clone();
            let token = cancellation_token.clone();
            let network = network.clone();

            tokio::spawn(async move {
                let mut task = task_clone.lock().await;

                loop {
                    EXPORTER_TASK_RUNS
                        .with_label_values(&[&task.name(), &network])
                        .inc();

                    // Always allow the task to run to completion
                    match task.run().await {
                        Ok(_) => {}
                        Err(e) => {
                            error!("Task: {} errored.\n{:?}", task.name(), e);
                            EXPORTER_TASK_ERRORS
                                .with_label_values(&[&task.name(), &network])
                                .inc();
                        }
                    }

                    // After run completes, wait for either delay or shutdown
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {
                            // Continue to next iteration
                        }
                        _ = token.cancelled() => {
                            // Exit loop after current run
                            break;
                        }
                    }
                }

                let _ = sender.send(());
                info!("Stopped task: {}", task.name());
            });
        }
    }

    pub async fn print_tasks(&self) -> () {
        info!("Tasks to run:");
        for scheduled_task in self.tasks.iter() {
            let task = scheduled_task.task.lock().await;
            info!("{}", task.name());
        }
        info!("--------------------------------------------------------------------");
    }

    pub fn number_of_tasks(&self) -> usize {
        self.tasks.len()
    }
}
