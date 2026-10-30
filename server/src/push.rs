use anyhow::{bail, Context, Result};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

const TOKEN_FILE: &str = "push-tokens.json";
const TOKEN_AUDIENCE: &str = "https://oauth2.googleapis.com/token";
const TOKEN_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

#[derive(Clone)]
pub struct PushService {
    state_dir: PathBuf,
    client: reqwest::Client,
    token_lock: Arc<Mutex<()>>,
    cached_access_token: Arc<Mutex<Option<(String, Instant)>>>,
}

#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: String,
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'static str,
    aud: &'static str,
    iat: u64,
    exp: u64,
}

#[derive(Deserialize)]
struct OAuthResponse {
    access_token: String,
    expires_in: u64,
}

impl PushService {
    pub fn new(state_dir: PathBuf, client: reqwest::Client) -> Self {
        Self {
            state_dir,
            client,
            token_lock: Arc::new(Mutex::new(())),
            cached_access_token: Arc::new(Mutex::new(None)),
        }
    }

    pub fn configured(&self) -> bool {
        std::env::var_os("GPU_POCKET_FCM_CREDENTIALS")
            .is_some_and(|path| PathBuf::from(path).is_file())
    }

    fn credentials(&self) -> Result<ServiceAccount> {
        let path = std::env::var_os("GPU_POCKET_FCM_CREDENTIALS")
            .context("GPU_POCKET_FCM_CREDENTIALS is not configured")?;
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    pub async fn register_token(&self, token: &str) -> Result<()> {
        if !(32..=3500).contains(&token.len()) || !token.bytes().all(|b| b.is_ascii_graphic()) {
            bail!("invalid Firebase registration token");
        }
        let _guard = self.token_lock.lock().await;
        let mut tokens = self.read_tokens()?;
        if !tokens.iter().any(|existing| existing == token) {
            tokens.push(token.to_string());
            self.write_tokens(&tokens)?;
        }
        Ok(())
    }

    pub async fn unregister_token(&self, token: &str) -> Result<()> {
        let _guard = self.token_lock.lock().await;
        let mut tokens = self.read_tokens()?;
        let original_len = tokens.len();
        tokens.retain(|existing| existing != token);
        if tokens.len() != original_len {
            self.write_tokens(&tokens)?;
        }
        Ok(())
    }

    fn read_tokens(&self) -> Result<Vec<String>> {
        let path = self.state_dir.join(TOKEN_FILE);
        if !path.exists() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    fn write_tokens(&self, tokens: &[String]) -> Result<()> {
        std::fs::create_dir_all(&self.state_dir)?;
        let temp = self.state_dir.join(format!(".{TOKEN_FILE}.tmp"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let mut file = options.open(&temp)?;
        file.write_all(&serde_json::to_vec(tokens)?)?;
        file.sync_all()?;
        std::fs::rename(temp, self.state_dir.join(TOKEN_FILE))?;
        Ok(())
    }

    async fn access_token(&self, account: &ServiceAccount) -> Result<String> {
        let mut cached = self.cached_access_token.lock().await;
        if let Some((token, expires_at)) = cached.as_ref() {
            if *expires_at > Instant::now() + Duration::from_secs(60) {
                return Ok(token.clone());
            }
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let assertion = encode(
            &Header::new(Algorithm::RS256),
            &JwtClaims {
                iss: &account.client_email,
                scope: TOKEN_SCOPE,
                aud: TOKEN_AUDIENCE,
                iat: now,
                exp: now + 3600,
            },
            &EncodingKey::from_rsa_pem(account.private_key.as_bytes())?,
        )?;
        let response: OAuthResponse = self
            .client
            .post(TOKEN_AUDIENCE)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let expires_at = Instant::now() + Duration::from_secs(response.expires_in);
        *cached = Some((response.access_token.clone(), expires_at));
        Ok(response.access_token)
    }

    pub async fn send(&self, title: &str, body: &str) -> Result<()> {
        let account = self.credentials()?;
        let tokens = {
            let _guard = self.token_lock.lock().await;
            self.read_tokens()?
        };
        if tokens.is_empty() {
            bail!("no registered Firebase devices");
        }
        let access_token = self.access_token(&account).await?;
        let endpoint = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            account.project_id
        );
        let mut invalid_tokens = Vec::new();
        let mut failures = Vec::new();
        for token in tokens {
            let response = self
                .client
                .post(&endpoint)
                .bearer_auth(&access_token)
                .json(&json!({
                    "message": {
                        "token": token,
                        "notification": {"title": title, "body": body},
                        "android": {"priority": "high"}
                    }
                }))
                .send()
                .await?;
            if !response.status().is_success() {
                let status = response.status();
                let details = response.text().await?;
                if status == reqwest::StatusCode::NOT_FOUND && details.contains("UNREGISTERED") {
                    invalid_tokens.push(token);
                } else {
                    failures.push(format!("HTTP {status}: {details}"));
                }
            }
        }
        if !invalid_tokens.is_empty() {
            let _guard = self.token_lock.lock().await;
            let mut current = self.read_tokens()?;
            current.retain(|token| !invalid_tokens.contains(token));
            self.write_tokens(&current)?;
        }
        if !failures.is_empty() {
            bail!(
                "Firebase Cloud Messaging failed for {} device(s): {}",
                failures.len(),
                failures.join("; ")
            );
        }
        Ok(())
    }
}
