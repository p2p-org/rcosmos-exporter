use super::http_client::HttpClient;
use tracing::info;

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
        let client = BlockchainClient::new(self.rpc, self.rest);

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
    
    pub async fn post_json<T: serde::Serialize>(&self, endpoint: &str, payload: &T) -> Result<String, crate::core::clients::http_client::HTTPClientErrors> {
        let rpc = self.rpc.as_ref().expect("RPC client not initialized");
        let json_string = serde_json::to_string(payload).unwrap();
        
        // Use the new post method
        rpc.post(endpoint, json_string).await
    }

    pub async fn print_rpc_endpoints(&self) {
        if let Some(rpc) = &self.rpc {
            rpc.print_endpoints().await;
        } else {
            info!("RPC client not initialized");
        }
    }

}
