use std::collections::VecDeque;

/// Tracks blocks, signed blocks by blocks and timestamps of a given block window
pub struct BlockWindow {
    blocks: VecDeque<Vec<String>>,
    pub window: usize,
}

impl BlockWindow {
    pub fn new(window: usize) -> Self {
        Self {
            blocks: VecDeque::with_capacity(window),
            window,
        }
    }

    pub fn add_block_signers(&mut self, signers: Vec<String>) {
        self.blocks.push_back(signers);

        if self.blocks.len() > self.window {
            self.blocks.pop_front();
        }
    }

    // Get access to the blocks for custom uptime calculations
    pub fn blocks(&self) -> &VecDeque<Vec<String>> {
        &self.blocks
    }
}
