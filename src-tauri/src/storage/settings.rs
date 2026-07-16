use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{
    json_to_value, now_ms, require_non_empty, ProviderProfile, ProviderProfileInput, SettingRecord,
    Storage, StorageError, StorageResult,
};

impl Storage {
    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> StorageResult<SettingRecord> {
        require_non_empty("setting key", key)?;
        let value = serde_json::to_value(value)?;
        let value_json = serde_json::to_string(&value)?;
        let updated_at_ms = now_ms()?;
        let connection = self.connection()?;
        connection.execute(
            r#"INSERT INTO settings (key, value_json, updated_at_ms)
               VALUES (?1, ?2, ?3)
               ON CONFLICT(key) DO UPDATE SET
                 value_json = excluded.value_json,
                 updated_at_ms = excluded.updated_at_ms"#,
            params![key, value_json, updated_at_ms],
        )?;
        Ok(SettingRecord {
            key: key.to_owned(),
            value,
            updated_at_ms,
        })
    }

    pub fn get_setting<T: DeserializeOwned>(&self, key: &str) -> StorageResult<Option<T>> {
        require_non_empty("setting key", key)?;
        let connection = self.connection()?;
        let value_json: Option<String> = connection
            .query_row(
                "SELECT value_json FROM settings WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        value_json
            .map(|value| serde_json::from_str(&value).map_err(StorageError::from))
            .transpose()
    }

    pub fn delete_setting(&self, key: &str) -> StorageResult<bool> {
        require_non_empty("setting key", key)?;
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM settings WHERE key = ?1", [key])? > 0)
    }

    pub fn list_settings(&self) -> StorageResult<Vec<SettingRecord>> {
        let connection = self.connection()?;
        list_settings_from(&connection)
    }

    pub fn save_provider_profile(
        &self,
        profile: &ProviderProfileInput,
    ) -> StorageResult<ProviderProfile> {
        validate_provider_profile(profile)?;
        let now = now_ms()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if profile.is_default {
            transaction.execute("UPDATE provider_profiles SET is_default = 0", [])?;
        }
        transaction.execute(
            r#"INSERT INTO provider_profiles (
                   id, name, base_url, model, provider_kind, credential_reference,
                   is_default, created_at_ms, updated_at_ms
               ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
               ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   base_url = excluded.base_url,
                   model = excluded.model,
                   provider_kind = excluded.provider_kind,
                   credential_reference = excluded.credential_reference,
                   is_default = excluded.is_default,
                   updated_at_ms = excluded.updated_at_ms"#,
            params![
                profile.id,
                profile.name,
                profile.base_url,
                profile.model,
                profile.provider_kind,
                profile.credential_reference,
                profile.is_default,
                now,
            ],
        )?;
        let saved = provider_profile_from(&transaction, &profile.id)?
            .ok_or_else(|| StorageError::not_found("provider profile", &profile.id))?;
        transaction.commit()?;
        Ok(saved)
    }

    pub fn get_provider_profile(&self, id: &str) -> StorageResult<Option<ProviderProfile>> {
        require_non_empty("provider profile id", id)?;
        let connection = self.connection()?;
        provider_profile_from(&connection, id)
    }

    pub fn default_provider_profile(&self) -> StorageResult<Option<ProviderProfile>> {
        let connection = self.connection()?;
        let raw = connection
            .query_row(
                r#"SELECT id, name, base_url, model, provider_kind, credential_reference,
                          is_default, created_at_ms, updated_at_ms
                   FROM provider_profiles WHERE is_default = 1"#,
                [],
                provider_profile_row,
            )
            .optional()?;
        Ok(raw.map(provider_profile_from_raw))
    }

    pub fn list_provider_profiles(&self) -> StorageResult<Vec<ProviderProfile>> {
        let connection = self.connection()?;
        list_provider_profiles_from(&connection)
    }

    pub fn set_default_provider_profile(&self, id: &str) -> StorageResult<ProviderProfile> {
        require_non_empty("provider profile id", id)?;
        let now = now_ms()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_profiles WHERE id = ?1)",
            [id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StorageError::not_found("provider profile", id));
        }
        transaction.execute("UPDATE provider_profiles SET is_default = 0", [])?;
        transaction.execute(
            "UPDATE provider_profiles SET is_default = 1, updated_at_ms = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        let profile = provider_profile_from(&transaction, id)?
            .ok_or_else(|| StorageError::not_found("provider profile", id))?;
        transaction.commit()?;
        Ok(profile)
    }

    pub fn delete_provider_profile(&self, id: &str) -> StorageResult<bool> {
        require_non_empty("provider profile id", id)?;
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM provider_profiles WHERE id = ?1", [id])? > 0)
    }
}

type RawProviderProfile = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    bool,
    i64,
    i64,
);

fn provider_profile_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawProviderProfile> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

fn provider_profile_from_raw(raw: RawProviderProfile) -> ProviderProfile {
    ProviderProfile {
        id: raw.0,
        name: raw.1,
        base_url: raw.2,
        model: raw.3,
        provider_kind: raw.4,
        credential_reference: raw.5,
        is_default: raw.6,
        created_at_ms: raw.7,
        updated_at_ms: raw.8,
    }
}

fn provider_profile_from(
    connection: &Connection,
    id: &str,
) -> StorageResult<Option<ProviderProfile>> {
    let raw = connection
        .query_row(
            r#"SELECT id, name, base_url, model, provider_kind, credential_reference,
                      is_default, created_at_ms, updated_at_ms
               FROM provider_profiles WHERE id = ?1"#,
            [id],
            provider_profile_row,
        )
        .optional()?;
    Ok(raw.map(provider_profile_from_raw))
}

pub(crate) fn list_settings_from(connection: &Connection) -> StorageResult<Vec<SettingRecord>> {
    let mut statement = connection.prepare(
        "SELECT key, value_json, updated_at_ms FROM settings ORDER BY key COLLATE NOCASE",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (key, value, updated_at_ms) = row?;
        Ok(SettingRecord {
            key,
            value: json_to_value(value)?,
            updated_at_ms,
        })
    })
    .collect()
}

pub(crate) fn list_provider_profiles_from(
    connection: &Connection,
) -> StorageResult<Vec<ProviderProfile>> {
    let mut statement = connection.prepare(
        r#"SELECT id, name, base_url, model, provider_kind, credential_reference,
                  is_default, created_at_ms, updated_at_ms
           FROM provider_profiles ORDER BY is_default DESC, name COLLATE NOCASE, id"#,
    )?;
    let rows = statement.query_map([], provider_profile_row)?;
    rows.map(|row| Ok(provider_profile_from_raw(row?)))
        .collect()
}

fn validate_provider_profile(profile: &ProviderProfileInput) -> StorageResult<()> {
    require_non_empty("provider profile id", &profile.id)?;
    require_non_empty("provider profile name", &profile.name)?;
    require_non_empty("provider base URL", &profile.base_url)?;
    require_non_empty("provider model", &profile.model)?;
    require_non_empty("provider kind", &profile.provider_kind)
}
