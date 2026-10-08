use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembershipIdentity {
    pub provider: String,
    pub issuer: String,
    pub subject: String,
    pub client_id: String,
    pub host_id: String,
    pub email: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MembershipModel {
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembershipCatalog {
    pub account_id: String,
    pub models: Vec<MembershipModel>,
    pub fetched_at_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembershipSelection {
    pub account_id: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MembershipAccount {
    pub id: String,
    pub label: String,
    pub identity: MembershipIdentity,
    pub has_credentials: bool,
    pub credential_version: u32,
    pub catalog: Option<MembershipCatalog>,
    pub selection: Option<MembershipSelection>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

// Secret bytes never enter the frontend DTO or normal Debug formatting.
pub struct ProtectedMembershipRecord {
    pub(crate) identity: MembershipIdentity,
    pub(crate) ciphertext: Vec<u8>,
}
impl std::fmt::Debug for ProtectedMembershipRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtectedMembershipRecord")
            .field("credentials", &"[protected]")
            .finish()
    }
}

pub(crate) fn validate_identity(identity: &MembershipIdentity) -> Result<(), String> {
    if !matches!(identity.provider.as_str(), "chatgpt" | "claude") {
        return Err("Account provider is unsupported".into());
    }
    for value in [
        &identity.issuer,
        &identity.subject,
        &identity.client_id,
        &identity.host_id,
    ] {
        if value.trim().is_empty() || value.len() > 1024 || value.contains('\0') {
            return Err("Account registration identity is invalid".into());
        }
    }
    if identity
        .email
        .as_ref()
        .is_some_and(|email| email.len() > 1024 || email.contains('\0'))
    {
        return Err("Account display metadata is invalid".into());
    }
    if identity.client_id == "dynamic_agent_client" {
        return Err("Initial registration entrypoint is not an issued client identifier".into());
    }
    if identity.provider == "chatgpt" && identity.issuer != "https://auth.openai.com" {
        return Err("ChatGPT account issuer is invalid".into());
    }
    let host = identity
        .host_id
        .strip_prefix("urn:uuid:")
        .ok_or("Host identifier is not an opaque UUID URI")?;
    let id = uuid::Uuid::parse_str(host).map_err(|_| "Host identifier is invalid")?;
    if id.get_version_num() != 4 || id.get_variant() != uuid::Variant::RFC4122 {
        return Err("Host identifier must be UUIDv4".into());
    }
    Ok(())
}

pub(crate) fn same_registration(a: &MembershipIdentity, b: &MembershipIdentity) -> bool {
    a.provider == b.provider
        && a.issuer == b.issuer
        && a.subject == b.subject
        && a.client_id == b.client_id
        && a.host_id == b.host_id
}
