use std::fmt::Display;
///
/// Different blockchains supported
///
pub enum Blockchain {
    Tendermint,
    Mezo,
    Babylon,
    CoreDao,
}

impl Blockchain {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tendermint" => Some(Blockchain::Tendermint),
            "mezo" => Some(Blockchain::Mezo),
            "babylon" => Some(Blockchain::Babylon),
            "coredao" => Some(Blockchain::CoreDao),
            _ => None,
        }
    }
}

impl Display for Blockchain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Blockchain::Tendermint => "Tendermint",
            Blockchain::Mezo => "Mezo",
            Blockchain::CoreDao => "CoreDao",
            Blockchain::Babylon => "Babylon",
        };
        write!(f, "{}", s)
    }
}
