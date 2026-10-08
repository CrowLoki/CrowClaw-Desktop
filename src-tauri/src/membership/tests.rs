use super::*;
use crate::storage::{RetentionChoice, Storage};
use std::sync::{Arc, Barrier};
use tempfile::TempDir;

fn identity(storage: &Storage, client: &str) -> MembershipIdentity {
    MembershipIdentity {
        provider: "chatgpt".into(),
        issuer: "https://auth.openai.com".into(),
        subject: "synthetic-user".into(),
        client_id: client.into(),
        host_id: storage.membership_host_id().unwrap(),
        email: Some("fixture@example.invalid".into()),
    }
}
fn credentials(identity: &MembershipIdentity, generation: &str) -> MembershipCredentials {
    MembershipCredentials {
        issuer: identity.issuer.clone(),
        subject: identity.subject.clone(),
        client_id: identity.client_id.clone(),
        ext_agent_host_id: identity.host_id.clone(),
        id_token: format!("SYNTHETIC-ID-{generation}"),
        access_token: format!("SYNTHETIC-ACCESS-{generation}"),
        refresh_token: format!("SYNTHETIC-REFRESH-{generation}"),
        token_type: "Bearer".into(),
        scopes: vec![
            "openid".into(),
            "resource.invoke".into(),
            "chatgpt.tokens.use.direct".into(),
        ],
        access_expires_at_ms: 1_900_000_000_000,
    }
}
fn protected(identity: &MembershipIdentity, generation: &str) -> ProtectedMembershipRecord {
    protect_credentials(identity, &credentials(identity, generation)).unwrap()
}
fn add(storage: &Storage, client: &str, label: &str) -> MembershipAccount {
    let identity = identity(storage, client);
    storage
        .membership_add_account(label, &protected(&identity, client))
        .unwrap()
}
fn catalog(account: &MembershipAccount, model: &str, efforts: &[&str]) -> MembershipCatalog {
    MembershipCatalog {
        account_id: account.id.clone(),
        models: vec![MembershipModel {
            slug: model.into(),
            display_name: "Account's offered model".into(),
            reasoning_efforts: efforts.iter().map(|s| s.to_string()).collect(),
        }],
        fetched_at_ms: 1_800_000_000_000,
    }
}
fn selection(
    account: &MembershipAccount,
    model: &str,
    effort: Option<&str>,
) -> MembershipSelection {
    MembershipSelection {
        account_id: account.id.clone(),
        model: model.into(),
        reasoning_effort: effort.map(str::to_string),
    }
}

#[test]
fn host_id_survives_restart_and_concurrent_creation_but_not_an_independent_install() {
    let directory = TempDir::new().unwrap();
    let first = Storage::open(directory.path()).unwrap();
    let second = Storage::open(directory.path()).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles = [first, second]
        .into_iter()
        .map(|storage| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                storage.membership_host_id().unwrap()
            })
        })
        .collect::<Vec<_>>();
    let ids = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids[0], ids[1]);
    assert_eq!(
        Storage::open(directory.path())
            .unwrap()
            .membership_host_id()
            .unwrap(),
        ids[0]
    );
    let other = TempDir::new().unwrap();
    assert_ne!(
        Storage::open(other.path())
            .unwrap()
            .membership_host_id()
            .unwrap(),
        ids[0]
    );
    assert_eq!(
        uuid::Uuid::parse_str(ids[0].strip_prefix("urn:uuid:").unwrap())
            .unwrap()
            .get_version_num(),
        4
    );
}

