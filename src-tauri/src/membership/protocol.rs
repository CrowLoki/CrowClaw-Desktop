use super::{MembershipCredentials, MembershipIdentity};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub const ISSUER: &str = "https://auth.openai.com";
pub const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
pub const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
pub const RESOURCE: &str = "https://api.openai.com/v1";
pub const DISCOVERY: &str = "https://auth.openai.com/.well-known/openid-configuration";
pub const JWKS: &str = "https://auth.openai.com/.well-known/jwks.json";
pub const SCOPE: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
// The separate Codex OAuth grant is used only for the image endpoint that
// accepted Hermes' own account credential; the SIWC Responses token is a
// different audience and was observed to fail that endpoint with 401.
pub const CODEX_IMAGE_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(600);

// Deliberately no Debug/Serialize: this object holds the verifier and nonce.
pub struct PendingSignIn {
    pub(crate) state: String,
    pub(crate) nonce: String,
    verifier: String,
    pub(crate) redirect_uri: String,
    pub(crate) host_id: String,
    pub(crate) expected: Option<MembershipIdentity>,
    issued_client: Option<String>,
    created: Instant,
    consumed: bool,
}

pub struct AuthorizationCode {
    code: String,
    pub(crate) client_id: String,
}

#[derive(Deserialize)]
pub struct TokenReply {
    pub(crate) id_token: Option<String>,
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) token_type: String,
    pub(crate) scope: Option<String>,
    pub(crate) expires_in: u64,
}

#[derive(Clone, Deserialize)]
struct IdentityClaims {
    iss: String,
    sub: String,
    aud: serde_json::Value,
    exp: u64,
    iat: u64,
    nonce: Option<String>,
    email: Option<String>,
    azp: Option<String>,
}

impl PendingSignIn {
    pub fn new(
        port: u16,
        host_id: String,
        expected: Option<MembershipIdentity>,
    ) -> Result<Self, String> {
        if port == 0 {
            return Err("The sign-in listener has no assigned port".into());
        }
        let host = host_id
            .strip_prefix("urn:uuid:")
            .ok_or("This install has no valid host identity")?;
        if Uuid::parse_str(host)
            .map_err(|_| "This install has no valid host identity")?
            .get_version_num()
            != 4
        {
            return Err("This install has no valid host identity".into());
        }
        if let Some(identity) = &expected {
            super::validate_identity(identity)?;
            if identity.provider != "chatgpt" || identity.host_id != host_id {
                return Err("Saved account does not belong to this ChatGPT host".into());
            }
        }
        let issued_client = expected.as_ref().map(|identity| identity.client_id.clone());
        Ok(Self {
            state: random_secret(),
            nonce: random_secret(),
            verifier: random_secret(),
            redirect_uri: format!("http://127.0.0.1:{port}/auth/callback"),
            host_id,
            expected,
            issued_client,
            created: Instant::now(),
            consumed: false,
        })
    }

