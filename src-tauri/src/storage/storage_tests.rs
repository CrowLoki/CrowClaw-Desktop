#![allow(dead_code)]

#[path = "mod.rs"]
mod storage;

use rusqlite::Connection;
use serde_json::{json, Value};
use storage::*;
use tempfile::TempDir;

fn provider() -> ProviderProfileInput {
    ProviderProfileInput {
        id: "provider-local".into(),
        name: "Local model".into(),
        base_url: "http://127.0.0.1:1234/v1".into(),
        model: "local-test-model".into(),
        provider_kind: "openai_compatible".into(),
        credential_reference: None,
        is_default: true,
    }
}

fn conversation(id: &str) -> ConversationInput {
    ConversationInput {
        id: id.into(),
        title: format!("Conversation {id}"),
        provider_profile_id: Some("provider-local".into()),
    }
}

#[test]
fn full_state_survives_close_and_reopen() -> StorageResult<()> {
    let directory = TempDir::new()?;
    {
        let storage = Storage::open(directory.path())?;
        assert_eq!(storage.schema_version()?, CURRENT_SCHEMA_VERSION);
        storage.set_setting("appearance", &json!({"theme": "dark"}))?;
        storage.save_provider_profile(&provider())?;
        storage.create_conversation(&conversation("conversation-1"))?;

        let first = storage.append_message(&MessageInput {
            id: "message-1".into(),
            conversation_id: "conversation-1".into(),
            role: MessageRole::User,
            content: "Inspect the selected folder.".into(),
            metadata: json!({"source": "user"}),
        })?;
        let second = storage.append_message(&MessageInput {
            id: "message-2".into(),
            conversation_id: "conversation-1".into(),
            role: MessageRole::Assistant,
            content: "I need approval before reading a file.".into(),
            metadata: Value::Null,
        })?;
        assert_eq!((first.ordinal, second.ordinal), (0, 1));

        storage.create_task(&TaskInput {
            id: "task-1".into(),
            conversation_id: Some("conversation-1".into()),
            kind: "inspect_folder".into(),
            payload: json!({"selection": "user-selected"}),
        })?;
        storage.update_task_status("task-1", TaskStatus::Running, None, None)?;
        storage.request_task_cancellation("task-1")?;
        storage.update_task_status("task-1", TaskStatus::Cancelled, None, None)?;

        storage.create_proposed_action(&ProposedActionInput {
            id: "action-1".into(),
            conversation_id: "conversation-1".into(),
            task_id: Some("task-1".into()),
            tool_name: "read_text_file".into(),
            summary: "Read one selected text file".into(),
            request: json!({"selection_token": "opaque-user-selection"}),
        })?;
        storage.approve_action("action-1", Some("User approved this exact request"))?;
        storage.record_action_success("action-1", &json!({"bytes_read": 42}))?;
    }

    let reopened = Storage::open(directory.path())?;
    let appearance: Value = reopened
        .get_setting("appearance")?
        .expect("setting should persist");
    assert_eq!(appearance, json!({"theme": "dark"}));
    assert!(
        reopened
            .default_provider_profile()?
            .expect("default provider should persist")
            .is_default
    );
    let messages = reopened.list_messages("conversation-1")?;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].ordinal, 0);
    assert_eq!(messages[1].ordinal, 1);
    assert_eq!(
        reopened
            .get_task("task-1")?
            .expect("task should persist")
            .status,
        TaskStatus::Cancelled
    );
    let action = reopened
        .get_proposed_action("action-1")?
        .expect("action should persist");
    assert_eq!(action.status, ActionStatus::Succeeded);
    assert_eq!(reopened.list_action_audit("action-1")?.len(), 3);
    let export = reopened.export_all()?;
    assert_eq!(export.conversations.len(), 1);
    assert_eq!(export.conversations[0].messages.len(), 2);
    assert_eq!(export.tasks.len(), 1);
    assert_eq!(export.actions.len(), 1);
    Ok(())
}

