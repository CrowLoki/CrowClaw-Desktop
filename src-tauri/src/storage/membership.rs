use super::membership_types::{
    same_registration, validate_identity, MembershipAccount, MembershipCatalog, MembershipIdentity,
    MembershipSelection, ProtectedMembershipRecord,
};
use super::{now_ms, Storage, StorageError, StorageResult};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use std::collections::HashSet;
use uuid::Uuid;

const COLUMNS: &str = "id,label,provider,issuer,subject,client_id,host_id,email,credential_blob IS NOT NULL,credential_version,catalog_json,selection_json,created_at_ms,updated_at_ms";
const MAX_VERSION: u32 = i32::MAX as u32;

impl Storage {
    /// First creation is arbitrated by SQLite, including concurrent processes.
    /// Sign-out and restart retain this profile's opaque identity.
    pub fn membership_host_id(&self) -> StorageResult<String> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let candidate = format!("urn:uuid:{}", Uuid::new_v4());
        tx.execute(
            "INSERT OR IGNORE INTO membership_host(id,host_id) VALUES(1,?1)",
            [&candidate],
        )?;
        let id = host_from(&tx)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn membership_accounts(&self) -> StorageResult<Vec<MembershipAccount>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} FROM membership_accounts ORDER BY created_at_ms,id"
        ))?;
        let rows = statement
            .query_map([], account_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(decode_account).collect()
    }

    pub fn membership_account(&self, id: &str) -> StorageResult<MembershipAccount> {
        let connection = self.connection()?;
        account_from(&connection, id)
    }

    /// Duplicate registrations cannot overwrite saved tokens. Returning sign-in
    /// uses the versioned replacement method, after validating its signed ID.
    pub(crate) fn membership_add_account(
        &self,
        label: &str,
        record: &ProtectedMembershipRecord,
    ) -> StorageResult<MembershipAccount> {
        bounded(label, 160)?;
        validate_record(record)?;
        let now = now_ms()?;
        let id = Uuid::new_v4().to_string();
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_host(&tx, &record.identity.host_id)?;
        let identity = &record.identity;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM membership_accounts WHERE provider=?1 AND (label=?2 OR (issuer=?3 AND subject=?4 AND client_id=?5)))",
            params![identity.provider,label,identity.issuer,identity.subject,identity.client_id], |r| r.get(0))?;
        if exists {
            return Err(StorageError::Conflict(
                "Choose a distinct account label or reconnect the saved registration".into(),
            ));
        }
        tx.execute("INSERT INTO membership_accounts(id,provider,label,issuer,subject,client_id,email,host_id,credential_blob,credential_version,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,?10,?10)",
            params![id,identity.provider,label,identity.issuer,identity.subject,identity.client_id,identity.email,identity.host_id,record.ciphertext,now])?;
        let account = account_from(&tx, &id)?;
        tx.commit()?;
        Ok(account)
    }

    pub(crate) fn membership_protected_credentials(
        &self,
        id: &str,
    ) -> StorageResult<(ProtectedMembershipRecord, u32)> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let account = account_from(&tx, id)?;
        require_host(&tx, &account.identity.host_id)?;
        let ciphertext: Option<Vec<u8>> = tx.query_row(
            "SELECT credential_blob FROM membership_accounts WHERE id=?1",
            [id],
            |row| row.get(0),
        )?;
        let record = ProtectedMembershipRecord {
            identity: account.identity,
            ciphertext: ciphertext
                .ok_or_else(|| StorageError::Conflict("Sign in again to this account".into()))?,
        };
        validate_record(&record)?;
        tx.commit()?;
        Ok((record, account.credential_version))
    }

    /// A late refresh/reconnect cannot restore tokens after disconnect or replace
    /// a newer generation. The provider service must also serialize refresh HTTP
    /// requests: compare-and-swap alone cannot serialize rotating remote tokens.
    pub(crate) fn membership_replace_credentials(
        &self,
        id: &str,
        expected_version: u32,
        record: &ProtectedMembershipRecord,
    ) -> StorageResult<MembershipAccount> {
        validate_record(record)?;
        let next = next_version(expected_version)?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = account_from(&tx, id)?;
        require_host(&tx, &record.identity.host_id)?;
        if !same_registration(&current.identity, &record.identity) {
            return Err(StorageError::InvalidData(
                "Credentials belong to another registration".into(),
            ));
        }
        let changed = tx.execute("UPDATE membership_accounts SET credential_blob=?1,credential_version=?2,email=?3,updated_at_ms=?4 WHERE id=?5 AND credential_version=?6",
            params![record.ciphertext,next,record.identity.email,now_ms()?,id,expected_version])?;
        require_changed(changed)?;
        let account = account_from(&tx, id)?;
        tx.commit()?;
        Ok(account)
    }

    /// Local clear only, not remote revocation. The sign-out service must attempt
    /// revocation and report its result. Registration and host mapping survive.
    pub fn membership_clear_credentials(
        &self,
        id: &str,
        expected_version: u32,
    ) -> StorageResult<MembershipAccount> {
        let next = next_version(expected_version)?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        account_from(&tx, id)?;
        let changed = tx.execute("UPDATE membership_accounts SET credential_blob=NULL,credential_version=?1,catalog_json=NULL,selection_json=NULL,updated_at_ms=?2 WHERE id=?3 AND credential_version=?4",
            params![next,now_ms()?,id,expected_version])?;
        require_changed(changed)?;
        let account = account_from(&tx, id)?;
        tx.commit()?;
        Ok(account)
    }

    /// An old catalog response cannot publish after credential replacement or
    /// sign-out. Choices remain account-specific and retain server ordering.
    pub(crate) fn membership_save_catalog(
        &self,
        id: &str,
        expected_version: u32,
        catalog: &MembershipCatalog,
    ) -> StorageResult<MembershipAccount> {
        validate_catalog(id, catalog)?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = account_from(&tx, id)?;
        require_connected(&current, expected_version)?;
        let selection = current
            .selection
            .filter(|selection| validate_selection(catalog, selection).is_ok());
        tx.execute("UPDATE membership_accounts SET catalog_json=?1,selection_json=?2,updated_at_ms=?3 WHERE id=?4", params![serde_json::to_string(catalog)?,selection.map(|s| serde_json::to_string(&s)).transpose()?,now_ms()?,id])?;
        let account = account_from(&tx, id)?;
        tx.commit()?;
        Ok(account)
    }

    pub fn membership_select(
        &self,
        id: &str,
        expected_version: u32,
        selection: &MembershipSelection,
    ) -> StorageResult<MembershipAccount> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = account_from(&tx, id)?;
        require_connected(&current, expected_version)?;
        let catalog = current.catalog.as_ref().ok_or_else(|| {
            StorageError::Conflict("Fetch this account's model choices before selecting".into())
        })?;
        validate_selection(catalog, selection)?;
        tx.execute(
            "UPDATE membership_accounts SET selection_json=?1,updated_at_ms=?2 WHERE id=?3",
            params![serde_json::to_string(selection)?, now_ms()?, id],
        )?;
        let account = account_from(&tx, id)?;
        tx.commit()?;
        Ok(account)
    }
}

