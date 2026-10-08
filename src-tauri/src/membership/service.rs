use super::protocol::{self, PendingSignIn, TokenReply, AUTH_TIMEOUT};
use super::{
    open_credentials, protect_credentials, MembershipAccount, MembershipCatalog,
    MembershipCredentials, MembershipModel,
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
            #[cfg(test)]
            test_discovery: None,
        })
    }

    pub fn accounts(&self) -> Result<Vec<MembershipAccount>, String> {
        self.storage
            .membership_accounts()
            .map_err(|e| e.to_string())
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
        let remote_revoked = match self
            .storage
            .membership_protected_credentials(id)
            .map_err(|e| e.to_string())
            .and_then(|(record, _)| open_credentials(&record))
        {
            Ok(credentials) => self.revoke(&credentials, &cancellation).await.is_ok(),
            Err(_) => false,
        };
        let account = self
            .storage
            .membership_clear_credentials(id, account.credential_version)
            .map_err(|e| e.to_string())?;
        Ok(SignOutResult {account,remote_revoked,detail: if remote_revoked {"Signed out and ended this account's renewable session."} else {"Signed out locally. Remote revocation could not be confirmed; you can disconnect CrowClaw in ChatGPT Settings."}.into()})
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
