use base64::{engine::general_purpose::STANDARD, Engine};
use crowclaw_desktop_lib::storage::{
    attachments::{
        AttachmentInput, AttachmentKind, AttachmentRecord, MAX_ATTACHMENT_BYTES,
        MAX_TEXT_ATTACHMENT_BYTES,
    },
    composer::ConversationModelChoice,
    ConversationInput, MessageInput, MessageRole, ProviderProfileInput, RetentionChoice, Storage,
    StorageError, TaskInput, TaskStatus,
};
use crowclaw_desktop_lib::tools::GeneratedImage;
use rusqlite::Connection;

fn setup() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    storage
        .save_provider_profile(&ProviderProfileInput {
            id: "local".into(),
            name: "Local".into(),
            base_url: "http://127.0.0.1:1234/v1".into(),
            model: "test".into(),
            provider_kind: "lm-studio".into(),
            credential_reference: None,
            is_default: true,
        })
        .unwrap();
    for id in ["a", "b"] {
        storage
            .create_conversation(&ConversationInput {
                id: id.into(),
                title: id.into(),
                provider_profile_id: None,
            })
            .unwrap();
    }
    (dir, storage)
}

fn text(id: &str) -> AttachmentInput {
    AttachmentInput {
        id: id.into(),
        name: "note.txt".into(),
        media_type: "text/plain".into(),
        kind: AttachmentKind::Text,
        bytes: b"abc".to_vec(),
    }
}

fn turn() -> (MessageInput, TaskInput) {
    (
        MessageInput {
            id: "m".into(),
            conversation_id: "a".into(),
            role: MessageRole::User,
            content: String::new(),
            metadata: serde_json::json!({"attachments":"forged"}),
        },
        TaskInput {
            id: "t".into(),
            conversation_id: Some("a".into()),
            kind: "agent-turn".into(),
            payload: serde_json::json!({"attachmentIds":["forged"]}),
        },
    )
}

fn selected(storage: &Storage) -> crowclaw_desktop_lib::storage::ConversationComposer {
    let c = storage.conversation_composer("a").unwrap();
    storage
        .save_conversation_composer(
            "a",
            c.revision,
            "draft",
            Some(&ConversationModelChoice {
                provider_profile_id: "local".into(),
                model: "test".into(),
                reasoning_effort: None,
            }),
        )
        .unwrap()
}

#[test]
fn snapshots_are_durable_scoped_and_computed_from_bytes() {
    let (dir, storage) = setup();
    let mut input = text("one");
    let c = storage
        .add_composer_attachments("a", 0, &[input.clone()])
        .unwrap();
    assert_eq!(c.revision, 1);
    assert_eq!(c.attachments[0].byte_length, 3);
    assert_eq!(
        c.attachments[0].sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    input.bytes.fill(b'z');
    assert!(storage.attachment("b", "one").is_err());
    storage.add_composer_attachments("b", 0, &[input]).unwrap();
    drop(storage);
    let reopened = Storage::open(dir.path()).unwrap();
    assert_eq!(reopened.conversation_composer("a").unwrap(), c);
    let record = reopened.attachment("a", "one").unwrap();
    assert_eq!(record.bytes, b"abc");
    assert!(!format!("{record:?}").contains("bytes"));
    assert_eq!(reopened.attachment("b", "one").unwrap().bytes, b"zzz");
    let json = serde_json::to_value(&c).unwrap();
    assert_eq!(json["attachments"][0]["kind"], "text");
    assert_eq!(json["attachments"][0]["conversationId"], "a");
    assert!(json["attachments"][0].get("path").is_none());
    let mut legacy = json;
    legacy.as_object_mut().unwrap().remove("attachments");
    let legacy: crowclaw_desktop_lib::storage::ConversationComposer =
        serde_json::from_value(legacy).unwrap();
    assert!(legacy.attachments.is_empty());
}

#[test]
fn generated_image_is_bound_atomically_to_its_assistant_message() {
    for next_status in [TaskStatus::Succeeded, TaskStatus::Failed] {
        let (_dir, storage) = setup();
        storage
            .create_task(&TaskInput {
                id: "image-task".into(),
                conversation_id: Some("a".into()),
                kind: "agent-turn".into(),
                payload: serde_json::json!({}),
            })
            .unwrap();
        storage
            .update_task_status("image-task", TaskStatus::Running, None, None)
            .unwrap();
        let message = MessageInput {
            id: "image-answer".into(),
            conversation_id: "a".into(),
            role: MessageRole::Assistant,
            content: "Generated it.".into(),
            metadata: serde_json::json!({"reportedModel":"gpt-6-luna"}),
        };
        let image=GeneratedImage{id:"generated-image".into(),name:"crowclaw-image-test.png".into(),media_type:"image/png".into(),model:"gpt-image-2".into(),bytes:STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/XioAAAAASUVORK5CYII=").unwrap()};
        storage
            .finish_task_with_generated_images(
                "image-task",
                next_status,
                Some(&serde_json::json!({})),
                if next_status == TaskStatus::Failed {
                    Some("follow-up unavailable")
                } else {
                    None
                },
                Some(&message),
                &[AttachmentInput {
                    id: image.id.clone(),
                    name: image.name.clone(),
                    media_type: image.media_type.clone(),
                    kind: AttachmentKind::Image,
                    bytes: image.bytes.clone(),
                }],
            )
            .unwrap();
        let saved = storage
            .list_messages("a")
            .unwrap()
            .into_iter()
            .find(|m| m.id == "image-answer")
            .unwrap();
        let summary: Vec<crowclaw_desktop_lib::storage::attachments::AttachmentSummary> =
            serde_json::from_value(saved.metadata["attachments"].clone()).unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].message_id.as_deref(), Some("image-answer"));
        let retained = storage.message_attachments("a", "image-answer").unwrap();
        assert_eq!(retained[0].bytes, image.bytes);
        assert_eq!(retained[0].summary, summary[0]);
    }
}

