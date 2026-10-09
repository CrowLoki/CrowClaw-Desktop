use super::protocol::{self, PendingSignIn, TokenReply, AUTH_TIMEOUT};
use super::{
    open_credentials, protect_credentials, CodexImageCredentials, MembershipAccount,
    MembershipCatalog, MembershipCredentials, MembershipModel,
};
use crate::{agent::CancellationToken, storage::Storage};
use jsonwebtoken::jwk::JwkSet;
use reqwest::{Client, RequestBuilder, Response, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions, TryLockError},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignInRequest {
    pub request_id: String,
    pub label: String,
    pub account_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignOutResult {
    pub account: MembershipAccount,
    pub remote_revoked: bool,
    pub detail: String,
}

const CODEX_DEVICE_CODE: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const CODEX_DEVICE_POLL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const CODEX_TOKEN: &str = "https://auth.openai.com/oauth/token";
const CODEX_DEVICE_PAGE: &str = "https://auth.openai.com/codex/device";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexImageAuthStatus {
    pub state: String,
    pub verification_url: Option<String>,
    pub user_code: Option<String>,
    pub poll_interval_seconds: Option<u64>,
    pub message: Option<String>,
}
#[derive(Clone, Deserialize)]
struct CodexDeviceCodeReply {
    device_auth_id: String,
    user_code: String,
    interval: Option<serde_json::Value>,
    expires_in: Option<serde_json::Value>,
}
#[derive(Clone, Deserialize)]
struct CodexDeviceAuthorizationReply {
    authorization_code: String,
    code_verifier: String,
}
#[derive(Deserialize)]
struct CodexTokenReply {
    id_token: Option<String>,
    access_token: String,
    refresh_token: Option<String>,
    token_type: String,
    expires_in: u64,
}
#[derive(Clone)]
struct PendingCodexImageAuth {
    device_auth_id: String,
    user_code: String,
    poll_interval_seconds: u64,
    expires_at_ms: i64,
}

struct PendingRequest {
    cancellation: CancellationToken,
    complete: bool,
    account_id: Option<String>,
}
struct AccountSession {
    version: u32,
    cancellation: CancellationToken,
}
pub struct MembershipService {
    pub(crate) storage: Arc<Storage>,
    pub(crate) client: Client,
    pending: Mutex<HashMap<String, PendingRequest>>,
    sessions: Mutex<HashMap<String, AccountSession>>,
    codex_image_pending: Mutex<HashMap<String, PendingCodexImageAuth>>,
    #[cfg(test)]
    test_discovery: Option<Discovery>,
}

impl MembershipService {
    pub fn new(storage: Arc<Storage>) -> Result<Self, String> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|_| "Could not initialize the membership HTTP client")?;
        Ok(Self {
            storage,
            client,
            pending: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            codex_image_pending: Mutex::new(HashMap::new()),
            #[cfg(test)]
            test_discovery: None,
        })
    }

    pub fn accounts(&self) -> Result<Vec<MembershipAccount>, String> {
        self.storage
            .membership_accounts()
            .map_err(|e| e.to_string())
    }

    pub async fn begin_codex_image_authorization(
        &self,
        id: &str,
    ) -> Result<CodexImageAuthStatus, String> {
        let account = self
            .storage
            .membership_account(id)
            .map_err(|e| e.to_string())?;
        if account.identity.provider != "chatgpt" || !account.has_credentials {
            return Err("Sign in to this ChatGPT account before enabling its image tools".into());
        }
        if let Some(existing) = self
            .codex_image_pending
            .lock()
            .map_err(|_| "Codex image authorization state is unavailable")?
            .get(id)
            .cloned()
        {
            if (protocol::unix_seconds()? as i64).saturating_mul(1000) < existing.expires_at_ms {
                return Ok(CodexImageAuthStatus {
                    state: "pending".into(),
                    verification_url: Some(CODEX_DEVICE_PAGE.into()),
                    user_code: Some(existing.user_code),
                    poll_interval_seconds: Some(existing.poll_interval_seconds),
                    message: None,
                });
            }
        }
        let cancellation = self.session_cancellation(id)?;
        let response = self
            .send(
                self.client
                    .post(CODEX_DEVICE_CODE)
                    .json(&serde_json::json!({"client_id":protocol::CODEX_IMAGE_CLIENT_ID}))
                    .timeout(Duration::from_secs(20)),
                &cancellation,
            )
            .await?;
        let status = response.status();
        let bytes = read_body(response, &cancellation, 256 * 1024).await?;
        if !status.is_success() {
            return Err(provider_failure(status.as_u16(), &bytes));
        }
        let reply: CodexDeviceCodeReply = serde_json::from_slice(&bytes)
            .map_err(|_| "OpenAI returned an invalid Codex authorization challenge")?;
        if reply.device_auth_id.trim().is_empty()
            || reply.device_auth_id.len() > 4096
            || reply.user_code.len() > 64
            || reply.user_code.is_empty()
            || !reply
                .user_code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("OpenAI returned an invalid Codex authorization challenge".into());
        }
        let interval = reply
            .interval
            .as_ref()
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            .unwrap_or(5)
            .clamp(3, 15);
        let ttl = reply
            .expires_in
            .as_ref()
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .unwrap_or(900)
            .clamp(60, 1800);
        let expires_at_ms = i64::try_from(protocol::unix_seconds()?)
            .ok()
            .and_then(|v| v.checked_add(ttl as i64))
            .and_then(|v| v.checked_mul(1000))
            .ok_or("Codex authorization expiry is invalid")?;
        self.codex_image_pending
            .lock()
            .map_err(|_| "Codex image authorization state is unavailable")?
            .insert(
                id.into(),
                PendingCodexImageAuth {
                    device_auth_id: reply.device_auth_id,
                    user_code: reply.user_code.clone(),
                    poll_interval_seconds: interval,
                    expires_at_ms,
                },
            );
        Ok(CodexImageAuthStatus {
            state: "pending".into(),
            verification_url: Some(CODEX_DEVICE_PAGE.into()),
            user_code: Some(reply.user_code),
            poll_interval_seconds: Some(interval),
            message: None,
        })
    }

    pub async fn poll_codex_image_authorization(
        &self,
        id: &str,
    ) -> Result<CodexImageAuthStatus, String> {
        let pending = self
            .codex_image_pending
            .lock()
            .map_err(|_| "Codex image authorization state is unavailable")?
            .get(id)
            .cloned();
        let Some(pending) = pending else {
            return Ok(CodexImageAuthStatus {
                state: "not_connected".into(),
                verification_url: None,
                user_code: None,
                poll_interval_seconds: None,
                message: None,
            });
        };
        if (protocol::unix_seconds()? as i64).saturating_mul(1000) >= pending.expires_at_ms {
            self.cancel_codex_image_authorization(id)?;
            return Ok(CodexImageAuthStatus {
                state: "expired".into(),
                verification_url: None,
                user_code: None,
                poll_interval_seconds: None,
                message: Some("Codex image sign-in expired. Start again when ready.".into()),
            });
        }
        let cancellation = self.session_cancellation(id)?;
        let response=self.send(self.client.post(CODEX_DEVICE_POLL).json(&serde_json::json!({"device_auth_id":pending.device_auth_id,"user_code":pending.user_code})).timeout(Duration::from_secs(20)),&cancellation).await?;
        let status = response.status();
        let bytes = read_body(response, &cancellation, 256 * 1024).await?;
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND {
            return Ok(CodexImageAuthStatus {
                state: "pending".into(),
                verification_url: Some(CODEX_DEVICE_PAGE.into()),
                user_code: Some(pending.user_code),
                poll_interval_seconds: Some(pending.poll_interval_seconds),
                message: None,
            });
        }
        if !status.is_success() {
            return Err(provider_failure(status.as_u16(), &bytes));
        }
        let authorization: CodexDeviceAuthorizationReply = serde_json::from_slice(&bytes)
            .map_err(|_| "OpenAI returned an invalid Codex authorization result")?;
        if authorization.authorization_code.is_empty()
            || authorization.authorization_code.len() > 8192
            || authorization.code_verifier.len() < 43
            || authorization.code_verifier.len() > 128
        {
            return Err("OpenAI returned an invalid Codex authorization result".into());
        }
        let token_response = self
            .send(
                self.client
                    .post(CODEX_TOKEN)
                    .form(&[
                        ("grant_type", "authorization_code"),
                        ("code", authorization.authorization_code.as_str()),
                        (
                            "redirect_uri",
                            "https://auth.openai.com/deviceauth/callback",
                        ),
                        ("client_id", protocol::CODEX_IMAGE_CLIENT_ID),
                        ("code_verifier", authorization.code_verifier.as_str()),
                    ])
                    .timeout(Duration::from_secs(30)),
                &cancellation,
            )
            .await?;
        let token_status = token_response.status();
        let token_bytes = read_body(token_response, &cancellation, 1024 * 1024).await?;
        if !token_status.is_success() {
            return Err(provider_failure(token_status.as_u16(), &token_bytes));
        }
        let token: CodexTokenReply = serde_json::from_slice(&token_bytes)
            .map_err(|_| "OpenAI returned an invalid Codex token response")?;
        let id_token = token
            .id_token
            .as_deref()
            .ok_or("Codex sign-in returned no verified account identity")?;
        let keys: JwkSet = self
            .json(self.client.get(protocol::JWKS), &cancellation)
            .await?;
        let account = self
            .storage
            .membership_account(id)
            .map_err(|e| e.to_string())?;
        let identity =
            protocol::verify_codex_image_identity(id_token, &account.identity.host_id, &keys)?;
        if identity.issuer != account.identity.issuer {
            return Err("The Codex authorization came from an unexpected identity issuer".into());
        }
        if !token.token_type.eq_ignore_ascii_case("Bearer")
            || token.access_token.trim().is_empty()
            || token.access_token.len() > 128 * 1024
        {
            return Err("Codex sign-in returned an invalid access credential".into());
        }
        let refresh_token = token
            .refresh_token
            .filter(|value| !value.trim().is_empty() && value.len() <= 128 * 1024)
            .ok_or("Codex sign-in returned no renewable session")?;
        let (record, _version) = self
            .storage
            .membership_protected_credentials(id)
            .map_err(|e| e.to_string())?;
        let mut saved = open_credentials(&record)?;
        let codex_account_id = protocol::chatgpt_account_id_from_oauth_token(&token.access_token)?;
        let identity = protocol::verify_codex_image_link(
            id_token,
            &record.identity,
            &saved.access_token,
            &token.access_token,
            &keys,
        )?;
        let credentials = CodexImageCredentials {
            linked_primary_subject: record.identity.subject.clone(),
            linked_primary_client_id: record.identity.client_id.clone(),
            verified_email: identity
                .email
                .clone()
                .ok_or("Image sign-in did not return an email")?,
            issuer: identity.issuer,
            subject: identity.subject,
            client_id: protocol::CODEX_IMAGE_CLIENT_ID.into(),
            chatgpt_account_id: codex_account_id,
            access_token: token.access_token,
            refresh_token,
            token_type: "Bearer".into(),
            access_expires_at_ms: protocol::codex_image_expiry_ms(token.expires_in)?,
        };
        let _lock = self.account_lock(Some(id), &cancellation).await?;
        let (record, version) = self
            .storage
            .membership_protected_credentials(id)
            .map_err(|e| e.to_string())?;
        saved = open_credentials(&record)?;
        if saved.subject != account.identity.subject
            || record.identity.subject != account.identity.subject
            || record.identity.client_id != account.identity.client_id
            || record.identity.host_id != account.identity.host_id
        {
            return Err("This Codex authorization no longer matches the selected account".into());
        }
        let current_primary = protocol::chatgpt_account_id_from_oauth_token(&saved.access_token)?;
        let current_codex =
            protocol::chatgpt_account_id_from_oauth_token(&credentials.access_token)?;
        let current_match = match (current_primary.as_deref(), current_codex.as_deref()) {
            (Some(primary), Some(codex)) => primary == codex,
            _ => {
                credentials.linked_primary_subject == record.identity.subject
                    && credentials.linked_primary_client_id == record.identity.client_id
                    && record.identity.email.as_deref() == Some(credentials.verified_email.as_str())
            }
        };
        if !current_match {
            return Err("The Codex authorization no longer matches this ChatGPT account".into());
        }
        saved.codex_images = Some(credentials);
        self.storage
            .membership_replace_credentials(
                id,
                version,
                &protect_credentials(&record.identity, &saved)?,
            )
            .map_err(|e| e.to_string())?;
        self.cancel_codex_image_authorization(id)?;
        Ok(CodexImageAuthStatus {
            state: "connected".into(),
            verification_url: None,
            user_code: None,
            poll_interval_seconds: None,
            message: Some("ChatGPT image tools are connected to this saved account.".into()),
        })
    }

    pub fn codex_image_authorization_status(
        &self,
        id: &str,
    ) -> Result<CodexImageAuthStatus, String> {
        let pending = self
            .codex_image_pending
            .lock()
            .map_err(|_| "Codex image authorization state is unavailable")?
            .get(id)
            .cloned();
        if let Some(pending) = pending {
            if (protocol::unix_seconds()? as i64).saturating_mul(1000) < pending.expires_at_ms {
                return Ok(CodexImageAuthStatus {
                    state: "pending".into(),
                    verification_url: Some(CODEX_DEVICE_PAGE.into()),
                    user_code: Some(pending.user_code),
                    poll_interval_seconds: Some(pending.poll_interval_seconds),
                    message: None,
                });
            }
            self.cancel_codex_image_authorization(id)?;
        }
        let (record, _) = self
            .storage
            .membership_protected_credentials(id)
            .map_err(|e| e.to_string())?;
        let credentials = open_credentials(&record)?;
        Ok(CodexImageAuthStatus {
            state: if credentials.codex_images.is_some() {
                "connected"
            } else {
                "not_connected"
            }
            .into(),
            verification_url: None,
            user_code: None,
            poll_interval_seconds: None,
            message: None,
        })
    }

    pub(crate) fn has_codex_image_credentials(&self, id: &str) -> Result<bool, String> {
        let (record, _) = self
            .storage
            .membership_protected_credentials(id)
            .map_err(|e| e.to_string())?;
        Ok(open_credentials(&record)?.codex_images.is_some())
    }

    pub fn cancel_codex_image_authorization(&self, id: &str) -> Result<bool, String> {
        Ok(self
            .codex_image_pending
            .lock()
            .map_err(|_| "Codex image authorization state is unavailable")?
            .remove(id)
            .is_some())
    }

    pub async fn codex_image_credentials(
        self: &Arc<Self>,
        id: &str,
        cancellation: &CancellationToken,
    ) -> Result<CodexImageCredentials, String> {
        let _lock = self.account_lock(Some(id), cancellation).await?;
        let (record, version) = self
            .storage
            .membership_protected_credentials(id)
            .map_err(|e| e.to_string())?;
        let mut saved = open_credentials(&record)?;
        let mut codex = saved
            .codex_images
            .clone()
            .ok_or("Authorize Codex image tools for this account in Settings")?;
        let now = (protocol::unix_seconds()? as i64).saturating_mul(1000);
        if codex.access_expires_at_ms > now + 30_000 {
            return Ok(codex);
        }
        let reply: CodexTokenReply = self
            .json(
                self.client.post(CODEX_TOKEN).form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", protocol::CODEX_IMAGE_CLIENT_ID),
                    ("refresh_token", codex.refresh_token.as_str()),
                ]),
                cancellation,
            )
            .await?;
        if !reply.token_type.eq_ignore_ascii_case("Bearer")
            || reply.access_token.trim().is_empty()
            || reply.access_token.len() > 128 * 1024
        {
            return Err("Codex image token refresh returned an invalid access credential".into());
        }
        super::validate_codex_image_routing(&codex, &reply.access_token)?;
        if let Some(id_token) = reply.id_token.as_deref() {
            let keys: JwkSet = self
                .json(self.client.get(protocol::JWKS), cancellation)
                .await?;
            let refreshed =
                protocol::verify_codex_image_identity(id_token, &record.identity.host_id, &keys)?;
            if refreshed.issuer != codex.issuer || refreshed.subject != codex.subject {
                return Err("Refreshed Codex credentials belong to a different account".into());
            }
        }
        codex.access_token = reply.access_token;
        if let Some(refresh) = reply.refresh_token.filter(|v| !v.trim().is_empty()) {
            codex.refresh_token = refresh
        }
        codex.access_expires_at_ms = protocol::codex_image_expiry_ms(reply.expires_in)?;
        saved.codex_images = Some(codex.clone());
        self.storage
            .membership_replace_credentials(
                id,
                version,
                &protect_credentials(&record.identity, &saved)?,
            )
            .map_err(|e| e.to_string())?;
        Ok(codex)
    }

    pub fn cancel_sign_in(&self, id: &str) -> Result<bool, String> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| "Sign-in state is unavailable")?;
        if let Some(request) = pending.get(id) {
            if !request.complete {
                request.cancellation.cancel();
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn sign_in(
        &self,
        request: SignInRequest,
        open_browser: impl Fn(String) -> Result<(), String>,
    ) -> Result<MembershipAccount, String> {
        Uuid::parse_str(&request.request_id)
            .map_err(|_| "Sign-in request identifier is invalid")?;
        if request.label.trim().is_empty()
            || request.label.len() > 160
            || request.label.contains('\0')
        {
            return Err("Enter an account label of at most 160 characters".into());
        }
        let cancellation = CancellationToken::new();
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| "Sign-in state is unavailable")?;
            if pending.contains_key(&request.request_id) {
                return Err("This sign-in request is already running".into());
            }
            pending.insert(
                request.request_id.clone(),
                PendingRequest {
                    cancellation: cancellation.clone(),
                    complete: false,
                    account_id: request.account_id.clone(),
                },
            );
        }
        let guard = SignInGuard {
            service: self,
            id: request.request_id.clone(),
        };
        let operation = async {
            let _lock = self
                .account_lock(request.account_id.as_deref(), &cancellation)
                .await?;
            let expected = request
                .account_id
                .as_deref()
                .map(|id| self.storage.membership_account(id))
                .transpose()
                .map_err(|e| e.to_string())?;
            if self.accounts()?.iter().any(|account| {
                account.identity.provider == "chatgpt"
                    && account.label == request.label.trim()
                    && Some(account.id.as_str()) != request.account_id.as_deref()
            }) {
                return Err("Choose a distinct label for this account".into());
            }
            let host_id = self
                .storage
                .membership_host_id()
                .map_err(|e| e.to_string())?;
            let hint = expected
                .as_ref()
                .filter(|a| a.has_credentials)
                .map(|account| {
                    let (record, _) = self
                        .storage
                        .membership_protected_credentials(&account.id)
                        .map_err(|e| e.to_string())?;
                    open_credentials(&record).map(|credentials| credentials.id_token)
                })
                .transpose()?;
            let discovery = self.discovery(&cancellation).await?;
            // Let the OS assign the callback port. A bind/listen failure is
            // surfaced; no search for a port that hides a broken host transport.
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .map_err(|_| "Could not open the local sign-in listener")?;
            let port = listener
                .local_addr()
                .map_err(|_| "Could not read the sign-in listener address")?
                .port();
            let mut attempt =
                PendingSignIn::new(port, host_id, expected.as_ref().map(|a| a.identity.clone()))?;
            let mut retry = false;
            loop {
                let authorization_url = attempt.authorization_url(hint.as_deref())?;
                open_browser(authorization_url.into())?;
                let callback =
                    read_callback(&listener, &attempt.redirect_uri, &cancellation).await?;
                let code = attempt.consume_callback(&callback)?;
                let response = self
                    .send(
                        self.client
                            .post(&discovery.token_endpoint)
                            .timeout(Duration::from_secs(30))
                            .form(&attempt.exchange_form(&code)),
                        &cancellation,
                    )
                    .await?;
                let status = response.status();
                let body = read_body(response, &cancellation, 1024 * 1024).await?;
                if !status.is_success() {
                    if !retry && safe_error_code(&body).as_deref() == Some("invalid_grant") {
                        attempt = attempt.retry_after_invalid_grant(&code)?;
                        retry = true;
                        continue;
                    }
                    return Err(provider_failure(status.as_u16(), &body));
                }
                let reply: TokenReply = serde_json::from_slice(&body)
                    .map_err(|_| "ChatGPT returned an invalid token response")?;
                let keys = self
                    .json::<JwkSet>(self.client.get(&discovery.jwks_uri), &cancellation)
                    .await?;
                let (identity, credentials) = attempt.validate_reply(&code, reply, &keys)?;
                let protected = protect_credentials(&identity, &credentials)?;
                // Cancellation and publication have one local ordering boundary.
                let mut pending = self
                    .pending
                    .lock()
                    .map_err(|_| "Sign-in state is unavailable")?;
                let state = pending
                    .get_mut(&guard.id)
                    .ok_or("Sign-in no longer exists")?;
                if state.cancellation.is_cancelled() {
                    return Err("Sign-in cancelled".into());
                }
                let account = match &expected {
                    Some(account) => self.storage.membership_reconnect(
                        &account.id,
                        account.credential_version,
                        &request.label,
                        &protected,
                    ),
                    None => self
                        .storage
                        .membership_add_account(request.label.trim(), &protected),
                }
                .map_err(|e| e.to_string())?;
                state.complete = true;
                if let Some(previous) = self
                    .sessions
                    .lock()
                    .map_err(|_| "Account request state is unavailable")?
                    .insert(
                        account.id.clone(),
                        AccountSession {
                            version: account.session_version,
                            cancellation: CancellationToken::new(),
                        },
                    )
                {
                    previous.cancellation.cancel();
                }
                return Ok(account);
            }
        };
        tokio::select! {
            _ = cancellation.cancelled() => Err("Sign-in cancelled".into()),
            result = tokio::time::timeout(AUTH_TIMEOUT, operation) => result.map_err(|_| "Sign-in expired; start again".to_string())?,
        }
    }

    pub async fn sign_out(self: &Arc<Self>, id: &str) -> Result<SignOutResult, String> {
        self.cancel_codex_image_authorization(id)?;
        let service = Arc::clone(self);
        let id = id.to_owned();
        tokio::spawn(async move { service.finish_sign_out(&id).await })
            .await
            .map_err(|_| "Account sign-out did not complete".to_string())?
    }

    async fn finish_sign_out(&self, id: &str) -> Result<SignOutResult, String> {
        Uuid::parse_str(id).map_err(|_| "Account identifier is invalid")?;
        {
            let pending = self
                .pending
                .lock()
                .map_err(|_| "Sign-in state is unavailable")?;
            for request in pending
                .values()
                .filter(|p| p.account_id.as_deref() == Some(id) && !p.complete)
            {
                request.cancellation.cancel();
            }
        }
        if let Some(token) = self
            .sessions
            .lock()
            .map_err(|_| "Account request state is unavailable")?
            .get(id)
        {
            token.cancellation.cancel();
        }
        let cancellation = CancellationToken::new();
        // Cleanup owns completion, not the ordinary request acquisition budget.
        // In-flight account operations retain their own bounded network/auth
        // lifetimes. Revoke and clear the latest credentials after they finish.
        let _lock = self
            .acquire_account_lock(Some(id), &cancellation, None)
            .await?;
        let account = self
            .storage
            .membership_account(id)
            .map_err(|e| e.to_string())?;
        if !account.has_credentials {
            return Ok(SignOutResult {
                account,
                remote_revoked: false,
                detail: "Already signed out locally; no renewable session was available to revoke."
                    .into(),
            });
        }
        let (remote_revoked, has_codex_grant) = match self
            .storage
            .membership_protected_credentials(id)
            .map_err(|e| e.to_string())
            .and_then(|(record, _)| open_credentials(&record))
        {
            Ok(credentials) => {
                let has_codex = credentials.codex_images.is_some();
                let primary = self.revoke(&credentials, &cancellation).await.is_ok();
                let codex = match credentials.codex_images.as_ref() {
                    Some(value) => self.revoke_codex_image(value, &cancellation).await.is_ok(),
                    None => true,
                };
                (primary && codex, has_codex)
            }
            Err(_) => (false, false),
        };
        let account = self
            .storage
            .membership_clear_credentials(id, account.credential_version)
            .map_err(|e| e.to_string())?;
        Ok(SignOutResult {account,remote_revoked,detail: if remote_revoked {"Signed out and ended this account's renewable session."} else if has_codex_grant {"Signed out locally. OpenAI did not confirm revocation of every saved membership session; disconnect CrowClaw in ChatGPT Settings if you want to revoke them remotely."} else {"Signed out locally. Remote revocation could not be confirmed; you can disconnect CrowClaw in ChatGPT Settings."}.into()})
    }

    async fn revoke_codex_image(
        &self,
        credentials: &CodexImageCredentials,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        let discovery = self.discovery(cancellation).await?;
        self.revoke_codex_image_at(credentials, cancellation, &discovery.revocation_endpoint)
            .await
    }

    async fn revoke_codex_image_at(
        &self,
        credentials: &CodexImageCredentials,
        cancellation: &CancellationToken,
        endpoint: &str,
    ) -> Result<(), String> {
        let response = self
            .send(
                self.client
                    .post(endpoint)
                    .timeout(Duration::from_secs(15))
                    .form(&[
                        ("token", credentials.refresh_token.as_str()),
                        ("token_type_hint", "refresh_token"),
                        ("client_id", protocol::CODEX_IMAGE_CLIENT_ID),
                    ]),
                cancellation,
            )
            .await?;
        confirm_revocation(response, cancellation).await.map(|_| ())
    }

    async fn revoke(
        &self,
        credentials: &MembershipCredentials,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        let discovery = self.discovery(cancellation).await?;
        self.revoke_at(credentials, cancellation, &discovery.revocation_endpoint)
            .await
    }

    async fn revoke_at(
        &self,
        credentials: &MembershipCredentials,
        cancellation: &CancellationToken,
        endpoint: &str,
    ) -> Result<(), String> {
        let mut last_error = "Remote revocation was not confirmed".to_string();
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::select! {
                    _ = cancellation.cancelled() => return Err("Account request cancelled".into()),
                    _ = tokio::time::sleep(Duration::from_millis(250 * (1 << attempt))) => {}
                }
            }
            match self
                .send(
                    self.client
                        .post(endpoint)
                        .timeout(Duration::from_secs(15))
                        .form(&[
                            ("token", credentials.refresh_token.as_str()),
                            ("token_type_hint", "refresh_token"),
                            ("client_id", credentials.client_id.as_str()),
                        ]),
                    cancellation,
                )
                .await
            {
                Ok(response) if response.status() == reqwest::StatusCode::OK => {
                    match confirm_revocation(response, cancellation).await {
                        Ok(true) => return Ok(()),
                        Ok(false) => return Err("Remote revocation was not confirmed".into()),
                        Err(error) => last_error = error,
                    }
                }
                Ok(response) => {
                    let status = response.status();
                    let body = match read_body(response, cancellation, 1024 * 1024).await {
                        Ok(body) => body,
                        Err(error) => {
                            last_error = error;
                            continue;
                        }
                    };
                    last_error = provider_failure(status.as_u16(), &body);
                    if !status.is_server_error() {
                        break;
                    }
                }
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    pub(crate) async fn credentials(
        self: &Arc<Self>,
        id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(MembershipCredentials, u32), String> {
        if cancellation.is_cancelled() {
            return Err("Account request cancelled".into());
        }
        let service = Arc::clone(self);
        let id = id.to_owned();
        let cancellation = cancellation.clone();
        // Dropping the requesting task detaches this owner; it must not discard
        // a replacement token after the issuer has consumed the previous one.
        tokio::spawn(async move {
            let _lock = service.account_lock(Some(&id), &cancellation).await?;
            service.credentials_locked(&id, &cancellation).await
        })
        .await
        .map_err(|_| "Account credential maintenance did not complete".to_string())?
    }

    // The caller holds the account's cross-process lock through publication.
    async fn credentials_locked(
        &self,
        id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(MembershipCredentials, u32), String> {
        let (record, version) = self
            .storage
            .membership_protected_credentials(id)
            .map_err(|e| e.to_string())?;
        let saved = open_credentials(&record)?;
        if saved.access_expires_at_ms > (protocol::unix_seconds()? as i64) * 1000 + 30_000 {
            return Ok((saved, version));
        }
        let discovery = self.discovery(cancellation).await?;
        if cancellation.is_cancelled() {
            return Err("Account request cancelled".into());
        }
        // Once renewal starts, task/session cancellation cannot undo the remote
        // token rotation. Finish validation and persistence while holding the
        // account lock; sign-out then revokes the newly saved token.
        let renewal = CancellationToken::new();
        let reply: TokenReply = self
            .json(
                self.client.post(&discovery.token_endpoint).form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", saved.client_id.as_str()),
                    ("refresh_token", saved.refresh_token.as_str()),
                    ("resource", protocol::RESOURCE),
                ]),
                &renewal,
            )
            .await?;
        if let Some(token) = reply.id_token.as_deref() {
            let keys: JwkSet = self
                .json(self.client.get(&discovery.jwks_uri), &renewal)
                .await?;
            protocol::verify_refreshed_identity(token, &record.identity, &keys)?;
        }
        let next = protocol::credentials_from_reply(&record.identity, reply, Some(&saved))?;
        let updated = self
            .storage
            .membership_replace_credentials(
                id,
                version,
                &protect_credentials(&record.identity, &next)?,
            )
            .map_err(|e| e.to_string())?;
        Ok((next, updated.credential_version))
    }

    pub async fn refresh_catalog(self: &Arc<Self>, id: &str) -> Result<MembershipAccount, String> {
        self.refresh_catalog_request(
            id,
            self.client.get(format!("{}/models", protocol::RESOURCE)),
        )
        .await
    }

    async fn refresh_catalog_request(
        self: &Arc<Self>,
        id: &str,
        request: RequestBuilder,
    ) -> Result<MembershipAccount, String> {
        let cancellation = self.session_cancellation(id)?;
        let service = Arc::clone(self);
        let id = id.to_owned();
        tokio::spawn(async move {
            service
                .refresh_catalog_locked(&id, request, &cancellation)
                .await
        })
        .await
        .map_err(|_| "Account catalog maintenance did not complete".to_string())?
    }

    async fn refresh_catalog_locked(
        &self,
        id: &str,
        request: RequestBuilder,
        cancellation: &CancellationToken,
    ) -> Result<MembershipAccount, String> {
        let _lock = self.account_lock(Some(id), &cancellation).await?;
        let (credentials, version) = self.credentials_locked(id, &cancellation).await?;
        if cancellation.is_cancelled() {
            return Err("Account request cancelled".into());
        }
        let raw: serde_json::Value = self
            .json(
                request.bearer_auth(&credentials.access_token),
                &cancellation,
            )
            .await?;
        let catalog = parse_catalog(id, raw)?;
        if cancellation.is_cancelled() {
            return Err("Account request cancelled".into());
        }
        self.storage
            .membership_save_catalog(id, version, &catalog)
            .map_err(|e| e.to_string())
    }

    pub(crate) fn session_cancellation(&self, id: &str) -> Result<CancellationToken, String> {
        Uuid::parse_str(id).map_err(|_| "Account identifier is invalid")?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "Account request state is unavailable")?;
        let account = self
            .storage
            .membership_account(id)
            .map_err(|e| e.to_string())?;
        let session = sessions.entry(id.into()).or_insert_with(|| AccountSession {
            version: account.session_version,
            cancellation: CancellationToken::new(),
        });
        if session.version != account.session_version {
            session.cancellation.cancel();
            *session = AccountSession {
                version: account.session_version,
                cancellation: CancellationToken::new(),
            };
        }
        Ok(session.cancellation.clone())
    }

    async fn account_lock(
        &self,
        id: Option<&str>,
        cancellation: &CancellationToken,
    ) -> Result<File, String> {
        self.acquire_account_lock(
            id,
            cancellation,
            Some(tokio::time::Instant::now() + Duration::from_secs(30)),
        )
        .await
    }

    async fn acquire_account_lock(
        &self,
        id: Option<&str>,
        cancellation: &CancellationToken,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<File, String> {
        if let Some(id) = id {
            Uuid::parse_str(id).map_err(|_| "Account identifier is invalid")?;
        }
        let path = self
            .storage
            .database_path()
            .parent()
            .ok_or("Account profile directory is unavailable")?
            .join(format!("membership-{}.lock", id.unwrap_or("registration")));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| "Could not open this account's session lock")?;
        loop {
            if cancellation.is_cancelled() {
                return Err("Account request cancelled".into());
            }
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(_)) => {
                    return Err("Could not lock this account's session".into())
                }
            }
            tokio::select! {
                _ = cancellation.cancelled() => return Err("Account request cancelled".into()),
                _ = async {
                    match deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                } => return Err("Another operation is using this account; try again after it finishes".into()),
                _ = tokio::time::sleep(Duration::from_millis(25)) => {},
            }
        }
    }

    async fn discovery(&self, cancellation: &CancellationToken) -> Result<Discovery, String> {
        #[cfg(test)]
        if let Some(document) = &self.test_discovery {
            return Ok(document.clone());
        }
        let document: Discovery = self
            .json(self.client.get(protocol::DISCOVERY), cancellation)
            .await?;
        document.validate()?;
        Ok(document)
    }
    pub(crate) async fn send(
        &self,
        request: RequestBuilder,
        cancellation: &CancellationToken,
    ) -> Result<Response, String> {
        tokio::select! {
            _ = cancellation.cancelled() => Err("Account request cancelled".into()),
            result = request.send() => result.map_err(|error| if error.is_timeout() {"The membership service timed out; try again".into()} else {"Could not reach the membership service; check your connection".into()}),
        }
    }
    async fn json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        cancellation: &CancellationToken,
    ) -> Result<T, String> {
        let response = self
            .send(request.timeout(Duration::from_secs(30)), cancellation)
            .await?;
        let status = response.status();
        let bytes = read_body(response, cancellation, 2 * 1024 * 1024).await?;
        if !status.is_success() {
            return Err(provider_failure(status.as_u16(), &bytes));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| "Membership service returned invalid data".into())
    }
}

