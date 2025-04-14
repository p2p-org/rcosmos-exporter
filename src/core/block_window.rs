use std::collections::{HashMap, VecDeque};

/// Tracks blocks, signed blocks by blocks and timestamps of a given block window
pub struct BlockWindow {
    blocks: VecDeque<Vec<String>>,
    pub window: usize,
}

impl BlockWindow {
    pub fn new(window: usize) -> Self {
        Self {
            blocks: VecDeque::with_capacity(500),
            window,
        }
    }

    pub fn add_block_signers(&mut self, signers: Vec<String>) {
        self.blocks.push_back(signers);

        if self.blocks.len() > self.window {
            self.blocks.pop_front();
        }
    }

    pub fn uptimes(&self) -> HashMap<String, f64> {
        let mut signer_counts: HashMap<String, f64> = HashMap::new();

        for block_signers in &self.blocks {
            for signer in block_signers {
                *signer_counts.entry(signer.clone()).or_insert(0.0) += 1.0;
            }
        }

        signer_counts
            .iter()
            .map(|(key, value)| (key.clone(), (value / (self.window as f64)) * 100.0))
            .collect()
    }
}