#[test]
fn revision_cas_and_selection_failures_leave_no_partial_rows() {
    let (dir, storage) = setup();
    let other = Storage::open(dir.path()).unwrap();
    let c = storage
        .add_composer_attachments("a", 0, &[text("one")])
        .unwrap();
    assert!(matches!(
        other.add_composer_attachments("a", 0, &[text("late")]),
        Err(StorageError::Conflict(_))
    ));
    assert!(other.attachment("a", "late").is_err());
    assert!(other.remove_composer_attachment("a", 0, "one").is_err());
    assert!(other
        .save_conversation_composer("a", 0, "late", None)
        .is_err());
    assert!(storage
        .add_composer_attachments("a", 1, &[text("new"), text("one")])
        .is_err());
    assert!(storage.attachment("a", "new").is_err());
    assert_eq!(storage.conversation_composer("a").unwrap(), c);
    assert!(storage.remove_composer_attachment("b", 0, "one").is_err());
    let next = storage.remove_composer_attachment("a", 1, "one").unwrap();
    assert_eq!(next.revision, 2);
    assert!(next.attachments.is_empty());
    assert!(storage.attachment("a", "one").is_err());
}

#[test]
fn invalid_names_types_text_and_sizes_roll_back_whole_batch() {
    let (_dir, storage) = setup();
    let mut invalid = Vec::new();
    for name in [
        "",
        " ",
        "..",
        "../note",
        "dir\\note",
        "C:note",
        "bad\nname",
        "bad\0name",
    ] {
        let mut input = text("bad");
        input.name = name.into();
        invalid.push(input);
    }
    for bytes in [
        vec![0],
        vec![255],
        vec![b'x'; MAX_TEXT_ATTACHMENT_BYTES + 1],
    ] {
        let mut input = text("bad");
        input.bytes = bytes;
        invalid.push(input);
    }
    for (kind, mime) in [
        (AttachmentKind::Text, "image/png"),
        (AttachmentKind::Image, "text/plain"),
        (AttachmentKind::File, "application/octet-stream"),
        (AttachmentKind::File, "application/vnd.ms-excel"),
        (AttachmentKind::Image, "image/svg+xml"),
    ] {
        let mut input = text("bad");
        input.kind = kind;
        input.media_type = mime.into();
        invalid.push(input);
    }
    let mut large = text("bad");
    large.kind = AttachmentKind::File;
    large.media_type = "application/pdf".into();
    large.bytes = vec![1; MAX_ATTACHMENT_BYTES + 1];
    invalid.push(large);
    for bad in invalid {
        assert!(storage
            .add_composer_attachments("a", 0, &[text("good"), bad])
            .is_err());
        assert!(storage.attachment("a", "good").is_err());
        assert_eq!(storage.conversation_composer("a").unwrap().revision, 0);
    }
    let mut max_text = text("max");
    max_text.bytes = vec![b'x'; MAX_TEXT_ATTACHMENT_BYTES];
    storage
        .add_composer_attachments("a", 0, &[max_text])
        .unwrap();
}