#[test]
fn migration_is_idempotent_across_repeated_opens() -> StorageResult<()> {
    let directory = TempDir::new()?;
    let database_path = {
        let storage = Storage::open(directory.path())?;
        storage.set_setting("migration-marker", &"preserved")?;
        storage.close()?
    };
    {
        let storage = Storage::open(directory.path())?;
        assert_eq!(storage.schema_version()?, CURRENT_SCHEMA_VERSION);
        assert_eq!(
            storage.get_setting::<String>("migration-marker")?,
            Some("preserved".into())
        );
        storage.close()?;
    }

    let connection = Connection::open(database_path)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let tables: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('settings', 'provider_profiles', 'conversations', 'messages', 'tasks', 'proposed_actions', 'action_audit', 'crowquant_memories')",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
    assert_eq!(tables, 8);
    Ok(())
}

#[test]
fn crowquant_memory_survives_close_and_reopen() -> StorageResult<()> {
    let directory = TempDir::new()?;
    {
        let storage = Storage::open(directory.path())?;
        let created = storage.create_crowquant_memory(&CrowQuantMemoryInput {
            id: "memory-1".into(),
            text: "The quantum lab uses local compressed memory.".into(),
            block: vec![1, 2, 3, 4],
            format_version: 1,
            algorithm: "CrowQuant test".into(),
            dimension: 256,
            seed: 42,
            bits: 4,
            original_bytes: 2048,
        })?;
        assert_eq!(
            created.text,
            "The quantum lab uses local compressed memory."
        );
    }
    let reopened = Storage::open(directory.path())?;
    let memories = reopened.list_crowquant_memories()?;
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].block, vec![1, 2, 3, 4]);
    assert_eq!(memories[0].seed, 42);
    Ok(())
}

#[test]
fn failed_mutations_roll_back_without_corrupting_order_or_approval() -> StorageResult<()> {
    let directory = TempDir::new()?;
    let storage = Storage::open(directory.path())?;
    storage.save_provider_profile(&provider())?;
    storage.create_conversation(&conversation("conversation-1"))?;

    let message = MessageInput {
        id: "message-1".into(),
        conversation_id: "conversation-1".into(),
        role: MessageRole::User,
        content: "First".into(),
        metadata: Value::Null,
    };
    storage.append_message(&message)?;
    assert!(storage.append_message(&message).is_err());
    let next = storage.append_message(&MessageInput {
        id: "message-2".into(),
        content: "Second".into(),
        ..message
    })?;
    assert_eq!(next.ordinal, 1);

    storage.create_task(&TaskInput {
        id: "task-1".into(),
        conversation_id: Some("conversation-1".into()),
        kind: "test".into(),
        payload: Value::Null,
    })?;
    assert!(storage
        .update_task_status("task-1", TaskStatus::Succeeded, None, None)
        .is_err());
    assert_eq!(
        storage.get_task("task-1")?.expect("task remains").status,
        TaskStatus::Queued
    );

    storage.create_proposed_action(&ProposedActionInput {
        id: "action-1".into(),
        conversation_id: "conversation-1".into(),
        task_id: None,
        tool_name: "read_text_file".into(),
        summary: "Read a file".into(),
        request: json!({"token": "one"}),
    })?;
    assert!(storage
        .record_action_success("action-1", &json!({"unexpected": true}))
        .is_err());
    assert_eq!(
        storage
            .get_proposed_action("action-1")?
            .expect("action remains")
            .status,
        ActionStatus::Pending
    );
    assert_eq!(storage.list_action_audit("action-1")?.len(), 1);
    Ok(())
}

#[test]
fn retention_choice_preserves_or_removes_user_records_explicitly() -> StorageResult<()> {
    let directory = TempDir::new()?;
    let storage = Storage::open(directory.path())?;
    storage.set_setting("retain", &true)?;
    storage.save_provider_profile(&provider())?;
    storage.create_conversation(&conversation("conversation-1"))?;
    storage.append_message(&MessageInput {
        id: "message-1".into(),
        conversation_id: "conversation-1".into(),
        role: MessageRole::User,
        content: "Persist me".into(),
        metadata: Value::Null,
    })?;

    let preserved = storage.apply_retention_choice(RetentionChoice::Preserve)?;
    assert_eq!(preserved.records_before, preserved.records_after);
    assert!(preserved.records_after > 0);
    assert!(storage.get_setting::<bool>("retain")?.is_some());

    let removed = storage.apply_retention_choice(RetentionChoice::Remove)?;
    assert!(removed.records_before > 0);
    assert_eq!(removed.records_after, 0);
    assert_eq!(storage.stored_record_count()?, 0);
    assert!(storage.export_all()?.conversations.is_empty());
    Ok(())
}
