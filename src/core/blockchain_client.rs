use super::http_client::HttpClient;

///
/// Ensures that BlockchainClients start with health checks
/// for http endpoints
///
pub struct BlockchainClientBuilder {
    validator_address: String,
    block_window: i64,
    rpc: Option<HttpClient>,
    rest: Option<HttpClient>,
}

impl BlockchainClientBuilder {
    pub fn new(validator_address: String, block_window: i64) -> Self {
        Self {
            validator_address,
            block_window,
            rpc: None,
            rest: None,
        }
    }

    pub fn with_rpc(mut self, rpc: Option<HttpClient>) -> Self {
        match rpc {
            Some(rpc) => {
                rpc.start_health_checks();
                self.rpc = Some(rpc)
            }
            None => panic!("RPC is not initialized"),
        }
        self
    }

    pub fn with_rest(mut self, rest: Option<HttpClient>) -> Self {
        match rest {
            Some(rest) => {
                rest.start_health_checks();
                self.rest = Some(rest)
            }
            None => panic!("REST is not initialized"),
        }
        self
    }

    pub async fn build(self) -> BlockchainClient {
        let client = BlockchainClient::new(
            self.validator_address,
            self.block_window,
            self.rpc,
            self.rest,
        );

        client
    }
}

///
/// Do not use BlockchainClient struct to create a new client
/// instead use BlockchainClientBuilder for safe creation.
///
/// Use with_rest and with_api to safe access for HttpClients
///
pub struct BlockchainClient {
    pub validator_address: String,

    pub proccessed_height: i64,
    pub block_window: i64,
    pub chain_id: String,

    pub rpc: Option<HttpClient>,
    pub rest: Option<HttpClient>,
    // grpc: Option<GrpcClient>
}

impl BlockchainClient {
    pub fn new(
        validator_address: String,
        block_window: i64,
        rpc: Option<HttpClient>,
        rest: Option<HttpClient>,
    ) -> Self {
        Self {
            validator_address,
            proccessed_height: 0,
            chain_id: "Unknown".to_string(),
            block_window,
            rpc,
            rest,
        }
    }

    pub fn with_rest(&self) -> &HttpClient {
        match &self.rest {
            Some(rest) => rest,
            None => panic!("REST client not initialized"),
        }
    }

    pub fn with_rpc(&self) -> &HttpClient {
        match &self.rpc {
            Some(rpc) => rpc,
            None => panic!("RPC client not initialized"),
        }
    }
}
