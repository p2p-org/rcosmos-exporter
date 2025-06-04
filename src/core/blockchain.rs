use std::fmt::Display;
///
/// Different blockchains supported
///
#[derive(PartialEq)]
pub enum Blockchain {
    Tendermint,
    Mezo,
    Babylon,
    CoreDao,
    Lombard,
    Noble,
    Astria,
}

impl Blockchain {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tendermint" => Some(Blockchain::Tendermint),
            "mezo" => Some(Blockchain::Mezo),
            "babylon" => Some(Blockchain::Babylon),
            "coredao" => Some(Blockchain::CoreDao),
            "lombard" => Some(Blockchain::Lombard),
            "noble" => Some(Blockchain::Noble),
            "astria" => Some(Blockchain::Astria),
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
            Blockchain::Lombard => "Lombard",
            Blockchain::Noble => "Noble",
            Blockchain::Astria => "Astria",
        };
        write!(f, "{}", s)
    }
}
