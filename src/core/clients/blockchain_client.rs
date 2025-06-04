use super::http_client::HttpClient;

///
/// Ensures that BlockchainClients start with health checks
/// for http endpoints
///
pub struct BlockchainClientBuilder {
    rpc: Option<HttpClient>,
    rest: Option<HttpClient>,
    //grpc client
}

impl BlockchainClientBuilder {
    pub fn new() -> Self {
        Self {
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
        if let Some(rest) = rest {
            rest.start_health_checks();
            self.rest = Some(rest)
        }
        self
    }

    pub async fn build(self) -> BlockchainClient {
        BlockchainClient::new(self.rpc, self.rest)
    }
}

///
/// Do not use BlockchainClient struct to create a new client
/// instead use BlockchainClientBuilder for safe creation.
///
/// Use with_rest and with_api to safe access for HttpClients
///
pub struct BlockchainClient {
    pub rpc: Option<HttpClient>,
    pub rest: Option<HttpClient>,
    // grpc: Option<GrpcClient>
}

impl BlockchainClient {
    pub fn new(rpc: Option<HttpClient>, rest: Option<HttpClient>) -> Self {
        Self { rpc, rest }
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
