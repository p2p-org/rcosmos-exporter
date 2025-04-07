use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::sync::Mutex;

#[async_trait]
pub trait Task: Send {
    async fn run(&mut self, delay: Duration);
}

pub struct ExporterTask {
    task: Arc<Mutex<Box<dyn Task + Send>>>,
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
    pub fn start(&self) {
        for scheduled_task in self.tasks.iter() {
            let task_clone = Arc::clone(&scheduled_task.task);
            let delay = scheduled_task.delay;

            tokio::spawn(async move {
                let mut task = task_clone.lock().await;
                task.run(delay).await;
            });
        }
    }
}
