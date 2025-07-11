use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::marker::PhantomData;
use std::time::SystemTime;
use thiserror::Error;

#[derive(Deserialize, Serialize, Default, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct DevCubeSignerStack {
    pub client_id: String,
    pub default_credential_rp_id: String,
    pub google_device_client_id: String,
    pub google_device_client_secret: String,
    pub region: String,
    pub signer_api_root: String,
    pub user_pool_id: String,
}

#[derive(Deserialize, Serialize, Default)]
pub struct Env {
    #[serde(rename = "Dev-CubeSignerStack")]
    pub dev_cub_signer_stack: Option<DevCubeSignerStack>,
}

#[derive(Deserialize, Serialize, Default)]
pub struct SessionData {
    pub org_id: String,
    pub role_id: Option<String>,
    pub expiration: usize,
    pub purpose: Option<String>,
    pub token: String,
    pub refresh_token: String,
    pub env: Env,
    pub session_info: ClientSessionInfo,
}

#[derive(Deserialize, Serialize, Default)]
pub struct ClientSessionInfo {
    pub session_id: String,
    pub auth_token: String,
    pub refresh_token: String,
    pub epoch: u64,
    pub epoch_token: String,
    pub auth_token_exp: u64,
    pub refresh_token_exp: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum TokenPathResponses {
    Success(TokenResponse),
    Error(ErrorResponse),
}

#[derive(Deserialize)]
pub struct TokenResponse {
    token: String,
    refresh_token: String,
    expiration: usize,
    session_info: ClientSessionInfo,
}

#[derive(Deserialize)]
pub struct ErrorResponse {
    message: String,
    error_code: String,
}

#[derive(Error, Debug)]
pub enum SessionManagerError {
    #[error("Request to refresh token did not have 200 as status code")]
    TokenRefreshNot200,
    #[error("api returned message: {message:?}, with error code {error_code:?}")]
    ErrorReturned { message: String, error_code: String },
    #[error("Could not obtain session from secret manager")]
    SecretManagerError,
}

pub struct Initialized;
pub struct Uninitialized;

pub struct SessionManager<State = Initialized> {
    pub session: SessionData,
    state: PhantomData<State>,
}

pub const DEFAULT_EXPIRATION_BUFFER_SECS: u64 = 30;

impl SessionManager<Uninitialized> {
    pub fn new() -> Self {
        Self {
            session: SessionData::default(),
            state: PhantomData::<Uninitialized>,
        }
    }

    pub async fn init(
        &self,
        secret_id: String,
    ) -> Result<SessionManager<Initialized>, anyhow::Error> {
        let session = self.fetch_session(secret_id).await?;
        let session = serde_json::from_str::<SessionData>(&session)?;

        Ok(SessionManager {
            session: session,
            state: PhantomData::<Initialized>,
        })
    }

    async fn fetch_session(&self, secret_id: String) -> Result<String, anyhow::Error> {
        Ok(r#"
{

}
        "#
        .to_string())
    }
}

impl SessionManager<Initialized> {
    async fn refresh(&mut self) -> Result<String, SessionManagerError> {
        let client = Client::new();

        let url = format!(
            "{}/v1/org/{}/token/refresh",
            &self
                .session
                .env
                .dev_cub_signer_stack
                .clone()
                .expect("Could not get cube signer stack")
                .signer_api_root
                .trim_end_matches('/'),
            urlencoding::encode(&self.session.org_id).to_string()
        );

        let body = json!({
            "epoch_num": &self.session.session_info.epoch,
            "epoch_token": &self.session.session_info.epoch_token,
            "other_token": &self.session.session_info.refresh_token
        });

        // Send the PATCH request
        let response = client
            .patch(url)
            .header(CONTENT_TYPE, "application/json") // Content-Type header
            .header(AUTHORIZATION, &self.session.token) // Authorization header
            .header(USER_AGENT, "curl")
            .json(&body) // Attach JSON body
            .send()
            .await
            .expect("Could not send request when refreshing token");

        if response.status() != 200 {
            return Err(SessionManagerError::TokenRefreshNot200);
        }

        let response: TokenPathResponses = response
            .json()
            .await
            .expect("Could not decode token response");

        match response {
            TokenPathResponses::Success(response) => {
                self.session.token = response.token;
                self.session.refresh_token = response.refresh_token;
                self.session.expiration = response.expiration;

                self.session.session_info.session_id = response.session_info.session_id;
                self.session.session_info.auth_token = response.session_info.auth_token;
                self.session.session_info.refresh_token = response.session_info.refresh_token;
                self.session.session_info.epoch = response.session_info.epoch;
                self.session.session_info.epoch_token = response.session_info.epoch_token;
                self.session.session_info.auth_token_exp = response.session_info.auth_token_exp;
                self.session.session_info.refresh_token_exp =
                    response.session_info.refresh_token_exp;
                Ok(self.session.token.clone())
            }
            TokenPathResponses::Error(error) => Err(SessionManagerError::ErrorReturned {
                message: error.message,
                error_code: error.error_code,
            }),
        }
    }

    fn is_within_buffer(&self, time_in_seconds: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        time_in_seconds < (now.as_secs() + DEFAULT_EXPIRATION_BUFFER_SECS)
    }

    fn is_stale(&self) -> bool {
        self.is_within_buffer(self.session.session_info.auth_token_exp)
    }

    pub async fn token(&mut self) -> Result<String, SessionManagerError> {
        if self.is_stale() {
            self.refresh().await
        } else {
            Ok(self.session.token.clone())
        }
    }
}