#[test]
fn windows_protection_roundtrips_redacts_and_binds_immutable_registration_not_email() {
    let directory = TempDir::new().unwrap();
    let storage = Storage::open(directory.path()).unwrap();
    let identity = identity(&storage, "issued-client");
    let creds = credentials(&identity, "ONE");
    let mut record = protect_credentials(&identity, &creds).unwrap();
    assert!(!record
        .ciphertext
        .windows(creds.access_token.len())
        .any(|bytes| bytes == creds.access_token.as_bytes()));
    assert!(!format!("{record:?} {creds:?}").contains("SYNTHETIC-"));
    assert_eq!(
        open_credentials(&record).unwrap().access_token,
        creds.access_token
    );
    record.identity.email = Some("new-display@example.invalid".into());
    assert_eq!(
        open_credentials(&record).unwrap().refresh_token,
        creds.refresh_token
    );
    record.identity.client_id = "wrong-client".into();
    let error = open_credentials(&record).unwrap_err();
    assert!(!error.contains("SYNTHETIC-"));
    record.identity.client_id = identity.client_id;
    record.ciphertext[0] ^= 1;
    assert!(open_credentials(&record).is_err());
}

#[test]
fn same_email_registrations_remain_separate_protected_and_durable() {
    let directory = TempDir::new().unwrap();
    let storage = Storage::open(directory.path()).unwrap();
    let first = add(&storage, "issued-personal", "Personal");
    let second = add(&storage, "issued-workspace", "Workspace");
    assert_eq!(first.identity.email, second.identity.email);
    assert_ne!(first.id, second.id);
    let duplicate = protected(&first.identity, "SHOULD-NOT-REPLACE");
    assert!(storage
        .membership_add_account("Another label", &duplicate)
        .is_err());
    assert!(storage
        .membership_add_account(
            "Personal",
            &protected(&identity(&storage, "third-client"), "THIRD")
        )
        .is_err());
    drop(storage);
    let reopened = Storage::open(directory.path()).unwrap();
    assert_eq!(reopened.membership_accounts().unwrap().len(), 2);
    let (first_saved, _) = reopened
        .membership_protected_credentials(&first.id)
        .unwrap();
    let (second_saved, _) = reopened
        .membership_protected_credentials(&second.id)
        .unwrap();
    assert_eq!(
        open_credentials(&first_saved).unwrap().access_token,
        "SYNTHETIC-ACCESS-issued-personal"
    );
    assert_eq!(
        open_credentials(&second_saved).unwrap().access_token,
        "SYNTHETIC-ACCESS-issued-workspace"
    );
    let json = serde_json::to_string(&reopened.membership_accounts().unwrap()).unwrap();
    assert!(
        !json.contains("SYNTHETIC-")
            && !json.contains("ciphertext")
            && !json.contains("credentialBlob")
    );
}

#[test]
fn registration_rejects_another_install_and_replacement_rejects_another_account() {
    let first_dir = TempDir::new().unwrap();
    let second_dir = TempDir::new().unwrap();
    let first = Storage::open(first_dir.path()).unwrap();
    let second = Storage::open(second_dir.path()).unwrap();
    second.membership_host_id().unwrap();
    let account = add(&first, "first-client", "First");
    let record = protected(&account.identity, "NEW");
    assert!(second.membership_add_account("Copied", &record).is_err());
    let other = add(&first, "other-client", "Other");
    assert!(first
        .membership_replace_credentials(
            &account.id,
            account.credential_version,
            &protected(&other.identity, "OTHER")
        )
        .is_err());
    assert_eq!(first.membership_account(&account.id).unwrap(), account);
}

