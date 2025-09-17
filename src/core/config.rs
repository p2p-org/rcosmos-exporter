use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Network,
    Node,
}

/// General configuration for the exporter
#[derive(Debug, Deserialize, Clone)]
pub struct GeneralConfig {
    pub network: String,
    pub mode: Mode,
    pub chain_id: String,
    pub metrics: MetricsConfig,
    pub alerting: AlertingConfig,
    pub nodes: NodesConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetricsConfig {
    pub address: String,
    pub port: u16,
    pub path: String,
}

/// Node configuration for RPC and LCD endpoints
#[derive(Debug, Deserialize, Clone)]
pub struct NodeConfig {
    pub name: String,
    pub url: String,
    #[serde(rename = "healthEndpoint")]
    pub health_endpoint: String,
}

/// Network configuration, including node lists and module configs
#[derive(Debug, Deserialize, Clone)]
pub struct NetworkConfig {
    #[serde(default)]
    pub cometbft: CometBFTConfig,
    #[serde(default)]
    pub tendermint: TendermintConfig,
    #[serde(default)]
    pub mezo: MezoConfig,
    #[serde(default)]
    pub babylon: BabylonConfig,
    #[serde(default)]
    pub lombard: LombardConfig,
    #[serde(default)]
    pub namada: NamadaConfig,
    #[serde(default)]
    pub coredao: CoreDaoConfig,
    #[serde(default)]
    pub sei: SeiConfig,
    // Add more blockchain configs as needed
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodesConfig {
    pub rpc: Vec<NodeConfig>,
    #[serde(default)]
    pub lcd: Vec<NodeConfig>,
}

/// Top-level config struct for the exporter
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub node: NodeModeConfig,
    pub network: NetworkConfig,
}

/// CometBFT module configuration (all fields required)
#[derive(Debug, Deserialize, Clone)]
pub struct CometBFTConfig {
    #[serde(default)]
    pub validators: CometBFTValidatorsConfig,
    #[serde(default)]
    pub block: CometBFTBlockConfig,
}

impl Default for CometBFTConfig {
    fn default() -> Self {
        Self {
            validators: CometBFTValidatorsConfig::default(),
            block: CometBFTBlockConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CometBFTValidatorsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_10")]
    pub interval: u64,
}

impl Default for CometBFTValidatorsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 10,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CometBFTBlockConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_10")]
    pub interval: u64,
    #[serde(default = "default_window_500")]
    pub window: u64,
    #[serde(default)]
    pub tx: CometBFTBlockTxConfig,
    #[serde(default)]
    pub uptime: CometBFTBlockUptimeConfig,
}

impl Default for CometBFTBlockConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 10,
            window: 500,
            tx: CometBFTBlockTxConfig::default(),
            uptime: CometBFTBlockUptimeConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CometBFTBlockTxConfig {
    #[serde(default)]
    pub enabled: bool,
}

impl Default for CometBFTBlockTxConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CometBFTBlockUptimeConfig {
    #[serde(default)]
    pub persistence: bool,
}

impl Default for CometBFTBlockUptimeConfig {
    fn default() -> Self {
        Self { persistence: false }
    }
}

/// Tendermint module configuration (all fields required)
#[derive(Debug, Deserialize, Clone)]
pub struct TendermintConfig {
    #[serde(default)]
    pub bank: TendermintBankConfig,
    #[serde(default)]
    pub distribution: TendermintSubmoduleConfig,
    #[serde(default)]
    pub gov: TendermintSubmoduleConfig,
    #[serde(default)]
    pub staking: TendermintSubmoduleConfig,
    #[serde(default)]
    pub slashing: TendermintSubmoduleConfig,
    #[serde(default)]
    pub upgrade: TendermintSubmoduleConfig,
}

impl Default for TendermintConfig {
    fn default() -> Self {
        Self {
            bank: TendermintBankConfig::default(),
            distribution: TendermintSubmoduleConfig::default(),
            gov: TendermintSubmoduleConfig::default(),
            staking: TendermintSubmoduleConfig::default(),
            slashing: TendermintSubmoduleConfig::default(),
            upgrade: TendermintSubmoduleConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct TendermintBankConfig {
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_30")]
    pub interval: u64,
}

impl Default for TendermintBankConfig {
    fn default() -> Self {
        Self {
            addresses: Vec::new(),
            enabled: false,
            interval: 30,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct TendermintSubmoduleConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_30")]
    pub interval: u64,
}

impl Default for TendermintSubmoduleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 30,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct AlertingConfig {
    pub validators: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MezoConfig {
    #[serde(default)]
    pub poa: MezoPoaConfig,
}

impl Default for MezoConfig {
    fn default() -> Self {
        Self {
            poa: MezoPoaConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct MezoPoaConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_30")]
    pub interval: u64,
}

impl Default for MezoPoaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 30,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct BabylonConfig {
    #[serde(default)]
    pub bls: BabylonBlsConfig,
}

impl Default for BabylonConfig {
    fn default() -> Self {
        Self {
            bls: BabylonBlsConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct BabylonBlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_30")]
    pub interval: u64,
}

impl Default for BabylonBlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 30,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct LombardConfig {
    #[serde(default)]
    pub ledger: LombardLedgerConfig,
}

impl Default for LombardConfig {
    fn default() -> Self {
        Self {
            ledger: LombardLedgerConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct LombardLedgerConfig {
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_30")]
    pub interval: u64,
}

impl Default for LombardLedgerConfig {
    fn default() -> Self {
        Self {
            addresses: Vec::new(),
            enabled: false,
            interval: 30,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodeModeConfig {
    pub client: String,
    pub tendermint: NodeModeTendermintConfig,
    pub cometbft: NodeModeCometBFTConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodeModeTendermintConfig {
    #[serde(rename = "nodeInfo")]
    pub node_info: NodeModeTendermintNodeInfoConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodeModeTendermintNodeInfoConfig {
    pub enabled: bool,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodeModeCometBFTConfig {
    pub status: NodeModeCometBFTStatusConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodeModeCometBFTStatusConfig {
    pub enabled: bool,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NamadaConfig {
    #[serde(default)]
    pub account: NamadaAccountConfig,
    #[serde(default)]
    pub pos: NamadaPosConfig,
}

impl Default for NamadaConfig {
    fn default() -> Self {
        Self {
            account: NamadaAccountConfig::default(),
            pos: NamadaPosConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct NamadaAccountConfig {
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_30")]
    pub interval: u64,
}

impl Default for NamadaAccountConfig {
    fn default() -> Self {
        Self {
            addresses: Vec::new(),
            enabled: false,
            interval: 30,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct NamadaPosConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_30")]
    pub interval: u64,
}

impl Default for NamadaPosConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 30,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoreDaoStakingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_30")]
    pub interval: u64,
}

impl Default for CoreDaoStakingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 30,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoreDaoConfig {
    #[serde(default)]
    pub block: CoreDaoBlockConfig,
    #[serde(default)]
    pub validator: CoreDaoValidatorConfig,
    #[serde(default)]
    pub staking: CoreDaoStakingConfig,
}

impl Default for CoreDaoConfig {
    fn default() -> Self {
        Self {
            block: CoreDaoBlockConfig::default(),
            validator: CoreDaoValidatorConfig::default(),
            staking: CoreDaoStakingConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoreDaoBlockConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_30")]
    pub interval: u64,
    #[serde(default = "default_window_500")]
    pub window: u64,
}

impl Default for CoreDaoBlockConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 30,
            window: 500,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoreDaoValidatorConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_30")]
    pub interval: u64,
    #[serde(default)]
    pub api: CoreDaoValidatorApiConfig,
}

impl Default for CoreDaoValidatorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 30,
            api: CoreDaoValidatorApiConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct CoreDaoValidatorApiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_cache_duration")]
    pub cache_duration_seconds: u64,
}

impl CoreDaoValidatorApiConfig {
    /// Get the API key, preferring environment variable over config file
    pub fn get_api_key(&self) -> Option<String> {
        // First try environment variable
        std::env::var("COREDAO_VALIDATOR_API_KEY").ok()
            // Fall back to config file
            .or_else(|| if !self.api_key.is_empty() { Some(self.api_key.clone()) } else { None })
    }

    /// Get the API URL from config file
    pub fn get_url(&self) -> Option<String> {
        if !self.url.is_empty() {
            Some(self.url.clone())
        } else {
            None
        }
    }
}

fn default_cache_duration() -> u64 {
    300 // 5 minutes default cache duration
}

fn default_interval_10() -> u64 {
    10
}

fn default_interval_30() -> u64 {
    30
}


fn default_window_500() -> u64 {
    500
}

/// Sei module configuration
#[derive(Debug, Deserialize, Clone)]
pub struct SeiConfig {
    #[serde(default)]
    pub validators: SeiValidatorsConfig,
    #[serde(default)]
    pub block: SeiBlockConfig,
}

impl Default for SeiConfig {
    fn default() -> Self {
        Self {
            validators: SeiValidatorsConfig::default(),
            block: SeiBlockConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeiValidatorsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_10")]
    pub interval: u64,
}

impl Default for SeiValidatorsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 10,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeiBlockConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval_10")]
    pub interval: u64,
}

impl Default for SeiBlockConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 10,
        }
    }
}
