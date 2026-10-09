use crowclaw_desktop_lib::storage::{
    attachments::{AttachmentInput, AttachmentKind},
    ConversationInput, MessageInput, MessageRole, ProviderProfileInput, Storage,
    CURRENT_SCHEMA_VERSION,
};
use rusqlite::Connection;

// Deliberately fabricated credentials. These tests never contact a provider.
const KEY: &str = "sk-or-v1-fixture-only-no-real-account";

#[cfg(windows)]
#[test]
fn catalog_failure_rolls_back_connection_key_and_default() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let original = storage
        .save_provider_profile(&profile("original", true))
        .unwrap();
    let raw = Connection::open(storage.database_path()).unwrap();
    raw.execute_batch("CREATE TRIGGER reject_catalog BEFORE INSERT ON settings WHEN NEW.key LIKE 'openrouter-free-catalog:%' BEGIN SELECT RAISE(ABORT,'catalog write rejected'); END;").unwrap();
    drop(raw);
    let catalog = serde_json::json!({"models":[],"fetchedAtMs":1});
    let result =
        storage.save_openrouter_connection_with_catalog(&profile("new", true), KEY, &catalog);
    assert!(result.is_err());
    assert!(storage.get_provider_profile("new").unwrap().is_none());
    assert!(!storage.has_openrouter_key("new").unwrap());
    assert_eq!(storage.default_provider_profile().unwrap(), Some(original));
}

fn profile(id: &str, is_default: bool) -> ProviderProfileInput {
    ProviderProfileInput {
        id: id.into(),
        name: "OpenRouter FREE".into(),
        base_url: "https://openrouter.ai/api/v1".into(),
        model: "fixture/model:free".into(),
        provider_kind: "openrouter".into(),
        credential_reference: Some(id.into()),
        is_default,
    }
}

fn history(storage: &Storage, profile_id: &str) {
    storage
        .create_conversation(&ConversationInput {
            id: "chat".into(),
            title: "Preserve history".into(),
            provider_profile_id: Some(profile_id.into()),
        })
        .unwrap();
    storage
        .append_message(&MessageInput {
            id: "message".into(),
            conversation_id: "chat".into(),
            role: MessageRole::User,
            content: "Original content".into(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();
}

#[test]
fn invalid_inputs_never_create_a_profile_or_change_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let original = storage
        .save_provider_profile(&profile("original", true))
        .unwrap();
    for key in [
        "".to_owned(),
        "   ".into(),
        "x\ny".into(),
        "x\0y".into(),
        "x\u{7f}y".into(),
        "x\u{85}y".into(),
        "x".repeat(4097),
    ] {
        assert!(storage
            .save_openrouter_connection(&profile("new", true), &key)
            .is_err());
    }
    for field in 0..8 {
        let mut input = profile("new", true);
        match field {
            0 => input.id = " ".into(),
            1 => input.name = " ".into(),
            2 => input.model = " ".into(),
            3 => input.provider_kind = "openai_compatible".into(),
            4 => input.base_url = "https://openrouter.ai/api/v1/".into(),
            5 => input.base_url = "http://openrouter.ai/api/v1".into(),
            6 => input.credential_reference = Some(KEY.into()),
            _ => input.id = "x".repeat(8192),
        }
        assert!(storage.save_openrouter_connection(&input, KEY).is_err());
    }
    assert_eq!(
        storage.list_provider_profiles().unwrap(),
        vec![original.clone()]
    );
    assert_eq!(storage.default_provider_profile().unwrap(), Some(original));
    assert!(!storage.has_openrouter_key("new").unwrap());
    assert_eq!(storage.load_openrouter_key("missing").unwrap(), None);
    storage.remove_openrouter_key("missing").unwrap();
    assert!(storage.has_openrouter_key(" ").is_err());
    assert!(storage.load_openrouter_key("").is_err());
    assert!(storage.remove_openrouter_key("").is_err());
}

#[test]
fn schema_ten_upgrade_preserves_profiles_messages_drafts_and_attachments() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let saved = storage
        .save_provider_profile(&profile("old", true))
        .unwrap();
    history(&storage, "old");
    storage
        .save_conversation_composer("chat", 0, "saved draft", None)
        .unwrap();
    let composer = storage
        .add_composer_attachments(
            "chat",
            1,
            &[AttachmentInput {
                id: "attachment".into(),
                name: "note.txt".into(),
                media_type: "text/plain".into(),
                kind: AttachmentKind::Text,
                bytes: b"preserved attachment".to_vec(),
            }],
        )
        .unwrap();
    let messages = storage.list_messages("chat").unwrap();
    let raw = Connection::open(storage.close().unwrap()).unwrap();
    raw.execute_batch("DROP TABLE openrouter_credentials; PRAGMA user_version=10;")
        .unwrap();
    drop(raw);
    let upgraded = Storage::open(dir.path()).unwrap();
    assert_eq!(upgraded.schema_version().unwrap(), 11);
    assert_eq!(CURRENT_SCHEMA_VERSION, 11);
    assert_eq!(upgraded.get_provider_profile("old").unwrap(), Some(saved));
    assert_eq!(upgraded.list_messages("chat").unwrap(), messages);
    assert_eq!(upgraded.conversation_composer("chat").unwrap(), composer);
    assert_eq!(
        upgraded.attachment("chat", "attachment").unwrap().bytes,
        b"preserved attachment"
    );
    assert!(!upgraded.has_openrouter_key("old").unwrap());
    let raw = Connection::open(upgraded.database_path()).unwrap();
    raw.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    assert!(raw
        .execute(
            "INSERT INTO openrouter_credentials VALUES('missing',X'01')",
            []
        )
        .is_err());
    assert!(raw
        .execute(
            "INSERT INTO openrouter_credentials VALUES('old','plaintext')",
            []
        )
        .is_err());
    assert!(raw
        .execute("INSERT INTO openrouter_credentials VALUES('old',X'')", [])
        .is_err());
}

#[test]
fn incorrect_old_schema_fixture_fails_instead_of_hiding_existing_secret_table() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let raw = Connection::open(storage.close().unwrap()).unwrap();
    raw.execute_batch("PRAGMA user_version=10;").unwrap();
    assert!(Storage::open(dir.path()).is_err());
    assert_eq!(
        raw.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        10
    );
}

