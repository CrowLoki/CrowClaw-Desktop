use std::sync::Arc;

use crowclaw_desktop_lib::{
    agent::CancellationToken,
    memory::{MemoryQuery, MemoryService, MemorySettings, SearchMode},
    storage::{ConversationInput, MessageInput, MessageRole, RetentionChoice, Storage},
};
use serde_json::Value;
use tempfile::TempDir;

const RESTORE_ALPHA_TWO:&str="DROP TABLE memory_vectors; DROP TABLE memory_embedding_profiles; DROP TRIGGER memory_message_insert; DROP TRIGGER memory_message_update; DROP TRIGGER memory_message_delete; DROP TRIGGER memory_conversation_title; DROP TRIGGER memory_crowquant_insert; DROP TRIGGER memory_crowquant_delete; DROP TRIGGER memory_action_success; DROP TRIGGER memory_action_delete; DROP TRIGGER memory_chunk_insert; DROP TRIGGER memory_chunk_delete; DROP TRIGGER memory_chunk_update; DROP TABLE memory_fts; DROP TABLE memory_chunks; DROP TABLE memory_sources; DROP TABLE memory_exclusions; DROP TABLE memory_jobs; DROP TABLE memory_job_errors; DELETE FROM settings WHERE key='memory_settings'; PRAGMA user_version=2;";

fn open() -> (TempDir, Arc<Storage>, MemoryService) {
    let dir = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(dir.path()).unwrap());
    let service = MemoryService::new(storage.clone());
    service.configure(MemorySettings::default()).unwrap();
    (dir, storage, service)
}

fn remove_evolution_fixture_schema(connection: &rusqlite::Connection) {
    // These disposable fixtures model older installed schemas, where the
    // later evolution/account tables did not exist. Canonical records stay intact.
    connection.execute_batch("DROP TABLE crowbot_credentials; DROP TABLE openrouter_credentials; DROP TABLE conversation_attachments; DROP INDEX messages_attachment_owner; DROP TABLE conversation_composers; DROP TABLE membership_accounts; DROP TABLE membership_host; DROP TABLE evolution_evaluations; DROP TABLE evolution_proposals; DROP TABLE evolution_feedback; DROP TABLE evolution_head; DROP TABLE evolution_revisions;").unwrap();
}

fn message(storage: &Storage, id: &str, role: MessageRole, text: &str) {
    if storage.get_conversation("conversation").unwrap().is_none() {
        storage
            .create_conversation(&ConversationInput {
                id: "conversation".into(),
                title: "Telescope project".into(),
                provider_profile_id: None,
            })
            .unwrap();
    }
    storage
        .append_message(&MessageInput {
            id: id.into(),
            conversation_id: "conversation".into(),
            role,
            content: text.into(),
            metadata: Value::Null,
        })
        .unwrap();
}

fn query(text: &str) -> MemoryQuery {
    MemoryQuery {
        query: text.into(),
        limit: 5,
        source_kind: None,
        mode: SearchMode::Hybrid,
    }
}

#[test]
fn status_counts_sources_once_and_only_counts_enabled_active_chunks() {
    let (_dir, storage, service) = open();
    message(&storage, "long", MessageRole::User, &"a".repeat(3500));
    message(
        &storage,
        "short",
        MessageRole::Assistant,
        "short telescope note",
    );
    let note = service.remember("retained quartz note").unwrap();
    service.sync(64, &CancellationToken::new()).unwrap();
    let status = service.status().unwrap();
    assert_eq!((status.active_sources, status.chunks), (3, 4));

    service
        .configure(MemorySettings {
            index_conversations: Some(false),
            ..MemorySettings::default()
        })
        .unwrap();
    let status = service.status().unwrap();
    assert_eq!((status.active_sources, status.chunks), (1, 1));
    let hit = service
        .search(&query("quartz"), &CancellationToken::new())
        .unwrap()
        .hits
        .remove(0);
    service.withdraw(&hit.source_id).unwrap();
    let status = service.status().unwrap();
    assert_eq!((status.active_sources, status.chunks), (0, 0));
    assert!(storage.get_crowquant_memory(&note.id).unwrap().is_some());
    assert_eq!(storage.list_messages("conversation").unwrap().len(), 2);
}

