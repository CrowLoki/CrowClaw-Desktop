pub(crate) mod images;
pub(crate) mod protection;
pub(crate) mod protocol;
pub(crate) mod responses;
pub(crate) mod service;
use crate::storage::membership_types::validate_identity;
pub use crate::storage::membership_types::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MembershipCredentials {
    pub issuer: String,
    pub subject: String,
    pub client_id: String,
    pub ext_agent_host_id: String,
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub scopes: Vec<String>,
    pub access_expires_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_images: Option<CodexImageCredentials>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexImageCredentials {
    pub linked_primary_subject: String,
    pub linked_primary_client_id: String,
    pub verified_email: String,
    pub issuer: String,
    pub subject: String,
    pub client_id: String,
    #[serde(default)]
    pub chatgpt_account_id: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub access_expires_at_ms: i64,
}
impl std::fmt::Debug for CodexImageCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexImageCredentials")
            .field("tokens", &"[redacted]")
            .finish()
    }
}

fn validate_codex_image_credentials(
    identity: &MembershipIdentity,
    primary_access_token: &str,
    credentials: &CodexImageCredentials,
) -> Result<(), String> {
    validate_codex_image_routing(credentials, &credentials.access_token)?;
    let valid = |token: &str| {
        !token.trim().is_empty()
            && token.len() <= 128 * 1024
            && !token.chars().any(char::is_control)
    };
    let primary_account_id = protocol::chatgpt_account_id_from_oauth_token(primary_access_token)?;
    let same_account = match (
        primary_account_id.as_deref(),
        credentials.chatgpt_account_id.as_deref(),
    ) {
        (Some(primary), Some(codex)) => primary == codex,
        _ => true,
    };
    if credentials.issuer != identity.issuer
        || !same_account
        || credentials.linked_primary_subject != identity.subject
        || credentials.linked_primary_client_id != identity.client_id
        || identity.email.as_deref() != Some(credentials.verified_email.as_str())
        || credentials.subject.trim().is_empty()
        || credentials.client_id != protocol::CODEX_IMAGE_CLIENT_ID
        || credentials.token_type != "Bearer"
        || !valid(&credentials.access_token)
        || !valid(&credentials.refresh_token)
        || credentials.access_expires_at_ms <= 0
    {
        return Err(
            "The Codex image authorization does not match this saved ChatGPT account".into(),
        );
    }
    Ok(())
}

fn validate_codex_image_routing(
    credentials: &CodexImageCredentials,
    access_token: &str,
) -> Result<(), String> {
    if protocol::chatgpt_account_id_from_oauth_token(access_token)?
        != credentials.chatgpt_account_id
    {
        return Err("The image credential changed its saved account routing".into());
    }
    Ok(())
}
impl std::fmt::Debug for MembershipCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MembershipCredentials")
            .field("tokens", &"[redacted]")
            .finish()
    }
}

// Native-only: the eventual sign-in service must first validate the signed OIDC
// identity. No IPC command accepts raw credentials from JavaScript.
pub(crate) fn protect_credentials(
    identity: &MembershipIdentity,
    credentials: &MembershipCredentials,
) -> Result<ProtectedMembershipRecord, String> {
    validate_identity(identity)?;
    if identity.provider != "chatgpt" {
        return Err("Claude credentials remain with its own official client profile; direct token import is unavailable".into());
    }
    if credentials.issuer != identity.issuer
        || credentials.subject != identity.subject
        || credentials.client_id != identity.client_id
        || credentials.ext_agent_host_id != identity.host_id
    {
        return Err("Credential identity does not match this registration and install".into());
    }
    if credentials.token_type != "Bearer"
        || credentials.access_token.is_empty()
        || credentials.refresh_token.is_empty()
        || credentials.id_token.is_empty()
    {
        return Err("A complete OAuth token record is required".into());
    }
    if !["openid", "resource.invoke", "chatgpt.tokens.use.direct"]
        .iter()
        .all(|scope| credentials.scopes.iter().any(|value| value == scope))
    {
        return Err("The granted scopes do not authorize this account's ChatGPT plan usage".into());
    }
    if let Some(codex) = &credentials.codex_images {
        validate_codex_image_credentials(identity, &credentials.access_token, codex)?;
    }
    let bytes = serde_json::to_vec(credentials)
        .map_err(|_| "Could not serialize the private credential record")?;
    if bytes.len() > 1024 * 1024 {
        return Err("Credential record exceeds the private storage bound".into());
    }
    let entropy = registration_entropy(identity)?;
    let ciphertext = protection::protect(&bytes, &entropy)?;
    Ok(ProtectedMembershipRecord {
        identity: identity.clone(),
        ciphertext,
    })
}

pub(crate) fn open_credentials(
    record: &ProtectedMembershipRecord,
) -> Result<MembershipCredentials, String> {
    validate_identity(&record.identity)?;
    let entropy = registration_entropy(&record.identity)?;
    let bytes = protection::unprotect(&record.ciphertext, &entropy)?;
    let credentials: MembershipCredentials = serde_json::from_slice(&bytes)
        .map_err(|_| "The protected credential record was invalid")?;
    if credentials.issuer != record.identity.issuer
        || credentials.subject != record.identity.subject
        || credentials.client_id != record.identity.client_id
        || credentials.ext_agent_host_id != record.identity.host_id
    {
        return Err("Saved credentials are bound to a different registration".into());
    }
    if let Some(codex) = &credentials.codex_images {
        validate_codex_image_credentials(&record.identity, &credentials.access_token, codex)?;
    }
    Ok(credentials)
}

fn registration_entropy(identity: &MembershipIdentity) -> Result<Vec<u8>, String> {
    // Email can change. Only immutable registration keys bind protected tokens.
    serde_json::to_vec(&(
        "CrowClaw membership v1",
        &identity.provider,
        &identity.issuer,
        &identity.subject,
        &identity.client_id,
        &identity.host_id,
    ))
    .map_err(|_| "Could not bind this credential record".into())
}

#[cfg(all(test, windows))]
mod tests;

#[cfg(all(test, windows))]
mod live_probe;