    pub fn authorization_url(&self, id_token_hint: Option<&str>) -> Result<Url, String> {
        if self.consumed || self.created.elapsed() >= AUTH_TIMEOUT {
            return Err("Start a new sign-in attempt".into());
        }
        let mut url = Url::parse(AUTHORIZE).map_err(|_| "Invalid authorization endpoint")?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair(
                "client_id",
                self.issued_client
                    .as_deref()
                    .unwrap_or("dynamic_agent_client"),
            );
            if self.issued_client.is_none() {
                query.append_pair("agent_name_hint", "CrowClaw");
            }
            query
                .append_pair("ext_agent_host_id", &self.host_id)
                .append_pair("response_type", "code")
                .append_pair("redirect_uri", &self.redirect_uri)
                .append_pair("scope", SCOPE)
                .append_pair("resource", RESOURCE)
                .append_pair("state", &self.state)
                .append_pair("nonce", &self.nonce)
                .append_pair("code_challenge_method", "S256")
                .append_pair("code_challenge", &pkce_challenge(&self.verifier));
            if let Some(expected) = &self.expected {
                if let Some(hint) = id_token_hint {
                    query.append_pair("id_token_hint", hint);
                }
                if let Some(email) = &expected.email {
                    query.append_pair("login_hint", email);
                }
            }
        }
        Ok(url)
    }

    /// Any matching-path callback consumes this one-time attempt, including a
    /// mismatch or denial. Unrelated HTTP paths are filtered by the listener.
    pub fn consume_callback(&mut self, callback: &str) -> Result<AuthorizationCode, String> {
        if self.consumed {
            return Err("This sign-in response was already used".into());
        }
        self.consumed = true;
        if self.created.elapsed() >= AUTH_TIMEOUT {
            return Err("Sign-in expired; start again".into());
        }
        if callback.len() > 16 * 1024 {
            return Err("Sign-in response exceeds its bounds".into());
        }
        let url = Url::parse(callback).map_err(|_| "Sign-in callback is invalid")?;
        let mut base = url.clone();
        base.set_query(None);
        base.set_fragment(None);
        if base.as_str() != self.redirect_uri || url.fragment().is_some() {
            return Err("Sign-in returned to the wrong callback".into());
        }
        let mut params = HashMap::new();
        for (key, value) in url.query_pairs() {
            if params
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                return Err("Sign-in callback contains duplicate fields".into());
            }
        }
        if params.get("state") != Some(&self.state) {
            return Err("Sign-in state did not match this attempt".into());
        }
        if params.contains_key("error") {
            return Err(
                "ChatGPT sign-in was not approved. Try again and allow this app to use your plan."
                    .into(),
            );
        }
        let code = params
            .remove("code")
            .filter(|code| !code.trim().is_empty() && code.len() <= 8192)
            .ok_or("Sign-in did not return an authorization code")?;
        let client_id = match &self.issued_client {
            Some(expected) => {
                if params.get("client_id").is_some_and(|id| id != expected) {
                    return Err("Sign-in returned a different account registration".into());
                }
                expected.clone()
            }
            None => params
                .remove("client_id")
                .ok_or("Registration did not return an issued client identifier")?,
        };
        if client_id.trim().is_empty()
            || client_id.len() > 1024
            || client_id.contains('\0')
            || client_id == "dynamic_agent_client"
        {
            return Err("Registration did not return a valid issued client identifier".into());
        }
        Ok(AuthorizationCode { code, client_id })
    }

    pub fn exchange_form(&self, code: &AuthorizationCode) -> Vec<(&'static str, String)> {
        vec![
            ("grant_type", "authorization_code".into()),
            ("client_id", code.client_id.clone()),
            ("code", code.code.clone()),
            ("code_verifier", self.verifier.clone()),
            ("redirect_uri", self.redirect_uri.clone()),
            ("resource", RESOURCE.into()),
        ]
    }

    /// A spent/expired code requires a fresh flow, retaining the issued client
    /// only inside this attempt until a signed identity is successfully verified.
    pub fn retry_after_invalid_grant(&self, code: &AuthorizationCode) -> Result<Self, String> {
        let port = Url::parse(&self.redirect_uri)
            .map_err(|_| "Invalid callback")?
            .port()
            .ok_or("Invalid callback port")?;
        let mut retry = Self::new(port, self.host_id.clone(), self.expected.clone())?;
        retry.issued_client = Some(code.client_id.clone());
        Ok(retry)
    }

    pub fn validate_reply(
        &self,
        code: &AuthorizationCode,
        reply: TokenReply,
        keys: &JwkSet,
    ) -> Result<(MembershipIdentity, MembershipCredentials), String> {
        if !self.consumed || self.created.elapsed() >= AUTH_TIMEOUT {
            return Err("Sign-in attempt is no longer active".into());
        }
        let id_token = reply
            .id_token
            .as_deref()
            .ok_or("Sign-in did not return an identity token")?;
        let identity =
            verify_identity(id_token, &code.client_id, &self.nonce, &self.host_id, keys)?;
        if let Some(expected) = &self.expected {
            if !crate::storage::membership_types::same_registration(expected, &identity) {
                return Err("Sign-in belongs to a different saved account or workspace".into());
            }
        }
        let credentials = credentials_from_reply(&identity, reply, None)?;
        Ok((identity, credentials))
    }
}

