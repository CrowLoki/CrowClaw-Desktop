use crowclaw_desktop_lib::storage::composer::{ConversationModelChoice, MAX_DRAFT_BYTES};
use crowclaw_desktop_lib::storage::{
    ConversationInput, MessageInput, MessageRole, ProviderProfileInput, RetentionChoice, Storage,
    StorageError,
};
use rusqlite::Connection;

fn setup(storage: &Storage) {
    storage
        .save_provider_profile(&ProviderProfileInput {
            id: "local".into(),
            name: "Local".into(),
            base_url: "http://127.0.0.1:1234/v1".into(),
            model: "model-a".into(),
            provider_kind: "lm-studio".into(),
            credential_reference: None,
            is_default: true,
        })
        .unwrap();
    for id in ["chat-a", "chat-b"] {
        storage
            .create_conversation(&ConversationInput {
                id: id.into(),
                title: id.into(),
                provider_profile_id: Some("local".into()),
            })
            .unwrap();
    }
}
fn choice(model: &str) -> ConversationModelChoice {
    ConversationModelChoice {
        provider_profile_id: "local".into(),
        model: model.into(),
        reasoning_effort: None,
    }
}

#[test]
fn drafts_and_model_choices_are_chat_owned_durable_and_do_not_change_defaults() {
    let dir = tempfile::TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    setup(&storage);
    let before = storage.get_conversation("chat-a").unwrap().unwrap();
    assert_eq!(storage.conversation_composer("chat-a").unwrap().revision, 0);
    let a = storage
        .save_conversation_composer("chat-a", 0, "Unsent α draft", Some(&choice("model-a")))
        .unwrap();
    let b = storage
        .save_conversation_composer("chat-b", 0, "Separate β draft", Some(&choice("model-b")))
        .unwrap();
    assert_eq!(storage.get_conversation("chat-a").unwrap().unwrap(), before);
    assert_eq!(
        storage.default_provider_profile().unwrap().unwrap().model,
        "model-a"
    );
    assert!(storage.list_messages("chat-a").unwrap().is_empty());
    drop(storage);
    let reopened = Storage::open(dir.path()).unwrap();
    assert_eq!(reopened.conversation_composer("chat-a").unwrap(), a);
    assert_eq!(reopened.conversation_composer("chat-b").unwrap(), b);
}

#[test]
fn late_autosave_cannot_overwrite_a_newer_choice_from_another_window() {
    let dir = tempfile::TempDir::new().unwrap();
    let first = Storage::open(dir.path()).unwrap();
    setup(&first);
    let second = Storage::open(dir.path()).unwrap();
    let saved = first
        .save_conversation_composer("chat-a", 0, "newer draft", Some(&choice("model-b")))
        .unwrap();
    assert!(matches!(
        second.save_conversation_composer("chat-a", 0, "late old draft", Some(&choice("model-a"))),
        Err(StorageError::Conflict(_))
    ));
    assert_eq!(second.conversation_composer("chat-a").unwrap(), saved);
    let next = second
        .save_conversation_composer(
            "chat-a",
            saved.revision,
            "second edit",
            Some(&choice("model-a")),
        )
        .unwrap();
    assert_eq!(next.revision, 2);
    assert_eq!(saved.selection.unwrap().model, "model-b");
}

#[test]
fn composer_export_and_conversation_deletion_follow_native_retention() {
    let dir = tempfile::TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    setup(&storage);
    storage
        .append_message(&MessageInput {
            id: "message".into(),
            conversation_id: "chat-a".into(),
            role: MessageRole::User,
            content: "submitted message".into(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();
    let saved = storage
        .save_conversation_composer("chat-a", 0, "not submitted", Some(&choice("model-b")))
        .unwrap();
    let export = storage.export_all().unwrap();
    let chat = export
        .conversations
        .iter()
        .find(|chat| chat.conversation.id == "chat-a")
        .unwrap();
    assert_eq!(chat.composer.as_ref(), Some(&saved));
    assert_eq!(chat.messages.len(), 1);
    assert_eq!(chat.messages[0].content, "submitted message");
    storage
        .apply_retention_choice(RetentionChoice::Preserve)
        .unwrap();
    assert_eq!(storage.conversation_composer("chat-a").unwrap(), saved);
    storage.delete_conversation("chat-a").unwrap();
    assert!(storage.conversation_composer("chat-a").is_err());
    assert_eq!(
        Connection::open(storage.database_path())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM conversation_composers", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    storage
        .save_conversation_composer("chat-b", 0, "also removable", None)
        .unwrap();
    assert_eq!(
        storage
            .apply_retention_choice(RetentionChoice::Remove)
            .unwrap()
            .records_after,
        0
    );
}

#[test]
fn invalid_or_orphaned_composer_writes_do_not_create_state() {
    let dir = tempfile::TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    setup(&storage);
    assert!(storage
        .save_conversation_composer("missing", 0, "draft", None)
        .is_err());
    let mut bad = choice("model-a");
    bad.provider_profile_id = "missing".into();
    assert!(storage
        .save_conversation_composer("chat-a", 0, "draft", Some(&bad))
        .is_err());
    assert!(storage
        .save_conversation_composer("chat-a", 0, &"x".repeat(MAX_DRAFT_BYTES + 1), None)
        .is_err());
    assert!(storage
        .save_conversation_composer("chat-a", 0, "nul\0draft", None)
        .is_err());
    assert_eq!(storage.conversation_composer("chat-a").unwrap().revision, 0);
}

#[test]
fn removing_a_connection_does_not_prevent_recovering_its_conversation_draft() {
    let dir = tempfile::TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    setup(&storage);
    let saved = storage
        .save_conversation_composer("chat-a", 0, "original", Some(&choice("model-a")))
        .unwrap();
    storage.delete_provider_profile("local").unwrap();
    let edited = storage
        .save_conversation_composer(
            "chat-a",
            saved.revision,
            "preserved edit",
            saved.selection.as_ref(),
        )
        .unwrap();
    assert_eq!(edited.draft, "preserved edit");
    assert!(storage
        .save_conversation_composer(
            "chat-a",
            edited.revision,
            "bad selection",
            Some(&choice("model-b"))
        )
        .is_err());
    assert_eq!(storage.conversation_composer("chat-a").unwrap(), edited);
}

#[test]
fn schema_eight_upgrade_preserves_messages_and_starts_with_no_invented_choice() {
    let dir = tempfile::TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    setup(&storage);
    storage
        .append_message(&MessageInput {
            id: "original".into(),
            conversation_id: "chat-a".into(),
            role: MessageRole::User,
            content: "original text".into(),
            metadata: serde_json::json!({"preserve":true}),
        })
        .unwrap();
    let before = storage.list_messages("chat-a").unwrap();
    let path = storage.close().unwrap();
    let raw = Connection::open(path).unwrap();
    raw.execute_batch("DROP TABLE conversation_composers; PRAGMA user_version=8;")
        .unwrap();
    drop(raw);
    let upgraded = Storage::open(dir.path()).unwrap();
    assert_eq!(
        upgraded.schema_version().unwrap(),
        crowclaw_desktop_lib::storage::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(upgraded.list_messages("chat-a").unwrap(), before);
    assert_eq!(
        upgraded.conversation_composer("chat-a").unwrap().selection,
        None
    );
}
