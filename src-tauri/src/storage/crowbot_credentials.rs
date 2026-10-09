//! Per-user supplier credentials. Never part of settings, IPC DTOs or exports.
use super::{now_ms, ProviderProfile, ProviderProfileInput, Storage, StorageError, StorageResult};
#[cfg(not(test))]
use crate::membership::protection;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
#[cfg(test)]
#[path = "../membership/protection.rs"]
mod protection;

fn entropy(id: &str, endpoint: &str) -> StorageResult<Vec<u8>> {
    if id.is_empty() || id.len() > 256 || endpoint.len() > 2048 {
        return Err(StorageError::InvalidData(
            "Invalid CrowBot connection binding".into(),
        ));
    }
    Ok(serde_json::to_vec(&("CrowClaw/CrowBot/v1", id, endpoint))?)
}
impl Storage {
    pub fn save_crowbot_connection(
        &self,
        profile: &ProviderProfileInput,
        key: Option<&str>,
    ) -> StorageResult<ProviderProfile> {
        if profile.provider_kind != "crowbot-ai"
            || profile.model != "crowbot-auto"
            || profile.credential_reference.is_some()
        {
            return Err(StorageError::InvalidData(
                "Invalid CrowBot provider profile".into(),
            ));
        }
        let blob = key
            .map(|key| {
                if key.trim().is_empty() || key.len() > 4096 || key.chars().any(char::is_control) {
                    return Err(StorageError::InvalidData(
                        "Invalid CrowBot gateway key".into(),
                    ));
                }
                protection::protect(key.as_bytes(), &entropy(&profile.id, &profile.base_url)?)
                    .map_err(|_| {
                        StorageError::InvalidData(
                            "Could not protect the personal CrowBot gateway key".into(),
                        )
                    })
            })
            .transpose()?;
        let now = now_ms()?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if profile.is_default {
            tx.execute("UPDATE provider_profiles SET is_default=0", [])?;
        }
        tx.execute("INSERT INTO provider_profiles(id,name,base_url,model,provider_kind,credential_reference,is_default,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,NULL,?6,?7,?7)",params![profile.id,profile.name,profile.base_url,profile.model,profile.provider_kind,profile.is_default,now])?;
        if let Some(blob) = blob {
            tx.execute(
                "INSERT INTO crowbot_credentials(provider_profile_id,protected_blob) VALUES(?1,?2)",
                params![profile.id, blob],
            )?;
        }
        tx.commit()?;
        drop(connection);
        self.get_provider_profile(&profile.id)?
            .ok_or_else(|| StorageError::not_found("provider profile", &profile.id))
    }
    pub fn crowbot_key(&self, profile: &ProviderProfile) -> StorageResult<Option<String>> {
        if profile.provider_kind != "crowbot-ai" {
            return Err(StorageError::InvalidData("Not a CrowBot connection".into()));
        }
        let connection = self.connection()?;
        let blob: Option<Vec<u8>> = connection
            .query_row(
                "SELECT protected_blob FROM crowbot_credentials WHERE provider_profile_id=?1",
                [&profile.id],
                |row| row.get(0),
            )
            .optional()?;
        blob.map(|blob| {
            let bytes = protection::unprotect(&blob, &entropy(&profile.id, &profile.base_url)?)
                .map_err(|_| {
                    StorageError::InvalidData(
                        "CrowBot gateway key does not match this user and connection".into(),
                    )
                })?;
            String::from_utf8(bytes).map_err(|_| {
                StorageError::InvalidData("Invalid protected CrowBot gateway key".into())
            })
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_connection_restart_needs_no_credential_and_exports_no_secret_table() {
        let dir = tempfile::TempDir::new().unwrap();
        let profile = ProviderProfileInput {
            id: "local-crowbot".into(),
            name: "CrowBot AI".into(),
            base_url: "http://127.0.0.1:12345/api/crowbot-ai/v1".into(),
            model: "crowbot-auto".into(),
            provider_kind: "crowbot-ai".into(),
            credential_reference: None,
            is_default: false,
        };
        let storage = Storage::open(dir.path()).unwrap();
        let saved = storage.save_crowbot_connection(&profile, None).unwrap();
        assert!(storage.crowbot_key(&saved).unwrap().is_none());
        drop(storage);
        let reopened = Storage::open(dir.path()).unwrap();
        assert_eq!(
            reopened
                .get_provider_profile("local-crowbot")
                .unwrap()
                .unwrap()
                .model,
            "crowbot-auto"
        );
    }
    #[cfg(windows)]
    #[test]
    fn hosted_key_is_protected_bound_not_exported_and_retained_until_remove() {
        let dir = tempfile::TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let profile = ProviderProfileInput {
            id: "hosted-crowbot".into(),
            name: "CrowBot AI".into(),
            base_url: "https://supplier.example/api/crowbot-ai/v1".into(),
            model: "crowbot-auto".into(),
            provider_kind: "crowbot-ai".into(),
            credential_reference: None,
            is_default: false,
        };
        let saved = storage
            .save_crowbot_connection(&profile, Some("fabricated-test-only-gateway-key"))
            .unwrap();
        assert_eq!(
            storage.crowbot_key(&saved).unwrap().as_deref(),
            Some("fabricated-test-only-gateway-key")
        );
        let export = serde_json::to_string(&storage.export_all().unwrap()).unwrap();
        assert!(!export.contains("fabricated-test-only") && !export.contains("protected_blob"));
        let mut wrong = saved.clone();
        wrong.base_url = "https://other.example/api/crowbot-ai/v1".into();
        assert!(storage.crowbot_key(&wrong).is_err());
        storage
            .apply_retention_choice(super::super::RetentionChoice::Preserve)
            .unwrap();
        assert!(storage.crowbot_key(&saved).unwrap().is_some());
        storage
            .apply_retention_choice(super::super::RetentionChoice::Remove)
            .unwrap();
        assert!(storage.get_provider_profile(&saved.id).unwrap().is_none());
    }
}
