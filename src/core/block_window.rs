use std::collections::HashMap;

/// Tracks blocks, signed blocks by blocks and timestamps of a given block window
pub struct BlockWindow {
    pub validator_signed_blocks: HashMap<String, isize>,
    pub window: usize,
    processed: usize,
}

impl BlockWindow {
    pub fn new(window: usize) -> Self {
        Self {
            validator_signed_blocks: HashMap::default(),
            window,
            processed: 0,
        }
    }

    pub fn add_block_signers(&mut self, signers: Vec<String>) {
        for signer in signers {
            self.validator_signed_blocks
                .entry(signer.to_string())
                .and_modify(|v| *v += 1)
                .or_insert(1);
        }

        if self.processed == self.window {
            for val in self.validator_signed_blocks.values_mut() {
                *val -= 1;
            }
        } else {
            self.processed += 1;
        }
    }

    pub fn uptimes(&self) -> HashMap<String, f64> {
        self.validator_signed_blocks
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    ((*value as f64) / (self.window as f64) * 100.0),
                )
            })
            .collect()
    }
}
