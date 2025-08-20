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
    pub cometbft: CometBFTConfig,
    pub tendermint: TendermintConfig,
    pub mezo: MezoConfig,
    pub babylon: BabylonConfig,
    pub lombard: LombardConfig,
    pub namada: NamadaConfig,
    pub coredao: CoreDaoConfig,
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
    pub validators: CometBFTValidatorsConfig,
    pub block: CometBFTBlockConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CometBFTValidatorsConfig {
    pub enabled: bool,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CometBFTBlockConfig {
    pub enabled: bool,
    pub interval: u64,
    pub window: u64,
    pub tx: CometBFTBlockTxConfig,
    pub uptime: CometBFTBlockUptimeConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CometBFTBlockTxConfig {
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CometBFTBlockUptimeConfig {
    pub persistence: bool,
}

/// Tendermint module configuration (all fields required)
#[derive(Debug, Deserialize, Clone)]
pub struct TendermintConfig {
    pub bank: TendermintBankConfig,
    pub distribution: TendermintSubmoduleConfig,
    pub gov: TendermintSubmoduleConfig,
    pub staking: TendermintSubmoduleConfig,
    pub slashing: TendermintSubmoduleConfig,
    pub upgrade: TendermintSubmoduleConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TendermintBankConfig {
    pub addresses: Vec<String>,
    pub enabled: bool,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TendermintSubmoduleConfig {
    pub enabled: bool,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AlertingConfig {
    pub validators: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MezoConfig {
    pub poa: MezoPoaConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MezoPoaConfig {
    pub enabled: bool,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BabylonConfig {
    pub bls: BabylonBlsConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BabylonBlsConfig {
    pub enabled: bool,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LombardConfig {
    pub ledger: LombardLedgerConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LombardLedgerConfig {
    pub addresses: Vec<String>,
    pub enabled: bool,
    pub interval: u64,
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
    pub account: NamadaAccountConfig,
    pub pos: NamadaPosConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NamadaAccountConfig {
    pub addresses: Vec<String>,
    pub enabled: bool,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NamadaPosConfig {
    pub enabled: bool,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoreDaoConfig {
    pub block: CoreDaoBlockConfig,
    pub validator: CoreDaoValidatorConfig,
    pub staking: CoreDaoStakingConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoreDaoBlockConfig {
    pub enabled: bool,
    pub interval: u64,
    pub window: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoreDaoValidatorConfig {
    pub enabled: bool,
    pub interval: u64,
    #[serde(default)]
    pub api: CoreDaoValidatorApiConfig,
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

#[derive(Debug, Deserialize, Clone)]
pub struct CoreDaoStakingConfig {
    pub enabled: bool,
    pub interval: u64,
}
