//! Native OpenRouter secrets, separate from membership registrations and exports.

use rusqlite::{params, OptionalExtension, TransactionBehavior};

#[cfg(not(test))]
use crate::membership::protection;
// The standalone storage test target includes storage/mod.rs directly. Reuse
// the same platform implementation there without importing membership types.
#[cfg(test)]
#[path = "../membership/protection.rs"]
mod protection;

use super::{
    now_ms, require_non_empty, ProviderProfile, ProviderProfileInput, Storage, StorageError,
    StorageResult,
};

const BASE_URL: &str = "https://openrouter.ai/api/v1";
const ENTROPY_PREFIX: &str = "CrowClaw/OpenRouter/v1\0";

fn entropy(profile_id: &str) -> StorageResult<Vec<u8>> {
    require_non_empty("provider profile id", profile_id)?;
    if profile_id.len() > 8192 - ENTROPY_PREFIX.len() {
        return Err(StorageError::InvalidData(
            "Provider profile id exceeds protection bound".into(),
        ));
    }
    Ok([ENTROPY_PREFIX.as_bytes(), profile_id.as_bytes()].concat())
}

fn validate_key(api_key: &str) -> StorageResult<()> {
    if api_key.len() > 4096 || api_key.trim().is_empty() || api_key.chars().any(char::is_control) {
        return Err(StorageError::InvalidData(
            "OpenRouter API key must be nonblank, at most 4096 bytes, and contain no control characters".into(),
        ));
    }
    Ok(())
}

fn decode(blob: &[u8], entropy: &[u8]) -> StorageResult<String> {
    let bytes = protection::unprotect(blob, entropy).map_err(StorageError::InvalidData)?;
    let key = String::from_utf8(bytes)
        .map_err(|_| StorageError::InvalidData("Protected OpenRouter key is not UTF-8".into()))?;
    validate_key(&key)?;
    Ok(key)
}

impl Storage {
    /// The caller verifies the supplied account/catalog before saving and assigns
    /// a fresh profile ID for each new connection. Secrets never enter profile
    /// fields or membership token records. Protection precedes all SQL writes.
    pub fn save_openrouter_connection(
        &self,
        profile: &ProviderProfileInput,
        api_key: &str,
    ) -> StorageResult<ProviderProfile> {
        self.save_openrouter_connection_inner(profile, api_key, None)
    }

    /// Production connection creation commits the verified catalog together with
    /// the key/profile/default, so a cache failure cannot become partial success.
    pub fn save_openrouter_connection_with_catalog(
        &self,
        profile: &ProviderProfileInput,
        api_key: &str,
        catalog: &serde_json::Value,
    ) -> StorageResult<ProviderProfile> {
        self.save_openrouter_connection_inner(profile, api_key, Some(catalog))
    }

    fn save_openrouter_connection_inner(
        &self,
        profile: &ProviderProfileInput,
        api_key: &str,
        catalog: Option<&serde_json::Value>,
    ) -> StorageResult<ProviderProfile> {
        let entropy = entropy(&profile.id)?;
        require_non_empty("provider profile name", &profile.name)?;
        require_non_empty("provider model", &profile.model)?;
        if profile.provider_kind != "openrouter" || profile.base_url != BASE_URL {
            return Err(StorageError::InvalidData(
                "OpenRouter requires its fixed provider kind and HTTPS endpoint".into(),
            ));
        }
        if profile
            .credential_reference
            .as_deref()
            .is_some_and(|reference| reference != profile.id)
        {
            return Err(StorageError::InvalidData(
                "OpenRouter credential reference must be this profile's ID or absent".into(),
            ));
        }
        validate_key(api_key)?;
        let protected =
            protection::protect(api_key.as_bytes(), &entropy).map_err(StorageError::InvalidData)?;
        let now = now_ms()?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, String, i64)> = tx
            .query_row(
                "SELECT provider_kind,base_url,created_at_ms FROM provider_profiles WHERE id=?1",
                [&profile.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if existing
            .as_ref()
            .is_some_and(|(kind, url, _)| kind != "openrouter" || url != BASE_URL)
        {
            return Err(StorageError::Conflict(
                "Profile ID belongs to a different provider connection".into(),
            ));
        }
        let existing_blob: Option<Vec<u8>> = tx
            .query_row(
                "SELECT protected_blob FROM openrouter_credentials WHERE provider_profile_id=?1",
                [&profile.id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(blob) = existing_blob {
            if decode(&blob, &entropy)? != api_key {
                return Err(StorageError::Conflict(
                    "A different OpenRouter key requires a new connection profile ID".into(),
                ));
            }
        }
        if profile.is_default {
            tx.execute("UPDATE provider_profiles SET is_default=0", [])?;
        }
        tx.execute(
            "INSERT INTO provider_profiles(id,name,base_url,model,provider_kind,credential_reference,is_default,created_at_ms,updated_at_ms)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name,base_url=excluded.base_url,
             model=excluded.model,provider_kind=excluded.provider_kind,credential_reference=excluded.credential_reference,
             is_default=excluded.is_default,updated_at_ms=excluded.updated_at_ms",
            params![profile.id,profile.name,profile.base_url,profile.model,profile.provider_kind,profile.credential_reference,profile.is_default,now],
        )?;
        tx.execute(
            "INSERT INTO openrouter_credentials(provider_profile_id,protected_blob) VALUES(?1,?2)
             ON CONFLICT(provider_profile_id) DO UPDATE SET protected_blob=excluded.protected_blob",
            params![profile.id, protected],
        )?;
        if let Some(catalog) = catalog {
            tx.execute("INSERT INTO settings(key,value_json,updated_at_ms) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at_ms=excluded.updated_at_ms",
                params![format!("openrouter-free-catalog:{}",profile.id),serde_json::to_string(catalog)?,now])?;
        }
        let saved = ProviderProfile {
            id: profile.id.clone(),
            name: profile.name.clone(),
            base_url: profile.base_url.clone(),
            model: profile.model.clone(),
            provider_kind: profile.provider_kind.clone(),
            credential_reference: profile.credential_reference.clone(),
            is_default: profile.is_default,
            created_at_ms: existing.map_or(now, |(_, _, created)| created),
            updated_at_ms: now,
        };
        tx.commit()?;
        Ok(saved)
    }

    /// Native callers only: the returned key must never be logged or exported.
    pub fn load_openrouter_key(&self, profile_id: &str) -> StorageResult<Option<String>> {
        let entropy = entropy(profile_id)?;
        let connection = self.connection()?;
        let blob: Option<Vec<u8>> = connection
            .query_row(
                "SELECT protected_blob FROM openrouter_credentials WHERE provider_profile_id=?1",
                [profile_id],
                |row| row.get(0),
            )
            .optional()?;
        blob.map(|blob| decode(&blob, &entropy)).transpose()
    }

    /// Tests presence only; a corrupt or foreign-user blob may still be present.
    pub fn has_openrouter_key(&self, profile_id: &str) -> StorageResult<bool> {
        entropy(profile_id)?;
        let connection = self.connection()?;
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM openrouter_credentials WHERE provider_profile_id=?1)",
            [profile_id],
            |row| row.get(0),
        )?)
    }

    /// Explicit disconnect: preserve the provider profile and all conversation history.
    pub fn remove_openrouter_key(&self, profile_id: &str) -> StorageResult<()> {
        entropy(profile_id)?;
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM openrouter_credentials WHERE provider_profile_id=?1",
            [profile_id],
        )?;
        Ok(())
    }
}