#[test]
fn schema_four_one_sided_note_withdrawal_is_repaired_before_any_search() {
    for withdrawn_kind in ["user_note", "legacy_crowquant"] {
        let (dir, storage, service) = open();
        let note = service
            .remember("previously withdrawn sapphire note")
            .unwrap();
        let native = storage.memory_sources().unwrap().remove(0);
        let connection = rusqlite::Connection::open(storage.database_path()).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys=ON; DROP TRIGGER IF EXISTS memory_note_exclusion;")
            .unwrap();
        connection.execute("INSERT INTO memory_sources(id,logical_key,source_kind,origin_id,title,authorship,content_hash,snapshot,state,predecessor_id,created_at_ms,updated_at_ms) SELECT 'old-legacy-alias','legacy_crowquant:'||origin_id,'legacy_crowquant',origin_id,title,authorship,content_hash,snapshot,'active',NULL,created_at_ms,updated_at_ms FROM memory_sources WHERE id=?1",[&native.id]).unwrap();
        connection.execute("INSERT INTO memory_chunks(id,source_id,ordinal,text,start_byte,end_byte,content_hash,lexical_block,title) SELECT 'old-legacy-chunk','old-legacy-alias',ordinal,text,start_byte,end_byte,content_hash,lexical_block,title FROM memory_chunks WHERE source_id=?1",[&native.id]).unwrap();
        connection.execute_batch("INSERT INTO memory_embedding_profiles VALUES('old-profile','{}',0); INSERT INTO memory_vectors SELECT id,'old-profile',2,'f32le-normalized-v1',X'0000803F00000000',content_hash,0 FROM memory_chunks;").unwrap();
        let withdrawn_id = if withdrawn_kind == "user_note" {
            native.id.clone()
        } else {
            "old-legacy-alias".into()
        };
        connection
            .execute(
                "INSERT INTO memory_exclusions VALUES(?1)",
                [format!("{withdrawn_kind}:{}", note.id)],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM memory_chunks WHERE source_id=?1",
                [&withdrawn_id],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE memory_sources SET state='withdrawn' WHERE id=?1",
                [&withdrawn_id],
            )
            .unwrap();
        connection
            .execute_batch("DELETE FROM memory_jobs; PRAGMA user_version=4;")
            .unwrap();
        remove_evolution_fixture_schema(&connection);
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM memory_vectors", [], |row| row
                    .get::<_, u32>(0))
                .unwrap(),
            1
        );
        drop(connection);
        drop(service);
        drop(storage);
        let storage = Arc::new(Storage::open(dir.path()).unwrap());
        let service = MemoryService::new(storage.clone());
        assert!(
            service
                .search(&query("sapphire"), &CancellationToken::new())
                .unwrap()
                .hits
                .is_empty(),
            "old {withdrawn_kind} tombstone must exclude its sibling immediately"
        );
        assert!(storage.export_all().unwrap().memory_vectors.is_empty());
        assert_eq!(
            storage.get_crowquant_memory(&note.id).unwrap().unwrap(),
            note
        );
        assert_eq!(service.status().unwrap().active_sources, 0);
    }
}

