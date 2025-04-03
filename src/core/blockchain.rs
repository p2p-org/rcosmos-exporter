///
/// Different blockchains supported
///
pub enum Blockchain {
    Tendermint,
    Mezo,
}

impl Blockchain {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tendermint" => Some(Blockchain::Tendermint),
            "mezo" => Some(Blockchain::Mezo),
            _ => None,
        }
    }
}