fn host_from(connection: &Connection) -> StorageResult<String> {
    connection
        .query_row("SELECT host_id FROM membership_host WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| {
            StorageError::Conflict(
                "Initialize this install's identity before registering an account".into(),
            )
        })
}
fn require_host(connection: &Connection, host: &str) -> StorageResult<()> {
    if host_from(connection)? != host {
        return Err(StorageError::InvalidData(
            "Account belongs to a different install".into(),
        ));
    }
    Ok(())
}
fn validate_record(record: &ProtectedMembershipRecord) -> StorageResult<()> {
    validate_identity(&record.identity).map_err(StorageError::InvalidData)?;
    if record.identity.provider != "chatgpt"
        || record.ciphertext.is_empty()
        || record.ciphertext.len() > 2 * 1024 * 1024
    {
        return Err(StorageError::InvalidData(
            "A supported protected credential record is required".into(),
        ));
    }
    Ok(())
}
fn bounded(value: &str, max: usize) -> StorageResult<()> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(StorageError::InvalidData(
            "Account display or model metadata is invalid".into(),
        ));
    }
    Ok(())
}
fn validate_catalog(id: &str, catalog: &MembershipCatalog) -> StorageResult<()> {
    if catalog.account_id != id || catalog.models.len() > 256 || catalog.fetched_at_ms <= 0 {
        return Err(StorageError::InvalidData(
            "Model catalog does not belong to this account or exceeds its bounds".into(),
        ));
    }
    let mut slugs = HashSet::new();
    for model in &catalog.models {
        bounded(&model.slug, 256)?;
        bounded(&model.display_name, 256)?;
        if !slugs.insert(&model.slug) || model.reasoning_efforts.len() > 16 {
            return Err(StorageError::InvalidData(
                "Model catalog contains duplicate or excessive choices".into(),
            ));
        }
        let mut efforts = HashSet::new();
        for effort in &model.reasoning_efforts {
            bounded(effort, 64)?;
            if !efforts.insert(effort) {
                return Err(StorageError::InvalidData(
                    "Model effort choices contain duplicates".into(),
                ));
            }
        }
    }
    Ok(())
}
fn validate_selection(
    catalog: &MembershipCatalog,
    selection: &MembershipSelection,
) -> StorageResult<()> {
    if selection.account_id != catalog.account_id {
        return Err(StorageError::InvalidData(
            "Selection belongs to another account".into(),
        ));
    }
    let model = catalog
        .models
        .iter()
        .find(|model| model.slug == selection.model)
        .ok_or_else(|| {
            StorageError::InvalidData("Selected model is not offered by this account".into())
        })?;
    if selection
        .reasoning_effort
        .as_ref()
        .is_some_and(|effort| !model.reasoning_efforts.contains(effort))
    {
        return Err(StorageError::InvalidData(
            "Selected effort is not offered for this account's model".into(),
        ));
    }
    Ok(())
}
fn next_version(version: u32) -> StorageResult<u32> {
    if version >= MAX_VERSION {
        return Err(StorageError::Conflict(
            "Credential version limit reached".into(),
        ));
    }
    Ok(version + 1)
}
fn require_changed(changed: usize) -> StorageResult<()> {
    if changed != 1 {
        return Err(StorageError::Conflict(
            "Account credentials changed; refresh the saved account state".into(),
        ));
    }
    Ok(())
}
fn require_connected(account: &MembershipAccount, version: u32) -> StorageResult<()> {
    if !account.has_credentials || account.credential_version != version {
        return Err(StorageError::Conflict(
            "Account disconnected or credentials changed".into(),
        ));
    }
    Ok(())
}