#[test]
fn count_and_aggregate_byte_limits_include_existing_drafts() {
    let (_dir, storage) = setup();
    let inputs: Vec<_> = (0..8).map(|i| text(&format!("{i}"))).collect();
    let c = storage.add_composer_attachments("a", 0, &inputs).unwrap();
    assert!(storage
        .add_composer_attachments("a", c.revision, &[text("ninth")])
        .is_err());
    let mut file = text("big1");
    file.kind = AttachmentKind::File;
    file.media_type = "application/pdf".into();
    file.bytes = vec![1; MAX_ATTACHMENT_BYTES];
    let mut second = file.clone();
    second.id = "big2".into();
    storage.add_composer_attachments("b", 0, &[file]).unwrap();
    assert!(storage
        .add_composer_attachments("b", 1, &[second.clone(), text("overflow")])
        .is_err());
    assert!(storage.attachment("b", "big2").is_err());
    let full = storage.add_composer_attachments("b", 1, &[second]).unwrap();
    assert_eq!(
        full.attachments.iter().map(|a| a.byte_length).sum::<u64>(),
        40 * 1024 * 1024
    );
}

#[test]
fn send_freezes_exact_snapshots_and_exports_bytes_and_metadata() {
    let (_dir, storage) = setup();
    let choice = selected(&storage);
    let c = storage
        .add_composer_attachments("a", choice.revision, &[text("one"), text("two")])
        .unwrap();
    let (message, task) = turn();
    let mut forged = c.clone();
    forged.attachments.pop();
    assert!(matches!(
        storage.begin_composer_turn(&forged, &message, &task),
        Err(StorageError::Conflict(_))
    ));
    let (message, task, next) = storage.begin_composer_turn(&c, &message, &task).unwrap();
    assert!(message.content.is_empty());
    assert!(next.attachments.is_empty());
    assert!(next.draft.is_empty());
    assert_eq!(next.selection, c.selection);
    assert_eq!(next.revision, c.revision + 1);
    assert_eq!(
        task.payload["attachmentIds"],
        serde_json::json!(["one", "two"])
    );
    assert_eq!(message.metadata["attachments"][0]["messageId"], "m");
    let records = storage.message_attachments("a", "m").unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].bytes, b"abc");
    assert!(storage.message_attachments("b", "m").unwrap().is_empty());
    assert!(storage
        .remove_composer_attachment("a", next.revision, "one")
        .is_err());
    assert!(storage
        .add_composer_attachments("a", next.revision, &[text("one")])
        .is_err());
    storage
        .add_composer_attachments("a", next.revision, &[text("draft")])
        .unwrap();
    let export = serde_json::to_value(storage.export_all().unwrap()).unwrap();
    let chat = export["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["conversation"]["id"] == "a")
        .unwrap();
    let exported: Vec<AttachmentRecord> =
        serde_json::from_value(chat["attachments"].clone()).unwrap();
    assert_eq!(exported.len(), 3);
    assert!(exported.iter().any(|r| r == &records[0]));
    assert_eq!(chat["composer"]["attachments"].as_array().unwrap().len(), 1);
    let raw = Connection::open(storage.database_path()).unwrap();
    assert!(raw
        .execute(
            "UPDATE conversation_attachments SET bytes=x'646566' WHERE id='one'",
            []
        )
        .is_err());
}