async fn confirm_revocation(
    response: Response,
    cancellation: &CancellationToken,
) -> Result<bool, String> {
    let status = response.status();
    let body = read_body(response, cancellation, 1024 * 1024).await?;
    Ok(status == reqwest::StatusCode::OK && body.is_empty())
}

struct SignInGuard<'a> {
    service: &'a MembershipService,
    id: String,
}
impl Drop for SignInGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.service.pending.lock() {
            pending.remove(&self.id);
        }
    }
}

#[derive(Clone, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    revocation_endpoint: String,
    id_token_signing_alg_values_supported: Vec<String>,
}
impl Discovery {
    fn validate(&self) -> Result<(), String> {
        let revoke = Url::parse(&self.revocation_endpoint)
            .map_err(|_| "Invalid membership revocation endpoint")?;
        if self.issuer != protocol::ISSUER
            || self.authorization_endpoint != protocol::AUTHORIZE
            || self.token_endpoint != protocol::TOKEN
            || self.jwks_uri != protocol::JWKS
            || !self
                .id_token_signing_alg_values_supported
                .iter()
                .any(|alg| alg == "RS256")
            || revoke.origin() != Url::parse(protocol::ISSUER).unwrap().origin()
            || !revoke.username().is_empty()
            || revoke.password().is_some()
            || revoke.query().is_some()
            || revoke.fragment().is_some()
        {
            return Err(
                "Membership discovery does not match the documented OpenAI endpoints".into(),
            );
        }
        Ok(())
    }
}

