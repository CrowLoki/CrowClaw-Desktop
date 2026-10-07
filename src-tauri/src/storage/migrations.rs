use rusqlite::{Connection, TransactionBehavior};

use super::{StorageError, StorageResult};

pub const CURRENT_SCHEMA_VERSION: u32 = 3;

pub(crate) fn migrate(connection: &mut Connection) -> StorageResult<()> {
    let installed_version: u32 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if installed_version > CURRENT_SCHEMA_VERSION {
        return Err(StorageError::Conflict(format!(
            "database schema version {installed_version} is newer than supported version {CURRENT_SCHEMA_VERSION}"
        )));
    }

    if installed_version < 1 {
        migrate_to_v1(connection)?;
    }
    if installed_version < 2 {
        migrate_to_v2(connection)?;
    }
    if installed_version < 3 {
        migrate_to_v3(connection)?;
    }

    Ok(())
}

fn migrate_to_v1(connection: &mut Connection) -> StorageResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY NOT NULL,
            value_json TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS provider_profiles (
            id TEXT PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            base_url TEXT NOT NULL,
            model TEXT NOT NULL,
            provider_kind TEXT NOT NULL,
            credential_reference TEXT,
            is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS one_default_provider_profile
            ON provider_profiles(is_default) WHERE is_default = 1;

        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY NOT NULL,
            title TEXT NOT NULL,
            provider_profile_id TEXT REFERENCES provider_profiles(id) ON DELETE SET NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            archived_at_ms INTEGER
        );

        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY NOT NULL,
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
            role TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
            content TEXT NOT NULL,
            metadata_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            UNIQUE (conversation_id, ordinal)
        );
        CREATE INDEX IF NOT EXISTS messages_by_conversation
            ON messages(conversation_id, ordinal);

        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY NOT NULL,
            conversation_id TEXT REFERENCES conversations(id) ON DELETE SET NULL,
            kind TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
            cancellation_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancellation_requested IN (0, 1)),
            result_json TEXT,
            error TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            started_at_ms INTEGER,
            completed_at_ms INTEGER
        );
        CREATE INDEX IF NOT EXISTS tasks_by_status ON tasks(status, created_at_ms);

        CREATE TABLE IF NOT EXISTS proposed_actions (
            id TEXT PRIMARY KEY NOT NULL,
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
            tool_name TEXT NOT NULL,
            summary TEXT NOT NULL,
            request_json TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'denied', 'succeeded', 'failed')),
            decision_reason TEXT,
            decided_at_ms INTEGER,
            result_json TEXT,
            error TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS actions_by_conversation
            ON proposed_actions(conversation_id, created_at_ms);

        CREATE TABLE IF NOT EXISTS action_audit (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            action_id TEXT NOT NULL REFERENCES proposed_actions(id) ON DELETE CASCADE,
            event_kind TEXT NOT NULL,
            detail_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS action_audit_by_action
            ON action_audit(action_id, sequence);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", 1u32)?;
    transaction.commit()?;
    Ok(())
}

fn migrate_to_v2(connection: &mut Connection) -> StorageResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS crowquant_memories (
            id TEXT PRIMARY KEY NOT NULL,
            text TEXT NOT NULL,
            block BLOB NOT NULL,
            format_version INTEGER NOT NULL CHECK (format_version > 0),
            algorithm TEXT NOT NULL,
            dimension INTEGER NOT NULL CHECK (dimension > 0),
            seed INTEGER NOT NULL,
            bits INTEGER NOT NULL CHECK (bits BETWEEN 1 AND 8),
            original_bytes INTEGER NOT NULL CHECK (original_bytes > 0),
            created_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS crowquant_memories_newest
            ON crowquant_memories(created_at_ms DESC);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", 2u32)?;
    transaction.commit()?;
    Ok(())
}

