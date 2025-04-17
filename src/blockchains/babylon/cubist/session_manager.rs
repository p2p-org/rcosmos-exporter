use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::marker::PhantomData;
use std::path::Path;
use std::time::SystemTime;
use thiserror::Error;
use tokio::{fs, io};
use tracing::info;
use vaultrs::client::{VaultClient, VaultClientSettingsBuilder};
use vaultrs::kv2;

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
    #[error("Could not read vault token")]
    VaultTokenReadError(#[from] io::Error),
    #[error("Could not find the vault token path")]
    VaultTokenPathDoesNotExist,
}

pub struct Initialized;
pub struct Uninitialized;

#[derive(Debug, Deserialize, Serialize)]
struct SessionSecret {
    key: String,
    session: String,
}

pub struct SessionManager<State = Initialized> {
    pub session: SessionData,
    state: PhantomData<State>,
    vault_token: String,
}

pub const DEFAULT_EXPIRATION_BUFFER_SECS: u64 = 30;

impl SessionManager<Uninitialized> {
    pub fn new() -> Self {
        Self {
            session: SessionData::default(),
            state: PhantomData::<Uninitialized>,
            vault_token: String::new(),
        }
    }

    pub async fn init(
        &self,
        secret_id: String,
        secret_path: String,
    ) -> Result<SessionManager<Initialized>, anyhow::Error> {
        let session = self.fetch_session(secret_id).await?;

        let session = serde_json::from_str::<SessionData>(&session)?;

        Ok(SessionManager {
            vault_token: self.vault_token.clone(),
            session: session,
            state: PhantomData::<Initialized>,
        })
    }

    async fn fetch_session(
        &self,
        secret_id: String,
        secret_path: String,
    ) -> Result<String, anyhow::Error> {
        let host = "https://vault.infra.p2p.org/";

        let token_path = "/var/run/secrets/kubernetes.io/serviceaccount/token";

        if !Path::new(token_path).exists() {
            return Err(SessionManagerError::VaultTokenPathDoesNotExist.into());
        }

        let token = match fs::read_to_string(token_path).await {
            Ok(token) => token,
            Err(e) => return Err(SessionManagerError::VaultTokenReadError(e).into()),
        };

        let client = VaultClient::new(
            VaultClientSettingsBuilder::default()
                .address(host)
                .token(token)
                .build()
                .unwrap(),
        )
        .unwrap();

        let secret = SessionSecret {
            key: secret_id.clone(),
            session:  r#"
            {
              "org_id": "Org#ffea7e1a-7ddb-4348-8bc4-8cd86bef3b98",
              "role_id": null,
              "expiration": 1775817096,
              "purpose": "User session with scopes [\"manage:org:*\"]",
              "token": "3d6fd7397:MzQwOGIxM2EtZDBjZi00NzhlLTgzYTAtNmU5ZTRiOTFjMjZj.eyJlcG9jaF9udW0iOjEsImVwb2NoX3Rva2VuIjoiWGsxTW1MRFFGdlQ0Rk5CQmZ5ZG5YRW1VdEV4ajNXMjBYNEZzd3hjZ2Q3Yz0iLCJvdGhlcl90b2tlbiI6IlJpWWN1czV1d0oraTFleXowUm1zWTlsOVRYMU4yVkt5TG9xV2hBd0NTL3M9In0=",
              "refresh_token": "3d6fd7397:MzQwOGIxM2EtZDBjZi00NzhlLTgzYTAtNmU5ZTRiOTFjMjZj.eyJlcG9jaF9udW0iOjEsImVwb2NoX3Rva2VuIjoiWGsxTW1MRFFGdlQ0Rk5CQmZ5ZG5YRW1VdEV4ajNXMjBYNEZzd3hjZ2Q3Yz0iLCJvdGhlcl90b2tlbiI6IlJpWWN1czV1d0oraTFleXowUm1zWTlsOVRYMU4yVkt5TG9xV2hBd0NTL3M9In0=.L5j7wuzGvtPpw1VIuoHQcPJa+XKIY5Jvvfm9T2yylbw=",
              "env": {
                "Dev-CubeSignerStack": {
                  "ClientId": "1tiou9ecj058khiidmhj4ds4rj",
                  "DefaultCredentialRpId": "cubist.dev",
                  "GoogleDeviceClientId": "59575607964-nc9hjnjka7jlb838jmg40qes4dtpsm6e.apps.googleusercontent.com",
                  "GoogleDeviceClientSecret": "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",
                  "Region": "us-east-1",
                  "SignerApiRoot": "https://gamma.signer.cubist.dev",
                  "UserPoolId": "us-east-1_RU7HEslOW"
                }
              },
              "session_info": {
                "auth_token": "RiYcus5uwJ+i1eyz0RmsY9l9TX1N2VKyLoqWhAwCS/s=",
                "auth_token_exp": 1744388651,
                "epoch": 1,
                "epoch_token": "Xk1MmLDQFvT4FNBBfydnXEmUtExj3W20X4Fswxcgd7c=",
                "refresh_token": "L5j7wuzGvtPpw1VIuoHQcPJa+XKIY5Jvvfm9T2yylbw=",
                "refresh_token_exp": 1744474751,
                "session_id": "3408b13a-d0cf-478e-83a0-6e9e4b91c26c"
              }
            }
            "#.to_string()
        };

        info!("Setting initial secret: {:?}", secret);
        kv2::set(&client, "secret", &secret_path, &secret).await?;

        let secret: SessionSecret = kv2::read(&client, "secret", &secret_path).await?;
        info!("Fetched secret from Vault: {:?}", secret);

        Ok(secret.session.to_string())
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

    pub async fn write_session_secret(&self) -> anyhow::Result<()> {
        let host = "https://vault.infra.p2p.org/";

        let token_path = "/var/run/secrets/kubernetes.io/serviceaccount/token";

        if !Path::new(token_path).exists() {
            return Err(SessionManagerError::VaultTokenPathDoesNotExist.into());
        }

        let token = match fs::read_to_string(token_path).await {
            Ok(token) => token,
            Err(e) => return Err(SessionManagerError::VaultTokenReadError(e).into()),
        };

        let client = VaultClient::new(
            VaultClientSettingsBuilder::default()
                .address(host)
                .token(token)
                .build()
                .unwrap(),
        )
        .unwrap();

        let secret = SessionSecret {
            key: "babylon-testnet".to_string(),
            session: serde_json::to_string(&self.session)?,
        };

        kv2::set(&client, "secret", "mysecret", &secret).await?;
        Ok(())
    }
}
