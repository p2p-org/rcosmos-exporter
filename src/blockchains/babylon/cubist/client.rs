use reqwest::{
    header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT},
    Client as HttpClient, Method,
};
use serde::Serialize;
use thiserror::Error;
use tokio::time::{sleep, Duration};

use super::session_manager::{Initialized, SessionManager, SessionManagerError};

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
    #[error("Session Error: {0}")]
    SessionError(#[from] SessionManagerError),
    #[error("Empty Data passed")]
    EmtpyData,
    #[error("Max retries reached")]
    MaxRetries,
}

pub struct ClientConfig {
    pub update_retry_delays_ms: Vec<u64>,
}

pub struct Client {
    pub session_manager: SessionManager<Initialized>,
    config: ClientConfig,
}

impl Client {
    // Constructor for BaseClient
    pub async fn new(secret_id: String, secret_path: String) -> anyhow::Result<Self> {
        let session_manager = SessionManager::new();
        let session_manager = session_manager.init(secret_id, secret_path).await?;
        Ok(Self {
            session_manager,
            config: ClientConfig {
                update_retry_delays_ms: vec![100, 200, 400],
            },
        })
    }

    // Executes a fetch operation with retry logic
    pub async fn fetch<T>(
        &mut self,
        path: &str,
        method: Method,
        data: Option<T>,
    ) -> Result<String, ClientError>
    where
        T: Serialize,
    {
        let stack = &self
            .session_manager
            .session
            .env
            .dev_cub_signer_stack
            .clone();

        let base_url = stack
            .as_ref()
            .expect("Could not get cube signer stack")
            .signer_api_root
            .trim_end_matches('/');

        // Retry logic
        let mut retries = 0;
        let max_retries = self.config.update_retry_delays_ms.len();

        loop {
            let client = HttpClient::new();
            let url = format!("{}/{}", &base_url, path);

            let client = match method.as_str() {
                "GET" => client.get(&url),
                _ => {
                    if let Some(ref data) = data {
                        client.request(method.clone(), &url).json(data)
                    } else {
                        return Err(ClientError::EmtpyData);
                    }
                }
            };

            let refreshed_token = &self.session_manager.token().await?;

            let response = client
                .header(CONTENT_TYPE, "application/json") // Content-Type header
                .header(AUTHORIZATION, refreshed_token) // Authorization header
                .header(USER_AGENT, "@cubist-labs/cubesigner-sdk@0.4.137-0")
                .header("X-Cubist-Ts-Sdk", "@cubist-labs/cubesigner-sdk@0.4.137-0")
                .send()
                .await?;

            let status = response.status().clone();
            let response = response.text().await.unwrap();

            if status.is_success() {
                return Ok(response);
            } else {
                if retries < max_retries {
                    let delay = self.config.update_retry_delays_ms[retries];
                    retries += 1;
                    sleep(Duration::from_millis(delay)).await;
                } else {
                    return Err(ClientError::MaxRetries);
                }
            }
        }
    }
}