fn migrate_to_v3(connection: &mut Connection) -> StorageResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        r#"
        CREATE TABLE memory_sources (
            id TEXT PRIMARY KEY NOT NULL,
            logical_key TEXT NOT NULL,
            source_kind TEXT NOT NULL CHECK(source_kind IN ('conversation_message','approved_action','user_note','approved_file','legacy_crowquant')),
            origin_id TEXT NOT NULL,
            title TEXT NOT NULL,
            authorship TEXT NOT NULL CHECK(authorship IN ('user','assistant','tool','system')),
            content_hash TEXT NOT NULL,
            snapshot TEXT,
            state TEXT NOT NULL CHECK(state IN ('active','superseded','withdrawn')),
            predecessor_id TEXT REFERENCES memory_sources(id),
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        CREATE UNIQUE INDEX memory_one_active_revision ON memory_sources(logical_key) WHERE state='active';
        CREATE INDEX memory_source_origin ON memory_sources(source_kind, origin_id);
        CREATE TABLE memory_chunks (
            id TEXT PRIMARY KEY NOT NULL,
            source_id TEXT NOT NULL REFERENCES memory_sources(id) ON DELETE CASCADE,
            ordinal INTEGER NOT NULL,
            text TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            content_hash TEXT NOT NULL,
            lexical_block BLOB,
            UNIQUE(source_id, ordinal)
        );
        CREATE VIRTUAL TABLE memory_fts USING fts5(text, title, content='memory_chunks', content_rowid='rowid');
        CREATE TRIGGER memory_chunk_insert AFTER INSERT ON memory_chunks BEGIN
            INSERT INTO memory_fts(rowid,text,title) VALUES(new.rowid,new.text,new.title);
        END;
        CREATE TRIGGER memory_chunk_delete AFTER DELETE ON memory_chunks BEGIN
            INSERT INTO memory_fts(memory_fts,rowid,text,title) VALUES('delete',old.rowid,old.text,old.title);
        END;
        CREATE TRIGGER memory_chunk_update AFTER UPDATE ON memory_chunks BEGIN
            INSERT INTO memory_fts(memory_fts,rowid,text,title) VALUES('delete',old.rowid,old.text,old.title);
            INSERT INTO memory_fts(rowid,text,title) VALUES(new.rowid,new.text,new.title);
        END;
        CREATE TABLE memory_exclusions (logical_key TEXT PRIMARY KEY NOT NULL);
        CREATE TABLE memory_jobs (
            source_kind TEXT NOT NULL,
            origin_id TEXT NOT NULL,
            PRIMARY KEY(source_kind,origin_id)
        );
        CREATE TABLE memory_job_errors (
            source_kind TEXT NOT NULL,
            origin_id TEXT NOT NULL,
            attempts INTEGER NOT NULL,
            error TEXT NOT NULL,
            PRIMARY KEY(source_kind,origin_id)
        );
        INSERT INTO memory_jobs SELECT 'conversation_message',id FROM messages;
        INSERT INTO memory_jobs SELECT 'legacy_crowquant',id FROM crowquant_memories;
        INSERT INTO memory_jobs SELECT 'approved_action',id FROM proposed_actions WHERE status='succeeded';
        CREATE TRIGGER memory_message_insert AFTER INSERT ON messages BEGIN
            INSERT OR IGNORE INTO memory_jobs VALUES('conversation_message',new.id);
        END;
        CREATE TRIGGER memory_message_update AFTER UPDATE OF content ON messages BEGIN
            INSERT OR IGNORE INTO memory_jobs VALUES('conversation_message',new.id);
            DELETE FROM memory_job_errors WHERE source_kind='conversation_message' AND origin_id=new.id;
        END;
        CREATE TRIGGER memory_message_delete AFTER DELETE ON messages BEGIN
            DELETE FROM memory_chunks WHERE source_id IN (SELECT id FROM memory_sources WHERE source_kind='conversation_message' AND origin_id=old.id);
            UPDATE memory_sources SET state='withdrawn',snapshot=NULL WHERE source_kind='conversation_message' AND origin_id=old.id;
            DELETE FROM memory_jobs WHERE source_kind='conversation_message' AND origin_id=old.id;
        END;
        CREATE TRIGGER memory_conversation_title AFTER UPDATE OF title ON conversations BEGIN
            INSERT OR IGNORE INTO memory_jobs SELECT 'conversation_message',id FROM messages WHERE conversation_id=new.id;
        END;
        CREATE TRIGGER memory_crowquant_insert AFTER INSERT ON crowquant_memories BEGIN
            INSERT OR IGNORE INTO memory_jobs VALUES('legacy_crowquant',new.id);
        END;
        CREATE TRIGGER memory_crowquant_delete AFTER DELETE ON crowquant_memories BEGIN
            DELETE FROM memory_chunks WHERE source_id IN (SELECT id FROM memory_sources WHERE source_kind IN ('legacy_crowquant','user_note') AND origin_id=old.id);
            UPDATE memory_sources SET state='withdrawn',snapshot=NULL WHERE source_kind IN ('legacy_crowquant','user_note') AND origin_id=old.id;
            DELETE FROM memory_jobs WHERE source_kind IN ('legacy_crowquant','user_note') AND origin_id=old.id;
        END;
        CREATE TRIGGER memory_action_success AFTER UPDATE OF status ON proposed_actions WHEN new.status='succeeded' BEGIN
            INSERT OR IGNORE INTO memory_jobs VALUES('approved_action',new.id);
        END;
        CREATE TRIGGER memory_action_delete AFTER DELETE ON proposed_actions BEGIN
            DELETE FROM memory_chunks WHERE source_id IN (SELECT id FROM memory_sources WHERE source_kind IN ('approved_action','approved_file') AND origin_id=old.id);
            UPDATE memory_sources SET state='withdrawn',snapshot=NULL WHERE source_kind IN ('approved_action','approved_file') AND origin_id=old.id;
            DELETE FROM memory_jobs WHERE source_kind='approved_action' AND origin_id=old.id;
        END;
        "#,
    )?;
    transaction.pragma_update(None, "user_version", 3u32)?;
    transaction.commit()?;
    Ok(())
}