#[test]
fn late_send_and_edit_failures_rollback_message_task_links_and_revision() {
    let (_dir, storage) = setup();
    let choice = selected(&storage);
    let c = storage
        .add_composer_attachments("a", choice.revision, &[text("one")])
        .unwrap();
    let raw = Connection::open(storage.database_path()).unwrap();
    raw.execute_batch("CREATE TRIGGER fail_revision BEFORE UPDATE ON conversation_composers BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    let (message, task) = turn();
    assert!(storage.begin_composer_turn(&c, &message, &task).is_err());
    assert!(storage.list_messages("a").unwrap().is_empty());
    assert!(storage.export_all().unwrap().tasks.is_empty());
    assert_eq!(storage.conversation_composer("a").unwrap(), c);
    assert_eq!(
        storage.attachment("a", "one").unwrap().summary.message_id,
        None
    );
    assert!(storage
        .add_composer_attachments("a", c.revision, &[text("new")])
        .is_err());
    assert!(storage.attachment("a", "new").is_err());
    assert!(storage
        .remove_composer_attachment("a", c.revision, "one")
        .is_err());
    assert_eq!(storage.conversation_composer("a").unwrap(), c);
    raw.execute_batch("DROP TRIGGER fail_revision;").unwrap();
    storage.begin_composer_turn(&c, &message, &task).unwrap();
}

#[test]
fn deletion_cascades_and_full_retention_counts_and_removes_snapshots() {
    let (_dir, storage) = setup();
    let choice = selected(&storage);
    let before = storage.stored_record_count().unwrap();
    let c = storage
        .add_composer_attachments("a", choice.revision, &[text("one")])
        .unwrap();
    assert_eq!(storage.stored_record_count().unwrap(), before + 1);
    let (message, task) = turn();
    storage.begin_composer_turn(&c, &message, &task).unwrap();
    let raw = Connection::open(storage.database_path()).unwrap();
    raw.execute_batch("PRAGMA foreign_keys=ON; DELETE FROM messages WHERE id='m';")
        .unwrap();
    assert!(storage.attachment("a", "one").is_err());
    let c = storage.conversation_composer("a").unwrap();
    storage
        .add_composer_attachments("a", c.revision, &[text("draft")])
        .unwrap();
    storage.delete_conversation("a").unwrap();
    assert!(storage.attachment("a", "draft").is_err());
    storage
        .add_composer_attachments("b", 0, &[text("kept")])
        .unwrap();
    let report = storage
        .apply_retention_choice(RetentionChoice::Preserve)
        .unwrap();
    assert_eq!(report.records_before, report.records_after);
    assert_eq!(storage.attachment("b", "kept").unwrap().bytes, b"abc");
    assert_eq!(
        storage
            .apply_retention_choice(RetentionChoice::Remove)
            .unwrap()
            .records_after,
        0
    );
    assert_eq!(
        raw.query_row("SELECT COUNT(*) FROM conversation_attachments", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}

#[test]
fn schema_nine_upgrade_preserves_existing_draft_and_message() {
    let (dir, storage) = setup();
    let c = selected(&storage);
    let (mut message, _) = turn();
    message.content = "existing".into();
    storage.append_message(&message).unwrap();
    let path = storage.close().unwrap();
    let raw = Connection::open(path).unwrap();
    raw.execute_batch("DROP TABLE crowbot_credentials; DROP TABLE openrouter_credentials; DROP TABLE conversation_attachments; DROP INDEX messages_attachment_owner; PRAGMA user_version=9;").unwrap();
    drop(raw);
    let upgraded = Storage::open(dir.path()).unwrap();
    assert_eq!(
        upgraded.schema_version().unwrap(),
        crowclaw_desktop_lib::storage::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(upgraded.conversation_composer("a").unwrap(), c);
    assert_eq!(upgraded.list_messages("a").unwrap()[0].content, "existing");
    upgraded
        .add_composer_attachments("a", c.revision, &[text("new")])
        .unwrap();
}

#[test]
fn concurrent_windows_allow_exactly_one_attachment_revision_winner() {
    let (dir, storage) = setup();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|id| {
            let window = Storage::open(dir.path()).unwrap();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                window.add_composer_attachments("a", 0, &[text(id)])
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(StorageError::Conflict(_))))
            .count(),
        1
    );
    let current = storage.conversation_composer("a").unwrap();
    assert_eq!(current.revision, 1);
    assert_eq!(current.attachments.len(), 1);
    assert_eq!(
        storage
            .export_all()
            .unwrap()
            .conversations
            .iter()
            .find(|c| c.conversation.id == "a")
            .unwrap()
            .attachments
            .len(),
        1
    );
}

#[test]
fn approved_types_are_preserved_and_sql_rejects_cross_conversation_linking() {
    let (_dir, storage) = setup();
    for (i, (kind, media_type)) in [
        (AttachmentKind::Text, "text/plain"),
        (AttachmentKind::Text, "text/markdown"),
        (AttachmentKind::Text, "application/json"),
        (AttachmentKind::Image, "image/png"),
        (AttachmentKind::Image, "image/jpeg"),
        (AttachmentKind::Image, "image/webp"),
        (AttachmentKind::Image, "image/gif"),
        (AttachmentKind::File, "application/pdf"),
        (
            AttachmentKind::File,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ),
        (
            AttachmentKind::File,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
        (
            AttachmentKind::File,
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut input = text("type-test");
        input.kind = kind;
        input.media_type = media_type.into();
        // Native file decoding is the caller's boundary. Storage validates the
        // allowlisted media-type/kind pair and independently bounds the bytes.
        let added = storage
            .add_composer_attachments("a", (i * 2) as u32, &[input])
            .unwrap();
        assert_eq!(added.attachments[0].kind, kind);
        assert_eq!(added.attachments[0].media_type, media_type);
        storage
            .remove_composer_attachment("a", added.revision, "type-test")
            .unwrap();
    }
    storage
        .add_composer_attachments("b", 0, &[text("other")])
        .unwrap();
    let (mut message, _) = turn();
    message.content = "message owned by a".into();
    storage.append_message(&message).unwrap();
    let raw = Connection::open(storage.database_path()).unwrap();
    raw.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    assert!(raw
        .execute(
            "UPDATE conversation_attachments SET message_id='m' WHERE conversation_id='b'",
            []
        )
        .is_err());
    assert_eq!(
        storage.attachment("b", "other").unwrap().summary.message_id,
        None
    );
}