fn random_secret() -> String {
    // Three independently generated UUIDv4 values provide 366 random bits and a
    // 96-character PKCE verifier within RFC 7636's 43–128-character bounds.
    format!(
        "{}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn signing_key_id(token: &str) -> Result<String, String> {
    if token.len() > 128 * 1024 {
        return Err("Identity token exceeds its size bound".into());
    }
    let header = decode_header(token).map_err(|_| "Identity token header is invalid")?;
    if header.alg != Algorithm::RS256 {
        return Err("Identity token uses an unsupported signing algorithm".into());
    }
    header
        .kid
        .filter(|kid| !kid.is_empty() && kid.len() <= 256)
        .ok_or_else(|| "Identity token has no valid signing key identifier".into())
}

pub fn verify_identity(
    token: &str,
    client_id: &str,
    nonce: &str,
    host_id: &str,
    keys: &JwkSet,
) -> Result<MembershipIdentity, String> {
    verify_signed_identity(token, client_id, Some(nonce), host_id, keys)
}

pub fn verify_refreshed_identity(
    token: &str,
    expected: &MembershipIdentity,
    keys: &JwkSet,
) -> Result<(), String> {
    let identity =
        verify_signed_identity(token, &expected.client_id, None, &expected.host_id, keys)?;
    if !crate::storage::membership_types::same_registration(expected, &identity) {
        return Err("Refreshed identity belongs to a different registration".into());
    }
    Ok(())
}

pub fn verify_codex_image_identity(
    token: &str,
    host_id: &str,
    keys: &JwkSet,
) -> Result<MembershipIdentity, String> {
    verify_signed_identity(token, CODEX_IMAGE_CLIENT_ID, None, host_id, keys)
}

/// Link an explicitly requested secondary grant to the selected registration.
/// OIDC subjects belong to their client namespace; do not compare them across
/// different clients when the primary token omits an account-routing claim.
pub fn verify_codex_image_link(
    token: &str,
    primary: &MembershipIdentity,
    primary_access: &str,
    codex_access: &str,
    keys: &JwkSet,
) -> Result<MembershipIdentity, String> {
    let identity = verify_codex_image_identity(token, &primary.host_id, keys)?;
    let claims: serde_json::Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(
                token
                    .split('.')
                    .nth(1)
                    .ok_or("Identity token is malformed")?,
            )
            .map_err(|_| "Identity token is malformed")?,
    )
    .map_err(|_| "Identity token is malformed")?;
    if identity.issuer != primary.issuer
        || claims["email_verified"] != true
        || primary
            .email
            .as_deref()
            .is_none_or(|email| email.is_empty())
        || identity.email != primary.email
    {
        return Err(
            "The image sign-in must verify the email of the selected ChatGPT registration".into(),
        );
    }
    if let (Some(primary_id), Some(codex_id)) = (
        chatgpt_account_id_from_oauth_token(primary_access)?,
        chatgpt_account_id_from_oauth_token(codex_access)?,
    ) {
        if primary_id != codex_id {
            return Err("The image sign-in returned a different ChatGPT workspace".into());
        }
    }
    Ok(identity)
}

/// Read account-routing metadata from an OAuth token received over the pinned
/// OpenAI token endpoint. This parses a claim; it does not validate a signature.
pub fn chatgpt_account_id_from_oauth_token(token: &str) -> Result<Option<String>, String> {
    if token.len() > 128 * 1024 {
        return Err("Identity token exceeds its size bound".into());
    }
    let payload = token
        .split('.')
        .nth(1)
        .ok_or("Identity token is malformed")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "Identity token is malformed")?;
    let claims: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "Identity token is malformed")?;
    let id = claims["https://api.openai.com/auth"]["chatgpt_account_id"].as_str();
    Ok(id
        .filter(|value| {
            !value.trim().is_empty()
                && value.len() <= 256
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        })
        .map(str::to_owned))
}

pub fn codex_image_expiry_ms(expires_in: u64) -> Result<i64, String> {
    let ttl = i64::try_from(expires_in)
        .ok()
        .filter(|value| (60..=31_536_000).contains(value))
        .ok_or("Codex image token expiry is invalid")?;
    let now = i64::try_from(unix_seconds()?)
        .ok()
        .and_then(|value| value.checked_mul(1000))
        .ok_or("System time exceeds token expiry bounds")?;
    now.checked_add(ttl.saturating_mul(1000))
        .ok_or_else(|| "Codex image token expiry exceeds its bounds".into())
}