pub(crate) async fn read_body(
    mut response: Response,
    cancellation: &CancellationToken,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|len| len > limit as u64)
    {
        return Err("Membership response exceeds its size limit".into());
    }
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! { _ = cancellation.cancelled() => return Err("Account request cancelled".into()), result = response.chunk() => result.map_err(|_| "Membership response was interrupted")? };
        match chunk {
            Some(chunk) => {
                if bytes.len().saturating_add(chunk.len()) > limit {
                    return Err("Membership response exceeds its size limit".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            None => return Ok(bytes),
        }
    }
}
fn safe_error_code(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    value
        .get("error")
        .and_then(|error| {
            error
                .as_str()
                .or_else(|| error.get("code").and_then(|code| code.as_str()))
        })
        .filter(|code| {
            code.len() <= 128
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
        .map(str::to_string)
}
pub(crate) fn provider_failure(status: u16, body: &[u8]) -> String {
    match safe_error_code(body).as_deref() {
        Some("subscription_sharing_usage_limit_exceeded") => "This ChatGPT account has reached its plan limit. Wait for its reset or choose another configured provider; no paid fallback was used.".into(),
        Some("subscription_sharing_usage_unavailable") => "ChatGPT plan usage is unavailable for this registration. Review its access in ChatGPT Settings.".into(),
        Some("invalid_grant" | "invalid_token") => "This account's session has expired or been revoked. Reconnect the saved account.".into(),
        _ if status == 401 || status == 403 => "This account is not authorized for that request. Reconnect or review its access in ChatGPT Settings.".into(),
        _ if status == 429 => "The membership service is limiting requests. Try again after its limit resets.".into(),
        _ if status == 0 => "ChatGPT reported a failed response. Review this account's access or try again.".into(),
        _ => format!("Membership service returned HTTP {status}; no alternative billing route was attempted"),
    }
}

fn parse_catalog(id: &str, raw: serde_json::Value) -> Result<MembershipCatalog, String> {
    let models = raw
        .get("models")
        .and_then(|models| models.as_array())
        .ok_or("Membership service did not return an account model catalog")?;
    if models.len() > 1024 {
        return Err("Account catalog exceeds its model bound".into());
    }
    let mut choices = Vec::new();
    for model in models
        .iter()
        .filter(|model| model.get("visibility").and_then(|v| v.as_str()) == Some("list"))
    {
        let slug = model
            .get("slug")
            .and_then(|v| v.as_str())
            .ok_or("Model catalog contains an invalid identifier")?;
        let display_name = model
            .get("display_name")
            .and_then(|v| v.as_str())
            .ok_or("Model catalog contains an invalid display name")?;
        // Optional published OpenAI catalog metadata. Use only values actually
        // returned for this account; absent metadata permits no explicit override.
        let reasoning_efforts = match model.get("supported_reasoning_levels") {
            None | Some(serde_json::Value::Null) => Vec::new(),
            Some(value) => value
                .as_array()
                .ok_or("Model effort metadata is invalid")?
                .iter()
                .map(|entry| {
                    entry
                        .get("effort")
                        .and_then(|effort| effort.as_str())
                        .map(str::to_string)
                        .ok_or("Model effort metadata is invalid")
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        choices.push(MembershipModel {
            slug: slug.into(),
            display_name: display_name.into(),
            reasoning_efforts,
        });
    }
    Ok(MembershipCatalog {
        account_id: id.into(),
        models: choices,
        fetched_at_ms: (protocol::unix_seconds()? as i64) * 1000,
    })
}

async fn read_callback(
    listener: &TcpListener,
    redirect: &str,
    cancellation: &CancellationToken,
) -> Result<String, String> {
    for _ in 0..16 {
        let (mut stream, peer) = tokio::select! { _ = cancellation.cancelled() => return Err("Sign-in cancelled".into()), result = listener.accept() => result.map_err(|_| "The local sign-in listener failed")? };
        if !peer.ip().is_loopback() {
            continue;
        }
        let request = tokio::select! { _ = cancellation.cancelled() => return Err("Sign-in cancelled".into()), result = tokio::time::timeout(Duration::from_secs(5),read_headers(&mut stream)) => result.map_err(|_| "The local sign-in response timed out")?? };
        let target = parse_callback_request(&request, redirect)?;
        if let Some(target) = target {
            let text = "Sign-in response received. Return to CrowClaw for the result.";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{text}",text.len());
            let _ = stream.write_all(response.as_bytes()).await;
            return Ok(target);
        }
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    }
    Err("The local sign-in listener received too many unrelated requests".into())
}
async fn read_headers(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let count = stream
            .read(&mut buffer)
            .await
            .map_err(|_| "Could not read the local sign-in response")?;
        if count == 0 {
            return Err("The local sign-in response ended early".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > 16 * 1024 {
            return Err("The local sign-in response exceeds its size limit".into());
        }
        if bytes.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            return Ok(bytes);
        }
    }
}
fn parse_callback_request(bytes: &[u8], redirect: &str) -> Result<Option<String>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "The local sign-in response is invalid")?;
    let mut lines = text.split("\r\n");
    let request = lines
        .next()
        .ok_or("Missing sign-in request")?
        .split_whitespace()
        .collect::<Vec<_>>();
    if request.len() != 3 || request[0] != "GET" || request[2] != "HTTP/1.1" {
        return Err("The local sign-in request is invalid".into());
    }
    if request[1].split('?').next() != Some("/auth/callback") {
        return Ok(None);
    }
    let redirect = Url::parse(redirect).map_err(|_| "Invalid sign-in callback configuration")?;
    let host = format!(
        "127.0.0.1:{}",
        redirect.port().ok_or("Invalid callback port")?
    );
    let hosts = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim())
        .collect::<Vec<_>>();
    if hosts != [host.as_str()] {
        return Err("The local sign-in host did not match this listener".into());
    }
    Ok(Some(format!("http://{host}{}", request[1])))
}

#[cfg(test)]
mod tests;
