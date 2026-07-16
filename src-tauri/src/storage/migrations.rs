use rusqlite::{Connection, TransactionBehavior};

use super::{StorageError, StorageResult};

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

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
    transaction.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}