fn verify_signed_identity(
    token: &str,
    client_id: &str,
    nonce: Option<&str>,
    host_id: &str,
    keys: &JwkSet,
) -> Result<MembershipIdentity, String> {
    let kid = signing_key_id(token)?;
    let matching = keys
        .keys
        .iter()
        .filter(|key| key.common.key_id.as_deref() == Some(&kid))
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err("Identity signing key is missing or ambiguous".into());
    }
    let jwk = matching[0];
    let raw_key = serde_json::to_value(jwk).map_err(|_| "Invalid identity signing key")?;
    if raw_key["kty"] != "RSA"
        || raw_key.get("alg").is_some_and(|alg| alg != "RS256")
        || raw_key.get("use").is_some_and(|usage| usage != "sig")
        || raw_key.get("key_ops").is_some_and(|ops| {
            !ops.as_array()
                .is_some_and(|ops| ops.iter().any(|op| op == "verify"))
        })
    {
        return Err("Identity key is not authorized for RS256 signature verification".into());
    }
    let key = DecodingKey::from_jwk(jwk).map_err(|_| "Invalid identity signing key")?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[ISSUER]);
    validation.set_audience(&[client_id]);
    validation.set_required_spec_claims(&["iss", "aud", "sub", "exp", "iat"]);
    validation.validate_nbf = true;
    validation.leeway = 5;
    let claims = decode::<IdentityClaims>(token, &key, &validation)
        .map_err(|_| "Identity token failed signature, issuer, audience or expiry validation")?
        .claims;
    let now = unix_seconds()?;
    if nonce.is_some_and(|nonce| claims.nonce.as_deref() != Some(nonce))
        || claims.iat > now.saturating_add(5)
        || claims.exp <= claims.iat
    {
        return Err("Identity nonce or validity period did not match this sign-in".into());
    }
    if claims
        .azp
        .as_deref()
        .is_some_and(|party| party != client_id)
        || (claims.aud.as_array().is_some_and(|aud| aud.len() > 1)
            && claims.azp.as_deref() != Some(client_id))
    {
        return Err("Identity token is authorized for a different client".into());
    }
    let identity = MembershipIdentity {
        provider: "chatgpt".into(),
        issuer: claims.iss,
        subject: claims.sub,
        client_id: client_id.into(),
        host_id: host_id.into(),
        email: claims.email,
    };
    super::validate_identity(&identity)?;
    Ok(identity)
}

pub fn credentials_from_reply(
    identity: &MembershipIdentity,
    reply: TokenReply,
    previous: Option<&MembershipCredentials>,
) -> Result<MembershipCredentials, String> {
    let scopes = match reply.scope {
        Some(scope) => scope.split_whitespace().map(str::to_string).collect(),
        None => previous
            .map(|p| p.scopes.clone())
            .ok_or("Token response did not report granted scopes")?,
    };
    let expires_ms = i64::try_from(reply.expires_in)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1000))
        .filter(|ms| *ms > 0)
        .ok_or("Token response has an invalid expiry")?;
    let now_ms = i64::try_from(unix_seconds()?)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1000))
        .ok_or("System time exceeds token expiry bounds")?;
    let credentials = MembershipCredentials {
        issuer: identity.issuer.clone(),
        subject: identity.subject.clone(),
        client_id: identity.client_id.clone(),
        ext_agent_host_id: identity.host_id.clone(),
        id_token: reply
            .id_token
            .or_else(|| previous.map(|p| p.id_token.clone()))
            .ok_or("Token response has no retained identity token")?,
        access_token: reply.access_token,
        refresh_token: reply
            .refresh_token
            .or_else(|| previous.map(|p| p.refresh_token.clone()))
            .ok_or("Token response has no renewable session")?,
        token_type: reply.token_type,
        scopes,
        access_expires_at_ms: now_ms
            .checked_add(expires_ms)
            .ok_or("Token expiry exceeds its bounds")?,
        codex_images: previous.and_then(|saved| saved.codex_images.clone()),
    };
    if !credentials.token_type.eq_ignore_ascii_case("Bearer")
        || credentials.access_token.trim().is_empty()
        || credentials.refresh_token.trim().is_empty()
        || credentials.id_token.trim().is_empty()
        || !["openid", "resource.invoke", "chatgpt.tokens.use.direct"]
            .iter()
            .all(|scope| credentials.scopes.iter().any(|granted| granted == scope))
    {
        return Err("This token response does not authorize ChatGPT membership use".into());
    }
    Ok(MembershipCredentials {
        token_type: "Bearer".into(),
        ..credentials
    })
}

pub fn unix_seconds() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| "System clock is before the Unix epoch".into())
}

#[cfg(test)]
mod tests;
