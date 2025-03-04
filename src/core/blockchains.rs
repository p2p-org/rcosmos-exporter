#[derive(Debug)]
pub enum BlockchainType {
    Tendermint,
}

impl BlockchainType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tendermint" => Some(BlockchainType::Tendermint),
            _ => None,
        }
    }
}