#[test]
fn canonical_note_identity_commits_atomically_with_its_content() {
    use crowclaw_desktop_lib::storage::CrowQuantMemoryInput;
    let (_dir, storage, service) = open();
    let template = service.remember("transactional sapphire note").unwrap();
    let input = CrowQuantMemoryInput {
        id: "new-native-note".into(),
        text: template.text,
        block: template.block,
        format_version: template.format_version,
        algorithm: template.algorithm,
        dimension: template.dimension,
        seed: template.seed,
        bits: template.bits,
        original_bytes: template.original_bytes,
    };
    storage.create_crowquant_memory(&input).unwrap();
    assert_eq!(
        storage
            .memory_candidate("legacy_crowquant", &input.id)
            .unwrap()
            .unwrap()
            .source_kind,
        "user_note"
    );
    rusqlite::Connection::open(storage.database_path()).unwrap().execute_batch("CREATE TRIGGER fail_note_identity BEFORE INSERT ON memory_sources BEGIN SELECT RAISE(ABORT,'forced admission failure'); END;").unwrap();
    let failed = CrowQuantMemoryInput {
        id: "failed-native-note".into(),
        ..input
    };
    assert!(storage.create_crowquant_memory(&failed).is_err());
    assert!(storage.get_crowquant_memory(&failed.id).unwrap().is_none());
}

#[test]
fn search_does_not_deserialize_snapshots_from_unrelated_retained_history() {
    let (_dir, storage, service) = open();
    service.remember("current violet telescope").unwrap();
    // A corrupt withdrawn snapshot is not a search input. Loading it anyway
    // reproduces the same unbounded historical-body query as large valid files.
    rusqlite::Connection::open(storage.database_path()).unwrap().execute_batch("INSERT INTO memory_sources(id,logical_key,source_kind,origin_id,title,authorship,content_hash,snapshot,state,created_at_ms,updated_at_ms) VALUES('old-file','approved_file:old','approved_file','old','Withdrawn file','tool','old',X'FF','withdrawn',0,0);").unwrap();
    let result = service
        .search(&query("violet"), &CancellationToken::new())
        .expect("search must not deserialize unrelated snapshot bodies");
    assert_eq!(result.hits[0].text, "current violet telescope");
}