// Only metadata and presence, never ciphertext, enters a frontend account DTO.
type AccountRow = (MembershipAccount, Option<String>, Option<String>);
fn account_row(row: &Row<'_>) -> rusqlite::Result<AccountRow> {
    Ok((
        MembershipAccount {
            id: row.get(0)?,
            label: row.get(1)?,
            identity: MembershipIdentity {
                provider: row.get(2)?,
                issuer: row.get(3)?,
                subject: row.get(4)?,
                client_id: row.get(5)?,
                host_id: row.get(6)?,
                email: row.get(7)?,
            },
            has_credentials: row.get(8)?,
            credential_version: row.get(9)?,
            catalog: None,
            selection: None,
            created_at_ms: row.get(12)?,
            updated_at_ms: row.get(13)?,
        },
        row.get(10)?,
        row.get(11)?,
    ))
}
fn decode_account(
    (mut account, catalog, selection): AccountRow,
) -> StorageResult<MembershipAccount> {
    validate_identity(&account.identity).map_err(StorageError::InvalidData)?;
    account.catalog = catalog
        .map(|text| serde_json::from_str(&text))
        .transpose()?;
    account.selection = selection
        .map(|text| serde_json::from_str(&text))
        .transpose()?;
    if let Some(catalog) = &account.catalog {
        validate_catalog(&account.id, catalog)?;
    }
    if let Some(selection) = &account.selection {
        validate_selection(
            account.catalog.as_ref().ok_or_else(|| {
                StorageError::InvalidData("Saved selection has no account catalog".into())
            })?,
            selection,
        )?;
    }
    if !account.has_credentials && (account.catalog.is_some() || account.selection.is_some()) {
        return Err(StorageError::InvalidData(
            "Disconnected account retained active model choices".into(),
        ));
    }
    Ok(account)
}
fn account_from(connection: &Connection, id: &str) -> StorageResult<MembershipAccount> {
    let row = connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM membership_accounts WHERE id=?1"),
            [id],
            account_row,
        )
        .optional()?
        .ok_or_else(|| StorageError::not_found("membership account", id))?;
    decode_account(row)
}