#[test]
fn concurrent_token_replacement_has_one_winner_and_signout_defeats_late_results() {
    let directory = TempDir::new().unwrap();
    let storage = Storage::open(directory.path()).unwrap();
    let account = add(&storage, "issued-client", "Personal");
    let barrier = Arc::new(Barrier::new(2));
    let handles = ["A", "B"]
        .into_iter()
        .map(|generation| {
            let storage = Storage::open(directory.path()).unwrap();
            let account = account.clone();
            let record = protected(&account.identity, generation);
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                storage.membership_replace_credentials(
                    &account.id,
                    account.credential_version,
                    &record,
                )
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let current = storage.membership_account(&account.id).unwrap();
    assert_eq!(current.credential_version, 2);
    let old_catalog = catalog(&account, "offered-model", &["high"]);
    assert!(storage
        .membership_save_catalog(&account.id, 1, &old_catalog)
        .is_err());
    storage
        .membership_save_catalog(&account.id, 2, &old_catalog)
        .unwrap();
    let signed_out = storage
        .membership_clear_credentials(&account.id, 2)
        .unwrap();
    assert!(
        !signed_out.has_credentials
            && signed_out.catalog.is_none()
            && signed_out.selection.is_none()
    );
    assert_eq!(signed_out.identity, account.identity);
    assert!(storage
        .membership_protected_credentials(&account.id)
        .is_err());
    assert!(storage
        .membership_replace_credentials(&account.id, 2, &protected(&account.identity, "LATE"))
        .is_err());
    assert!(storage
        .membership_save_catalog(&account.id, 2, &old_catalog)
        .is_err());
    let reconnected = storage
        .membership_replace_credentials(&account.id, 3, &protected(&account.identity, "RECONNECT"))
        .unwrap();
    assert!(reconnected.has_credentials);
    assert_eq!(reconnected.identity.client_id, account.identity.client_id);
    assert_eq!(
        storage.membership_host_id().unwrap(),
        account.identity.host_id
    );
}

#[test]
fn model_and_effort_choices_are_strictly_account_bound_and_refreshed() {
    let directory = TempDir::new().unwrap();
    let storage = Storage::open(directory.path()).unwrap();
    let first = add(&storage, "first-client", "First");
    let second = add(&storage, "second-client", "Second");
    let first_catalog = catalog(&first, "first-model", &["high"]);
    let second_catalog = catalog(&second, "second-model", &["low"]);
    assert!(storage
        .membership_save_catalog(&second.id, 1, &first_catalog)
        .is_err());
    storage
        .membership_save_catalog(&first.id, 1, &first_catalog)
        .unwrap();
    storage
        .membership_save_catalog(&second.id, 1, &second_catalog)
        .unwrap();
    assert!(storage
        .membership_select(
            &first.id,
            1,
            &selection(&second, "second-model", Some("low"))
        )
        .is_err());
    assert!(storage
        .membership_select(&first.id, 1, &selection(&first, "second-model", None))
        .is_err());
    assert!(storage
        .membership_select(&first.id, 1, &selection(&first, "first-model", Some("low")))
        .is_err());
    let selected = storage
        .membership_select(
            &first.id,
            1,
            &selection(&first, "first-model", Some("high")),
        )
        .unwrap();
    assert_eq!(
        selected.selection.unwrap().reasoning_effort.as_deref(),
        Some("high")
    );
    assert!(storage
        .membership_account(&second.id)
        .unwrap()
        .selection
        .is_none());
    let mut invalid = first_catalog.clone();
    invalid.models.push(invalid.models[0].clone());
    assert!(storage
        .membership_save_catalog(&first.id, 1, &invalid)
        .is_err());
    invalid = first_catalog.clone();
    invalid.models[0].reasoning_efforts.push("high".into());
    assert!(storage
        .membership_save_catalog(&first.id, 1, &invalid)
        .is_err());
    let changed = catalog(&first, "replacement-model", &[]);
    assert!(storage
        .membership_save_catalog(&first.id, 1, &changed)
        .unwrap()
        .selection
        .is_none());
    assert!(storage
        .membership_select(&first.id, 1, &selection(&first, "replacement-model", None))
        .is_ok());
    let empty = MembershipCatalog {
        models: vec![],
        ..changed
    };
    assert!(storage
        .membership_save_catalog(&first.id, 1, &empty)
        .unwrap()
        .selection
        .is_none());
}

#[test]
fn exports_exclude_accounts_and_credentials_and_full_remove_clears_the_vault() {
    let directory = TempDir::new().unwrap();
    let storage = Storage::open(directory.path()).unwrap();
    storage
        .set_setting("existing-choice", &serde_json::json!("local-model"))
        .unwrap();
    let before_settings = storage.export_all().unwrap().settings;
    let account = add(&storage, "never-export-client", "Private registration");
    let exported = storage.export_all().unwrap();
    assert_eq!(exported.settings, before_settings);
    let json = serde_json::to_string(&exported).unwrap();
    for private in [
        &account.id,
        &account.identity.client_id,
        &account.identity.host_id,
        "fixture@example.invalid",
        "SYNTHETIC-",
    ] {
        assert!(!json.contains(private));
    }
    let preserved = storage
        .apply_retention_choice(RetentionChoice::Preserve)
        .unwrap();
    assert_eq!(preserved.records_before, preserved.records_after);
    assert!(storage
        .membership_protected_credentials(&account.id)
        .is_ok());
    let removed = storage
        .apply_retention_choice(RetentionChoice::Remove)
        .unwrap();
    assert_eq!(removed.records_after, 0);
    assert!(storage.membership_accounts().unwrap().is_empty());
    assert_ne!(
        storage.membership_host_id().unwrap(),
        account.identity.host_id
    );
}

#[test]
fn token_rotation_preserves_the_same_accounts_model_choice_until_catalog_refresh() {
    let directory = TempDir::new().unwrap();
    let storage = Storage::open(directory.path()).unwrap();
    let account = add(&storage, "issued-client", "Personal");
    storage
        .membership_save_catalog(
            &account.id,
            1,
            &catalog(&account, "offered-model", &["high"]),
        )
        .unwrap();
    let selected = storage
        .membership_select(
            &account.id,
            1,
            &selection(&account, "offered-model", Some("high")),
        )
        .unwrap();
    let rotated = storage
        .membership_replace_credentials(&account.id, 1, &protected(&account.identity, "ROTATED"))
        .unwrap();
    assert_eq!(rotated.selection, selected.selection);
    assert_eq!(rotated.catalog, selected.catalog);
    assert!(storage
        .membership_save_catalog(&account.id, 1, &catalog(&account, "stale-model", &[]))
        .is_err());
    let refreshed = storage
        .membership_save_catalog(&account.id, 2, &catalog(&account, "replacement-model", &[]))
        .unwrap();
    assert!(refreshed.selection.is_none());
    let cleared = storage
        .membership_clear_credentials(&account.id, 2)
        .unwrap();
    assert!(cleared.catalog.is_none() && cleared.selection.is_none());
}

#[test]
fn schema_six_upgrade_adds_vault_without_changing_existing_content() {
    let directory = TempDir::new().unwrap();
    let storage = Storage::open(directory.path()).unwrap();
    storage
        .set_setting("existing-choice", &serde_json::json!({"provider":"local"}))
        .unwrap();
    let before = storage.export_all().unwrap().settings;
    let path = storage.close().unwrap();
    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .execute_batch(
            "DROP TABLE membership_accounts; DROP TABLE membership_host; PRAGMA user_version=6;",
        )
        .unwrap();
    drop(connection);
    let upgraded = Storage::open(directory.path()).unwrap();
    assert_eq!(upgraded.schema_version().unwrap(), 7);
    assert_eq!(upgraded.export_all().unwrap().settings, before);
    assert!(upgraded.membership_accounts().unwrap().is_empty());
    assert!(upgraded.membership_host_id().is_ok());
}

#[test]
fn invalid_identity_scope_and_cross_registration_tokens_are_refused_before_storage() {
    let directory = TempDir::new().unwrap();
    let storage = Storage::open(directory.path()).unwrap();
    let mut identity = identity(&storage, "issued-client");
    let mut creds = credentials(&identity, "ONE");
    creds
        .scopes
        .retain(|scope| scope != "chatgpt.tokens.use.direct");
    assert!(protect_credentials(&identity, &creds).is_err());
    creds = credentials(&identity, "ONE");
    creds.client_id = "different-client".into();
    assert!(protect_credentials(&identity, &creds).is_err());
    identity.client_id = "dynamic_agent_client".into();
    assert!(protect_credentials(&identity, &credentials(&identity, "ONE")).is_err());
    identity.client_id = "issued-client".into();
    identity.issuer = "https://example.invalid".into();
    assert!(protect_credentials(&identity, &credentials(&identity, "ONE")).is_err());
    identity.issuer = "https://auth.openai.com".into();
    identity.provider = "claude".into();
    assert!(protect_credentials(&identity, &credentials(&identity, "ONE")).is_err());
}