#[test]
fn stale_note_classification_cannot_create_an_alternate_withdrawable_identity() {
    let (_dir, storage, service) = open();
    let note = service.remember("singular cobalt origin").unwrap();
    let original = storage.memory_sources().unwrap().remove(0);
    let mut stale = original.clone();
    stale.id = "stale-legacy-candidate".into();
    stale.source_kind = "legacy_crowquant".into();
    stale.logical_key = format!("legacy_crowquant:{}", note.id);
    let mut chunks = storage.export_all().unwrap().memory_chunks;
    for chunk in &mut chunks {
        chunk.id = format!("stale-{}", chunk.id);
        chunk.source_id = stale.id.clone();
    }
    // The background reader can classify a queued note before direct admission
    // completes. Its delayed writer must not install a second logical origin.
    assert!(!storage.memory_store_revision(&stale, &chunks).unwrap());
    assert_eq!(service.status().unwrap().active_sources, 1);
    service.withdraw(&original.id).unwrap();
    service.rebuild(&CancellationToken::new()).unwrap();
    assert!(service
        .search(&query("cobalt"), &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
    assert!(storage.get_crowquant_memory(&note.id).unwrap().is_some());
}

#[test]
fn native_note_promotion_retires_the_legacy_alias_and_withdraws_both_keys() {
    let (_dir, storage, service) = open();
    let note = service.remember("promoted quartz origin").unwrap();
    // Alpha 3 had the canonical row but no native source metadata.
    rusqlite::Connection::open(storage.database_path()).unwrap().execute_batch("DELETE FROM memory_chunks; DELETE FROM memory_sources; INSERT OR IGNORE INTO memory_jobs SELECT 'legacy_crowquant',id FROM crowquant_memories;").unwrap();
    service.sync(64, &CancellationToken::new()).unwrap();
    let legacy = storage.memory_sources().unwrap().remove(0);
    let mut native = legacy.clone();
    native.id = "native-note-candidate".into();
    native.source_kind = "user_note".into();
    native.logical_key = format!("user_note:{}", note.id);
    let mut chunks = storage.export_all().unwrap().memory_chunks;
    for chunk in &mut chunks {
        chunk.id = format!("native-{}", chunk.id);
        chunk.source_id = native.id.clone();
    }
    storage.memory_store_revision(&native, &chunks).unwrap();
    assert_eq!(service.status().unwrap().active_sources, 1);
    // Even an older result bound to the legacy source must withdraw the origin.
    service.withdraw(&legacy.id).unwrap();
    service.rebuild(&CancellationToken::new()).unwrap();
    assert!(service
        .search(&query("quartz"), &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
    assert_eq!(service.status().unwrap().pending, 0);
}

#[test]
fn own_conversations_are_searchable_offline_with_authorship() {
    let (_dir, storage, service) = open();
    message(
        &storage,
        "user",
        MessageRole::User,
        "I calibrated the telescope mirror in September.",
    );
    message(
        &storage,
        "assistant",
        MessageRole::Assistant,
        "My suggested telescope alignment is unverified.",
    );
    let report = service.sync(64, &CancellationToken::new()).unwrap();
    assert_eq!(report.indexed, 2);
    let result = service
        .search(&query("telescope"), &CancellationToken::new())
        .unwrap();
    assert_eq!(result.hits.len(), 2);
    assert!(result
        .hits
        .iter()
        .any(|h| h.origin_id == "user" && h.authorship == "user"));
    assert!(result
        .hits
        .iter()
        .any(|h| h.origin_id == "assistant" && h.authorship == "assistant"));
    assert!(result
        .hits
        .iter()
        .all(|h| h.channels.iter().any(|c| c.channel == "full_text")));
}

#[test]
fn upgrade_waits_for_index_choice_without_affecting_originals() {
    let dir = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(dir.path()).unwrap());
    message(
        &storage,
        "old",
        MessageRole::User,
        "existing conversation sentinel",
    );
    let service = MemoryService::new(storage.clone());
    assert_eq!(service.settings().unwrap().index_conversations, None);
    service.sync(64, &CancellationToken::new()).unwrap();
    assert!(service
        .search(&query("sentinel"), &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
    service.configure(MemorySettings::default()).unwrap();
    service.sync(64, &CancellationToken::new()).unwrap();
    assert_eq!(
        service
            .search(&query("sentinel"), &CancellationToken::new())
            .unwrap()
            .hits
            .len(),
        1
    );
    assert_eq!(storage.list_messages("conversation").unwrap().len(), 1);
}

#[test]
fn withdrawal_survives_rebuild_and_restart_without_deleting_source() {
    let (dir, storage, service) = open();
    message(
        &storage,
        "forget",
        MessageRole::User,
        "private sapphire sentinel",
    );
    service.sync(64, &CancellationToken::new()).unwrap();
    let hit = service
        .search(&query("sapphire"), &CancellationToken::new())
        .unwrap()
        .hits
        .remove(0);
    service.withdraw(&hit.source_id).unwrap();
    service.rebuild(&CancellationToken::new()).unwrap();
    assert!(service
        .search(&query("sapphire"), &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
    assert_eq!(storage.list_messages("conversation").unwrap().len(), 1);
    drop(service);
    drop(storage);
    let reopened = MemoryService::new(Arc::new(Storage::open(dir.path()).unwrap()));
    reopened.sync(64, &CancellationToken::new()).unwrap();
    assert!(reopened
        .search(&query("sapphire"), &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
}

#[test]
fn deleting_conversation_removes_derived_search_text() {
    let (_dir, storage, service) = open();
    message(
        &storage,
        "delete",
        MessageRole::User,
        "deleted amber sentinel",
    );
    service.sync(64, &CancellationToken::new()).unwrap();
    storage.delete_conversation("conversation").unwrap();
    assert!(service
        .search(&query("amber"), &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
    assert!(!serde_json::to_string(&storage.export_all().unwrap())
        .unwrap()
        .contains("deleted amber sentinel"));
}

#[test]
fn legacy_crowquant_bytes_and_ids_are_preserved() {
    let (_dir, storage, service) = open();
    let old = service.remember("legacy telescope calibration").unwrap();
    let before = storage.get_crowquant_memory(&old.id).unwrap().unwrap();
    service.sync(64, &CancellationToken::new()).unwrap();
    service.rebuild(&CancellationToken::new()).unwrap();
    assert_eq!(
        storage.get_crowquant_memory(&old.id).unwrap().unwrap(),
        before
    );
    let hits = service
        .search(&query("telescope calibration"), &CancellationToken::new())
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].origin_id, old.id);
}

#[test]
fn withdrawing_new_note_does_not_leave_a_rebuild_job_spinning() {
    let (_dir, storage, service) = open();
    let note = service.remember("withdrawn jade note").unwrap();
    let source = service
        .search(&query("jade"), &CancellationToken::new())
        .unwrap()
        .hits
        .remove(0)
        .source_id;
    service.withdraw(&source).unwrap();
    let report = service.rebuild(&CancellationToken::new()).unwrap();
    assert_eq!(report.pending, 0);
    assert!(service
        .search(&query("jade"), &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
    assert!(storage.get_crowquant_memory(&note.id).unwrap().is_some());
}

#[test]
fn symbol_only_unicode_is_indexed_and_bounds_are_explicit() {
    let (_dir, storage, service) = open();
    message(&storage, "symbols", MessageRole::User, "🌒 🐦‍⬛ ∑ → ∞");
    service.sync(64, &CancellationToken::new()).unwrap();
    assert_eq!(service.status().unwrap().active_sources, 1);
    assert_eq!(
        service
            .search(&query("🌒"), &CancellationToken::new())
            .unwrap()
            .hits
            .len(),
        1
    );
    assert!(service
        .search(&query(&"x".repeat(4097)), &CancellationToken::new())
        .is_err());
    let mut invalid = query("test");
    invalid.limit = 21;
    assert!(service.search(&invalid, &CancellationToken::new()).is_err());
}

#[test]
fn cancellation_is_checked_before_any_index_mutation() {
    let (_dir, storage, service) = open();
    message(
        &storage,
        "cancel",
        MessageRole::User,
        "cancelled indexing sentinel",
    );
    let token = CancellationToken::new();
    token.cancel();
    assert!(service.sync(64, &token).is_err());
    assert_eq!(service.status().unwrap().active_sources, 0);
    assert!(service.search(&query("sentinel"), &token).is_err());
}

#[test]
fn export_and_remove_cover_native_memory_records() {
    let (_dir, storage, service) = open();
    service.remember("retained violet note").unwrap();
    service.sync(64, &CancellationToken::new()).unwrap();
    let export = storage.export_all().unwrap();
    assert!(!export.memory_sources.is_empty());
    assert!(!export.memory_chunks.is_empty());
    storage
        .apply_retention_choice(RetentionChoice::Remove)
        .unwrap();
    assert_eq!(service.status().unwrap().active_sources, 0);
    assert!(service
        .search(&query("violet"), &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
    assert_eq!(storage.stored_record_count().unwrap(), 0);
}

#[test]
fn batch_limit_leaves_visible_pending_work_and_reconcile_is_idempotent() {
    let (_dir, storage, service) = open();
    for index in 0..70 {
        message(
            &storage,
            &format!("m-{index}"),
            MessageRole::User,
            &format!("observation number {index}"),
        );
    }
    let first = service.sync(64, &CancellationToken::new()).unwrap();
    assert_eq!(first.indexed, 64);
    assert_eq!(first.pending, 6);
    let second = service.sync(64, &CancellationToken::new()).unwrap();
    assert_eq!(second.indexed, 6);
    assert_eq!(second.pending, 0);
    assert_eq!(
        service.sync(64, &CancellationToken::new()).unwrap().indexed,
        0
    );
    assert_eq!(service.status().unwrap().active_sources, 70);
}

#[tokio::test]
async fn actual_agent_search_requires_approval_and_returns_source_provenance() {
    use crowclaw_desktop_lib::tools::{
        ApprovalDecision, ToolExecution, ToolExecutor, ToolOutput, ToolPolicy, ToolRequest,
    };
    let (_dir, storage, service) = open();
    message(
        &storage,
        "origin-user",
        MessageRole::User,
        "telescope prism alignment",
    );
    service.sync(64, &CancellationToken::new()).unwrap();
    let executor = ToolExecutor::new(ToolPolicy::default())
        .unwrap()
        .with_memory_backend(Arc::new(service));
    let denied = executor
        .propose(ToolRequest::SearchMemory {
            query: "prism".into(),
            limit: 5,
        })
        .unwrap();
    assert!(executor
        .execute(&denied.approval_token, &CancellationToken::new())
        .await
        .is_err());
    executor
        .resolve(
            &denied.approval_token,
            ApprovalDecision::Deny { reason: None },
        )
        .unwrap();
    let denied_result = executor
        .execute(&denied.approval_token, &CancellationToken::new())
        .await
        .unwrap();
    assert!(matches!(denied_result, ToolExecution::Denied { .. }));
    assert!(!serde_json::to_string(&denied_result)
        .unwrap()
        .contains("telescope prism alignment"));
    let approved = executor
        .propose(ToolRequest::SearchMemory {
            query: "prism".into(),
            limit: 5,
        })
        .unwrap();
    executor
        .resolve(&approved.approval_token, ApprovalDecision::Approve)
        .unwrap();
    let result = executor
        .execute(&approved.approval_token, &CancellationToken::new())
        .await
        .unwrap();
    match result {
        ToolExecution::Executed {
            output: ToolOutput::MemorySearch { results, .. },
            ..
        } => {
            assert_eq!(results.len(), 1);
            assert_eq!(
                results[0].provenance.as_ref().unwrap()["originId"],
                "origin-user"
            );
            assert_eq!(
                results[0].provenance.as_ref().unwrap()["authorship"],
                "user"
            );
            assert_eq!(results[0].text, "telescope prism alignment");
        }
        other => panic!("Unexpected execution: {other:?}"),
    }
    assert!(executor
        .execute(&approved.approval_token, &CancellationToken::new())
        .await
        .is_err());
}

#[test]
fn updated_source_supersedes_previous_revision_without_searching_old_text() {
    let (_dir, storage, service) = open();
    message(
        &storage,
        "revision",
        MessageRole::User,
        "old zephyr alignment",
    );
    service.sync(64, &CancellationToken::new()).unwrap();
    let connection = rusqlite::Connection::open(storage.database_path()).unwrap();
    connection
        .execute(
            "UPDATE messages SET content=?1 WHERE id='revision'",
            ["new cobalt alignment"],
        )
        .unwrap();
    service.sync(64, &CancellationToken::new()).unwrap();
    let sources = storage.memory_sources().unwrap();
    assert_eq!(sources.iter().filter(|s| s.state == "active").count(), 1);
    assert_eq!(
        sources.iter().filter(|s| s.state == "superseded").count(),
        1
    );
    let mut old = query("zephyr");
    old.mode = SearchMode::FullText;
    assert!(service
        .search(&old, &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
    let mut new = query("cobalt");
    new.mode = SearchMode::FullText;
    assert_eq!(
        service
            .search(&new, &CancellationToken::new())
            .unwrap()
            .hits[0]
            .text,
        "new cobalt alignment"
    );
}

#[test]
fn schema_two_upgrade_preserves_original_message_and_crowquant_data() {
    let (dir, storage, service) = open();
    message(
        &storage,
        "pre-upgrade",
        MessageRole::User,
        "persisted before native upgrade",
    );
    let old = service.remember("earlier quartz note").unwrap();
    let before = storage.get_crowquant_memory(&old.id).unwrap().unwrap();
    let path = storage.database_path().to_path_buf();
    drop(service);
    drop(storage);
    let connection = rusqlite::Connection::open(&path).unwrap();
    // The disposable fixture is restored to schema 2, preserving all canonical
    // tables/bytes. No user database or checkout data is touched.
    remove_evolution_fixture_schema(&connection);
    connection.execute_batch(RESTORE_ALPHA_TWO).unwrap();
    drop(connection);
    let reopened = Arc::new(Storage::open(dir.path()).unwrap());
    assert_eq!(
        reopened.schema_version().unwrap(),
        crowclaw_desktop_lib::storage::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(
        reopened.get_crowquant_memory(&old.id).unwrap().unwrap(),
        before
    );
    assert_eq!(
        reopened.list_messages("conversation").unwrap()[0].content,
        "persisted before native upgrade"
    );
    let service = MemoryService::new(reopened);
    assert_eq!(service.settings().unwrap().index_conversations, None);
    service.sync(64, &CancellationToken::new()).unwrap();
    assert_eq!(service.status().unwrap().active_sources, 1);
}

#[test]
fn failed_multi_version_upgrade_rolls_back_the_entire_schema_change() {
    let (dir, storage, service) = open();
    message(
        &storage,
        "rollback",
        MessageRole::User,
        "original rollback sentinel",
    );
    let note = service.remember("original quartz note").unwrap();
    let before = storage.get_crowquant_memory(&note.id).unwrap().unwrap();
    let path = storage.database_path().to_path_buf();
    drop(service);
    drop(storage);
    let connection = rusqlite::Connection::open(&path).unwrap();
    remove_evolution_fixture_schema(&connection);
    connection.execute_batch(RESTORE_ALPHA_TWO).unwrap();
    // An unexpected name collision forces the final migration step to fail.
    connection
        .execute_batch("CREATE TABLE memory_embedding_profiles(id TEXT);")
        .unwrap();
    drop(connection);
    assert!(Storage::open(dir.path()).is_err());
    let connection = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(
        version, 2,
        "The whole upgrade must roll back, including completed earlier steps"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='memory_sources'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT content FROM messages WHERE id='rollback'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "original rollback sentinel"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT block FROM crowquant_memories WHERE id=?1",
                [&note.id],
                |r| r.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
        before.block
    );
}

#[test]
fn keyword_search_uses_titles_and_tracks_a_renamed_conversation() {
    let (_dir, storage, service) = open();
    message(
        &storage,
        "title",
        MessageRole::User,
        "observed light intensity",
    );
    service.sync(64, &CancellationToken::new()).unwrap();
    let mut lookup = query("Telescope");
    lookup.mode = SearchMode::FullText;
    assert_eq!(
        service
            .search(&lookup, &CancellationToken::new())
            .unwrap()
            .hits
            .len(),
        1
    );
    storage
        .update_conversation(&ConversationInput {
            id: "conversation".into(),
            title: "Quartz experiment".into(),
            provider_profile_id: None,
        })
        .unwrap();
    service.sync(64, &CancellationToken::new()).unwrap();
    assert!(service
        .search(&lookup, &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
    lookup.query = "Quartz".into();
    assert_eq!(
        service
            .search(&lookup, &CancellationToken::new())
            .unwrap()
            .hits[0]
            .title,
        "Quartz experiment"
    );
}

#[test]
fn oversized_source_is_visible_and_retry_bounded_without_blocking_other_work() {
    let (_dir, storage, service) = open();
    message(
        &storage,
        "large",
        MessageRole::User,
        &"x".repeat(1024 * 1024 + 1),
    );
    for _ in 0..3 {
        assert_eq!(
            service
                .sync(64, &CancellationToken::new())
                .unwrap()
                .warnings
                .len(),
            1
        );
    }
    assert!(!service.status().unwrap().warnings.is_empty());
    assert_eq!(
        service.sync(64, &CancellationToken::new()).unwrap().indexed,
        0
    );
    message(
        &storage,
        "other",
        MessageRole::User,
        "unblocked telescope note",
    );
    assert_eq!(
        service.sync(64, &CancellationToken::new()).unwrap().indexed,
        1
    );
    assert_eq!(storage.list_messages("conversation").unwrap().len(), 2);
}

fn admitted_file_fixture(
    storage: &Storage,
    service: &MemoryService,
) -> crowclaw_desktop_lib::storage::NativeMemorySource {
    use crowclaw_desktop_lib::{
        storage::ProposedActionInput,
        tools::{ActionId, ToolExecution, ToolOutput},
    };
    message(
        storage,
        "file-question",
        MessageRole::User,
        "Read selected file",
    );
    let id: ActionId =
        serde_json::from_value(Value::String(uuid::Uuid::new_v4().to_string())).unwrap();
    storage
        .create_proposed_action(&ProposedActionInput {
            id: id.to_string(),
            conversation_id: "conversation".into(),
            task_id: None,
            tool_name: "read_text_file".into(),
            summary: "Read selected file".into(),
            request: Value::Null,
        })
        .unwrap();
    assert!(service.admit_approved_file(&id.to_string()).is_err());
    storage.approve_action(&id.to_string(), None).unwrap();
    storage
        .record_action_success(
            &id.to_string(),
            &serde_json::to_value(ToolExecution::Executed {
                action_id: id.clone(),
                output: ToolOutput::TextFile {
                    path: "C:/no-such-file.txt".into(),
                    content: "retained sapphire file contents".into(),
                    bytes: 30,
                },
            })
            .unwrap(),
        )
        .unwrap();
    service.admit_approved_file(&id.to_string()).unwrap()
}

#[test]
fn file_recovery_rejects_a_snapshot_that_no_longer_matches_its_admitted_hash() {
    for replacement in [Some("unapproved replacement contents"), None] {
        let (_dir, storage, service) = open();
        let source = admitted_file_fixture(&storage, &service);
        rusqlite::Connection::open(storage.database_path())
            .unwrap()
            .execute(
                "UPDATE memory_sources SET snapshot=?1 WHERE id=?2",
                rusqlite::params![replacement, source.id],
            )
            .unwrap();
        let report = service.rebuild(&CancellationToken::new()).unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("snapshot")),
            "corrupt canonical snapshot must be an explicit indexing failure"
        );
        let mut file = query("unapproved");
        file.source_kind = Some("approved_file".into());
        assert!(service
            .search(&file, &CancellationToken::new())
            .unwrap()
            .hits
            .is_empty());
        let files = storage
            .memory_sources()
            .unwrap()
            .into_iter()
            .filter(|item| item.source_kind == "approved_file")
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id, source.id);
        assert_eq!(files[0].content_hash, source.content_hash);
        assert!(report.pending > 0);
    }
}

#[test]
fn admitted_file_uses_retained_result_without_reopening_and_survives_rebuild() {
    let (dir, storage, service) = open();
    let source = admitted_file_fixture(&storage, &service);
    let id = source.origin_id.clone();
    assert_eq!(source.authorship, "tool");
    assert!(!serde_json::to_string(&source)
        .unwrap()
        .contains("C:/no-such-file"));
    // Simulate process loss after the reset transaction, before any replay.
    storage.memory_reset_index().unwrap();
    drop(service);
    drop(storage);
    let storage = Arc::new(Storage::open(dir.path()).unwrap());
    let service = MemoryService::new(storage.clone());
    service.sync(64, &CancellationToken::new()).unwrap();
    let mut recovered = query("sapphire");
    recovered.source_kind = Some("approved_file".into());
    assert_eq!(
        service
            .search(&recovered, &CancellationToken::new())
            .unwrap()
            .hits
            .len(),
        1
    );
    service.rebuild(&CancellationToken::new()).unwrap();
    let mut file = query("sapphire");
    file.source_kind = Some("approved_file".into());
    assert_eq!(
        service
            .search(&file, &CancellationToken::new())
            .unwrap()
            .hits[0]
            .text,
        "retained sapphire file contents"
    );
    service.withdraw(&source.id).unwrap();
    service.rebuild(&CancellationToken::new()).unwrap();
    assert!(service
        .search(&file, &CancellationToken::new())
        .unwrap()
        .hits
        .is_empty());
    assert!(service.admit_approved_file(&id.to_string()).is_err());
}
