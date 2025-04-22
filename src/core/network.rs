use std::fmt::Display;

#[derive(Clone)]
pub enum Network {
    BabylonMainnet,
    BabylonTestnet,
    CoreDaoTestnet,
    MezoTestnet,
    PellTestnet,
}

impl Network {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "babylon-mainnet" => Some(Network::BabylonMainnet),
            "babylon-testnet" => Some(Network::BabylonTestnet),
            "mezo-testnet" => Some(Network::MezoTestnet),
            "coredao-testnet" => Some(Network::CoreDaoTestnet),
            "pell-testnet" => Some(Network::PellTestnet),
            _ => None,
        }
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Network::BabylonMainnet => "babylon-mainnet",
            Network::BabylonTestnet => "babylon-testnet",
            Network::CoreDaoTestnet => "coredao-testnet",
            Network::MezoTestnet => "mezo-testnet",
            Network::PellTestnet => "pell-testnet",
        };
        write!(f, "{}", s)
    }
}
