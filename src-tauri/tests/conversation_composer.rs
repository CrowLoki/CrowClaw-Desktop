use crowclaw_desktop_lib::storage::composer::{ConversationModelChoice, MAX_DRAFT_BYTES};
use crowclaw_desktop_lib::storage::{
    ConversationInput, MessageInput, MessageRole, ProviderProfileInput, RetentionChoice, Storage,
    StorageError, TaskInput, TaskStatus,
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

fn turn_inputs() -> (MessageInput, TaskInput) {
    (
        MessageInput {
            id: "submitted-user".into(),
            conversation_id: "chat-a".into(),
            role: MessageRole::User,
            content: "Send this α text".into(),
            metadata: serde_json::json!({"attachment": "document", "modelSelection": "forged"}),
        },
        TaskInput {
            id: "submitted-task".into(),
            conversation_id: Some("chat-a".into()),
            kind: "agent-turn".into(),
            payload: serde_json::json!({"title": "Turn", "modelSelection": "forged", "composerRevision": 999}),
        },
    )
}

fn saved_state(storage: &Storage) -> serde_json::Value {
    let mut export = storage.export_all().unwrap();
    // Export time describes the read, not persisted state.
    export.exported_at_ms = 0;
    serde_json::to_value(export).unwrap()
}

#[test]
fn submit_atomically_freezes_selection_and_clears_only_the_saved_draft() {
    let dir = tempfile::TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    setup(&storage);
    let mut selected = choice("model-b");
    selected.reasoning_effort = Some("high".into());
    let composer = storage
        .save_conversation_composer("chat-a", 0, "unsent draft", Some(&selected))
        .unwrap();
    let (message, task) = turn_inputs();
    let mut earlier = message.clone();
    earlier.id = "earlier".into();
    storage.append_message(&earlier).unwrap();
    Connection::open(storage.database_path())
        .unwrap()
        .execute(
            "UPDATE conversations SET updated_at_ms=1 WHERE id='chat-a'",
            [],
        )
        .unwrap();
    let other = storage.get_conversation("chat-b").unwrap();
    let (saved_message, saved_task, next) = storage
        .begin_composer_turn(&composer, &message, &task)
        .unwrap();
    let snapshot = serde_json::to_value(&selected).unwrap();
    assert_eq!(saved_message.ordinal, 1);
    assert_eq!(saved_message.content, message.content);
    assert_eq!(
        saved_message.metadata,
        serde_json::json!({"attachment": "document", "modelSelection": snapshot})
    );
    assert_eq!(
        saved_task.payload,
        serde_json::json!({"title": "Turn", "modelSelection": snapshot, "composerRevision": composer.revision})
    );
    assert_eq!(saved_task.status, TaskStatus::Queued);
    assert!(!saved_task.cancellation_requested);
    assert_eq!(next.revision, composer.revision + 1);
    assert_eq!(next.draft, "");
    assert_eq!(next.selection, composer.selection);
    let conversation = storage.get_conversation("chat-a").unwrap().unwrap();
    assert_eq!(conversation.updated_at_ms, saved_message.created_at_ms);
    assert!(conversation.updated_at_ms > 1);
    assert_eq!(conversation.title, "chat-a");
    assert_eq!(storage.get_conversation("chat-b").unwrap(), other);
    assert_eq!(
        storage.default_provider_profile().unwrap().unwrap().model,
        "model-a"
    );
    storage
        .save_conversation_composer(
            "chat-a",
            next.revision,
            "later draft",
            Some(&choice("model-a")),
        )
        .unwrap();
    drop(storage);
    let reopened = Storage::open(dir.path()).unwrap();
    assert_eq!(reopened.get_task(&task.id).unwrap(), Some(saved_task));
    assert_eq!(reopened.list_messages("chat-a").unwrap()[1], saved_message);
    assert_eq!(
        reopened.conversation_composer("chat-a").unwrap().draft,
        "later draft"
    );
    assert!(matches!(
        reopened.save_conversation_composer(
            "chat-a",
            composer.revision,
            "late autosave",
            composer.selection.as_ref()
        ),
        Err(StorageError::Conflict(_))
    ));
}

#[test]
fn stale_revision_or_selection_cannot_submit() {
    let dir = tempfile::TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    setup(&storage);
    let old = storage
        .save_conversation_composer("chat-a", 0, "draft", Some(&choice("model-a")))
        .unwrap();
    let second = Storage::open(dir.path()).unwrap();
    let current = second
        .save_conversation_composer(
            "chat-a",
            old.revision,
            "new draft",
            Some(&choice("model-b")),
        )
        .unwrap();
    let before = saved_state(&storage);
    let (message, task) = turn_inputs();
    let mut wrong_selection = current.clone();
    wrong_selection.selection = old.selection.clone();
    for expected in [old, wrong_selection] {
        assert!(matches!(
            storage.begin_composer_turn(&expected, &message, &task),
            Err(StorageError::Conflict(_))
        ));
        assert_eq!(saved_state(&storage), before);
    }
}

#[test]
fn duplicate_ids_roll_back_message_task_composer_recency_and_trigger_effects() {
    for duplicate_task in [false, true] {
        let dir = tempfile::TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        setup(&storage);
        let composer = storage
            .save_conversation_composer("chat-a", 0, "keep draft", Some(&choice("model-a")))
            .unwrap();
        let (message, task) = turn_inputs();
        if duplicate_task {
            storage.create_task(&task).unwrap();
        } else {
            storage.append_message(&message).unwrap();
        }
        let before = saved_state(&storage);
        let raw = Connection::open(storage.database_path()).unwrap();
        let source_count: i64 = raw
            .query_row("SELECT COUNT(*) FROM memory_jobs", [], |row| row.get(0))
            .unwrap();
        assert!(storage
            .begin_composer_turn(&composer, &message, &task)
            .is_err());
        assert_eq!(saved_state(&storage), before);
        assert_eq!(
            raw.query_row("SELECT COUNT(*) FROM memory_jobs", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            source_count
        );
        assert_eq!(storage.conversation_composer("chat-a").unwrap(), composer);
    }
}

#[test]
fn invalid_submission_setup_leaves_all_saved_state_unchanged() {
    let dir = tempfile::TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    setup(&storage);
    let unselected = storage.conversation_composer("chat-b").unwrap();
    let composer = storage
        .save_conversation_composer("chat-a", 0, "keep draft", Some(&choice("model-a")))
        .unwrap();
    let before = saved_state(&storage);
    let (message, task) = turn_inputs();
    let mut cases = Vec::new();
    for role in [
        MessageRole::Assistant,
        MessageRole::System,
        MessageRole::Tool,
    ] {
        let mut m = message.clone();
        m.role = role;
        cases.push((composer.clone(), m, task.clone()));
    }
    for content in [
        " ".into(),
        "".into(),
        "bad\0text".into(),
        "α".repeat(MAX_DRAFT_BYTES / 2 + 1),
    ] {
        let mut m = message.clone();
        m.content = content;
        cases.push((composer.clone(), m, task.clone()));
    }
    let mut m = message.clone();
    m.conversation_id = "chat-b".into();
    cases.push((composer.clone(), m, task.clone()));
    for id in [None, Some("chat-b".into())] {
        let mut t = task.clone();
        t.conversation_id = id;
        cases.push((composer.clone(), message.clone(), t));
    }
    let mut t = task.clone();
    t.kind = "other".into();
    cases.push((composer.clone(), message.clone(), t));
    let mut m = message.clone();
    m.metadata = serde_json::json!([1]);
    cases.push((composer.clone(), m, task.clone()));
    let mut t = task.clone();
    t.payload = serde_json::json!(42);
    cases.push((composer.clone(), message.clone(), t));
    for id in ["chat-b", "missing"] {
        let mut c = unselected.clone();
        c.conversation_id = id.into();
        let mut m = message.clone();
        m.conversation_id = id.into();
        let mut t = task.clone();
        t.conversation_id = Some(id.into());
        cases.push((c, m, t));
    }
    for (c, m, t) in cases {
        assert!(storage.begin_composer_turn(&c, &m, &t).is_err());
        assert_eq!(saved_state(&storage), before);
    }
}

#[test]
fn late_database_failure_rolls_back_the_whole_submission() {
    let dir = tempfile::TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    setup(&storage);
    let composer = storage
        .save_conversation_composer("chat-a", 0, "keep draft", Some(&choice("model-a")))
        .unwrap();
    let before = saved_state(&storage);
    let raw = Connection::open(storage.database_path()).unwrap();
    raw.execute_batch("CREATE TRIGGER fail_composer_update BEFORE UPDATE ON conversation_composers BEGIN SELECT RAISE(ABORT, 'fixture setup failure'); END;").unwrap();
    let (message, task) = turn_inputs();
    assert!(storage
        .begin_composer_turn(&composer, &message, &task)
        .is_err());
    assert_eq!(saved_state(&storage), before);
    raw.execute_batch("DROP TRIGGER fail_composer_update;")
        .unwrap();
    let mut message = message;
    message.metadata = serde_json::Value::Null;
    let mut task = task;
    task.payload = serde_json::Value::Null;
    storage
        .begin_composer_turn(&composer, &message, &task)
        .unwrap();
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
