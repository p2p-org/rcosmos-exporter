use std::{fmt::Display, sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::sync::{mpsc::UnboundedSender, Mutex, Notify};

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

enum Tasks {
    Simple(Box<dyn Task + Send>),
    Graceful(Box<dyn GracefulTask + Send>),
}
#[async_trait]
pub trait Task: Send {
    async fn run(&mut self, delay: Duration);
}

#[async_trait]
pub trait GracefulTask: Send {
    async fn run_graceful(
        &mut self,
        delay: Duration,
        shutdown_notify: Arc<Notify>,
        sender: UnboundedSender<()>,
    );
}

#[derive(Clone)]
pub struct ExporterTask {
    task: Arc<Mutex<Tasks>>,
    delay: Duration,
    sender: Option<UnboundedSender<()>>,
}

impl ExporterTask {
    pub fn new(task: Box<dyn Task>, delay: Duration) -> Self {
        ExporterTask {
            task: Arc::new(Mutex::new(Tasks::Simple(task))),
            delay,
            sender: None,
        }
    }

    pub fn graceful(
        task: Box<dyn GracefulTask>,
        delay: Duration,
        sender: UnboundedSender<()>,
    ) -> Self {
        ExporterTask {
            task: Arc::new(Mutex::new(Tasks::Graceful(task))),
            delay,
            sender: Some(sender),
        }
    }
}

pub struct BlockchainExporter {
    tasks: Vec<ExporterTask>,
    graceful_tasks: usize,
}

impl BlockchainExporter {
    /// Creates a blockchain exporter without tasks
    pub fn new() -> Self {
        Self {
            tasks: Vec::default(),
            graceful_tasks: 0,
        }
    }

    /// Add a new task to the blockchain exporter
    pub fn add_task(mut self, task: ExporterTask) -> Self {
        if task.sender.is_some() {
            self.graceful_tasks += 1;
        }
        self.tasks.push(task);
        self
    }

    /// Start running tasks
    pub fn start(&self, shutdown_notify: Arc<Notify>) {
        for scheduled_task in self.tasks.iter() {
            let task_clone = Arc::clone(&scheduled_task.task);
            let delay = scheduled_task.delay;
            let sender = scheduled_task.sender.clone();
            let shutdown_notify = shutdown_notify.clone();

            tokio::spawn(async move {
                let mut task = task_clone.lock().await;
                match &mut *task {
                    Tasks::Simple(task) => task.run(delay).await,
                    Tasks::Graceful(graceful_task) => {
                        graceful_task
                            .run_graceful(delay, shutdown_notify, sender.unwrap())
                            .await
                    }
                };
            });
        }
    }

    pub fn graceful_task_count(&self) -> usize {
        self.graceful_tasks
    }
}