#[cfg(not(windows))]
#[test]
fn unsupported_platform_has_no_plaintext_fallback_or_partial_profile_write() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let saved = storage
        .save_provider_profile(&profile("old", true))
        .unwrap();
    let error = storage
        .save_openrouter_connection(&profile("new", true), KEY)
        .unwrap_err();
    assert!(error.to_string().contains("platform credential protector"));
    assert_eq!(storage.list_provider_profiles().unwrap(), vec![saved]);
    assert!(!storage.has_openrouter_key("new").unwrap());
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crowclaw_desktop_lib::storage::{composer::ConversationModelChoice, RetentionChoice};

    fn blob(raw: &Connection, id: &str) -> Vec<u8> {
        raw.query_row(
            "SELECT protected_blob FROM openrouter_credentials WHERE provider_profile_id=?1",
            [id],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn real_dpapi_round_trip_restart_default_transition_and_export_exclusion() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        storage
            .save_provider_profile(&profile("previous-default", true))
            .unwrap();
        let saved = storage
            .save_openrouter_connection(&profile("account-a", true), KEY)
            .unwrap();
        assert_eq!(
            storage.default_provider_profile().unwrap(),
            Some(saved.clone())
        );
        assert!(
            !storage
                .get_provider_profile("previous-default")
                .unwrap()
                .unwrap()
                .is_default
        );
        assert!(storage.has_openrouter_key("account-a").unwrap());
        assert!(storage.load_openrouter_key("account-a").unwrap().as_deref() == Some(KEY));
        let raw = Connection::open(storage.database_path()).unwrap();
        let encrypted = blob(&raw, "account-a");
        assert!(!encrypted
            .windows(KEY.len())
            .any(|window| window == KEY.as_bytes()));
        let export = serde_json::to_value(storage.export_all().unwrap()).unwrap();
        let exported = export.to_string();
        assert!(!exported.contains(KEY));
        assert!(!exported.contains("openrouter_credentials"));
        assert!(!exported.contains("protected_blob"));
        assert!(!exported.contains(&serde_json::to_string(&encrypted).unwrap()));
        assert_eq!(
            export["provider_profiles"][0]["credential_reference"],
            "account-a"
        );
        let memberships: i64 = raw.query_row("SELECT (SELECT COUNT(*) FROM membership_accounts)+(SELECT COUNT(*) FROM membership_host)", [], |r| r.get(0)).unwrap();
        assert_eq!(memberships, 0);
        drop(raw);
        storage.close().unwrap();
        // The database and checkpointed WAL contain no fabricated plaintext key.
        let bytes = std::fs::read(dir.path().join("crowclaw.sqlite3")).unwrap();
        assert!(!bytes
            .windows(KEY.len())
            .any(|window| window == KEY.as_bytes()));
        let reopened = Storage::open(dir.path()).unwrap();
        assert_eq!(
            reopened.get_provider_profile("account-a").unwrap(),
            Some(saved)
        );
        assert!(
            reopened
                .load_openrouter_key("account-a")
                .unwrap()
                .as_deref()
                == Some(KEY)
        );
        let mut max = profile("max-key", false);
        max.credential_reference = None;
        let key = "x".repeat(4096);
        reopened.save_openrouter_connection(&max, &key).unwrap();
        assert!(reopened.load_openrouter_key("max-key").unwrap().as_deref() == Some(key.as_str()));
    }

    #[test]
    fn copied_blob_and_tampering_are_rejected_with_no_plaintext_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        storage
            .save_openrouter_connection(&profile("a", true), KEY)
            .unwrap();
        storage
            .save_openrouter_connection(&profile("b", false), "other-fixture-key")
            .unwrap();
        let raw = Connection::open(storage.database_path()).unwrap();
        let encrypted = blob(&raw, "a");
        raw.execute(
            "UPDATE openrouter_credentials SET protected_blob=?1 WHERE provider_profile_id='b'",
            [&encrypted],
        )
        .unwrap();
        assert!(storage.load_openrouter_key("b").is_err());
        assert!(storage.load_openrouter_key("a").unwrap().as_deref() == Some(KEY));
        let mut tampered = encrypted;
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        raw.execute(
            "UPDATE openrouter_credentials SET protected_blob=?1 WHERE provider_profile_id='a'",
            [&tampered],
        )
        .unwrap();
        assert!(storage.has_openrouter_key("a").unwrap());
        assert!(storage.load_openrouter_key("a").is_err());
        raw.execute(
            "UPDATE openrouter_credentials SET protected_blob=?1 WHERE provider_profile_id='a'",
            [KEY.as_bytes()],
        )
        .unwrap();
        let error = storage.load_openrouter_key("a").unwrap_err();
        assert!(!format!("{error:?}").contains(KEY));
    }

    #[test]
    fn key_failure_rolls_back_new_and_existing_profile_and_default_changes() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        storage
            .save_openrouter_connection(&profile("a", true), KEY)
            .unwrap();
        storage
            .save_openrouter_connection(&profile("b", false), KEY)
            .unwrap();
        let before = storage.list_provider_profiles().unwrap();
        let raw = Connection::open(storage.database_path()).unwrap();
        let original_blob = blob(&raw, "b");
        raw.execute_batch("CREATE TRIGGER fail_key_insert BEFORE INSERT ON openrouter_credentials BEGIN SELECT RAISE(ABORT,'fixture insert failure'); END;").unwrap();
        assert!(storage
            .save_openrouter_connection(&profile("new", true), KEY)
            .is_err());
        assert!(storage.get_provider_profile("new").unwrap().is_none());
        assert!(!storage.has_openrouter_key("new").unwrap());
        assert_eq!(storage.list_provider_profiles().unwrap(), before);
        raw.execute_batch("DROP TRIGGER fail_key_insert; CREATE TRIGGER fail_key_update BEFORE UPDATE ON openrouter_credentials BEGIN SELECT RAISE(ABORT,'fixture update failure'); END;").unwrap();
        let mut changed = profile("b", true);
        changed.name = "new name".into();
        changed.model = "different/model:free".into();
        assert!(storage.save_openrouter_connection(&changed, KEY).is_err());
        assert_eq!(storage.list_provider_profiles().unwrap(), before);
        assert_eq!(blob(&raw, "b"), original_blob);
        assert!(storage.load_openrouter_key("b").unwrap().as_deref() == Some(KEY));
    }

    #[test]
    fn account_replacement_is_rejected_and_model_or_draft_changes_preserve_keys() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let saved = storage
            .save_openrouter_connection(&profile("a", true), KEY)
            .unwrap();
        history(&storage, "a");
        assert!(storage
            .save_openrouter_connection(&profile("a", true), "different-account-key")
            .is_err());
        assert_eq!(
            storage.get_provider_profile("a").unwrap(),
            Some(saved.clone())
        );
        let mut changed = profile("a", true);
        changed.model = "another/model:free".into();
        storage.save_provider_profile(&changed).unwrap();
        storage
            .save_conversation_composer(
                "chat",
                0,
                "draft text",
                Some(&ConversationModelChoice {
                    provider_profile_id: "a".into(),
                    model: changed.model.clone(),
                    reasoning_effort: None,
                }),
            )
            .unwrap();
        assert!(storage.load_openrouter_key("a").unwrap().as_deref() == Some(KEY));
        let updated = storage.save_openrouter_connection(&changed, KEY).unwrap();
        assert_eq!(updated.created_at_ms, saved.created_at_ms);
        let mut foreign = profile("local", false);
        foreign.provider_kind = "lm-studio".into();
        foreign.base_url = "http://127.0.0.1:1234/v1".into();
        let foreign_saved = storage.save_provider_profile(&foreign).unwrap();
        assert!(storage
            .save_openrouter_connection(&profile("local", true), KEY)
            .is_err());
        assert_eq!(
            storage.get_provider_profile("local").unwrap(),
            Some(foreign_saved)
        );
    }

    #[test]
    fn disconnect_retains_history_and_other_account_while_full_removal_counts_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let baseline = storage.stored_record_count().unwrap();
        let saved = storage
            .save_openrouter_connection(&profile("a", true), KEY)
            .unwrap();
        assert_eq!(storage.stored_record_count().unwrap(), baseline + 2);
        storage
            .save_openrouter_connection(&profile("b", false), "other-fixture-key")
            .unwrap();
        history(&storage, "a");
        let messages = storage.list_messages("chat").unwrap();
        let count = storage.stored_record_count().unwrap();
        let preserved = storage
            .apply_retention_choice(RetentionChoice::Preserve)
            .unwrap();
        assert_eq!(preserved.records_before, count);
        assert_eq!(preserved.records_after, count);
        storage.remove_openrouter_key("a").unwrap();
        storage.remove_openrouter_key("a").unwrap();
        assert_eq!(storage.stored_record_count().unwrap(), count - 1);
        assert_eq!(storage.get_provider_profile("a").unwrap(), Some(saved));
        assert_eq!(storage.list_messages("chat").unwrap(), messages);
        assert_eq!(
            storage
                .get_conversation("chat")
                .unwrap()
                .unwrap()
                .provider_profile_id
                .as_deref(),
            Some("a")
        );
        assert!(!storage.has_openrouter_key("a").unwrap());
        assert!(storage.load_openrouter_key("a").unwrap().is_none());
        assert!(storage.load_openrouter_key("b").unwrap().as_deref() == Some("other-fixture-key"));
        storage
            .save_openrouter_connection(&profile("cascade", false), KEY)
            .unwrap();
        storage.delete_provider_profile("cascade").unwrap();
        assert!(!storage.has_openrouter_key("cascade").unwrap());
        let removed = storage
            .apply_retention_choice(RetentionChoice::Remove)
            .unwrap();
        assert_eq!(removed.records_after, 0);
        assert_eq!(storage.stored_record_count().unwrap(), 0);
        assert!(!storage.has_openrouter_key("b").unwrap());
    }
}
