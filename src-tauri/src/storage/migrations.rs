use rusqlite::{Connection, TransactionBehavior};

use super::{StorageError, StorageResult};

pub const CURRENT_SCHEMA_VERSION: u32 = 11;

pub(crate) fn migrate(connection: &mut Connection) -> StorageResult<()> {
    let installed_version: u32 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if installed_version > CURRENT_SCHEMA_VERSION {
        return Err(StorageError::Conflict(format!(
            "database schema version {installed_version} is newer than supported version {CURRENT_SCHEMA_VERSION}"
        )));
    }

    if installed_version == CURRENT_SCHEMA_VERSION {
        return Ok(());
    }
    // Every intermediate version is part of one upgrade. A later failed step
    // must not strand an earlier installation at a half-upgraded schema.
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if installed_version < 1 {
        migrate_to_v1(&transaction)?;
    }
    if installed_version < 2 {
        migrate_to_v2(&transaction)?;
    }
    if installed_version < 3 {
        migrate_to_v3(&transaction)?;
    }
    if installed_version < 4 {
        migrate_to_v4(&transaction)?;
    }
    if installed_version < 5 {
        migrate_to_v5(&transaction)?;
    }
    if installed_version < 6 {
        migrate_to_v6(&transaction)?;
    }
    if installed_version < 7 {
        migrate_to_v7(&transaction)?;
    }
    if installed_version < 8 {
        migrate_to_v8(&transaction)?;
    }
    if installed_version < 9 {
        migrate_to_v9(&transaction)?;
    }
    if installed_version < 10 {
        migrate_to_v10(&transaction)?;
    }
    if installed_version < 11 {
        migrate_to_v11(&transaction)?;
    }

    transaction.commit()?;

    Ok(())
}

fn migrate_to_v11(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(
        "CREATE TABLE openrouter_credentials (
            provider_profile_id TEXT PRIMARY KEY NOT NULL REFERENCES provider_profiles(id) ON DELETE CASCADE,
            protected_blob BLOB NOT NULL CHECK(typeof(protected_blob)='blob' AND length(protected_blob) BETWEEN 1 AND 2097152)
        );",
    )?;
    connection.pragma_update(None, "user_version", 11u32)?;
    Ok(())
}

fn migrate_to_v10(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(
        "CREATE UNIQUE INDEX messages_attachment_owner ON messages(conversation_id,id);
        CREATE TABLE conversation_attachments (
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            id TEXT NOT NULL,
            message_id TEXT,
            name TEXT NOT NULL,
            media_type TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('text','image','file')),
            byte_length INTEGER NOT NULL CHECK(byte_length BETWEEN 0 AND 20971520),
            sha256 TEXT NOT NULL CHECK(length(sha256)=64),
            created_at_ms INTEGER NOT NULL,
            bytes BLOB NOT NULL CHECK(length(bytes)=byte_length),
            PRIMARY KEY(conversation_id,id),
            FOREIGN KEY(conversation_id,message_id) REFERENCES messages(conversation_id,id) ON DELETE CASCADE
        );
        CREATE INDEX attachments_by_message ON conversation_attachments(conversation_id,message_id,created_at_ms,id);
        CREATE TRIGGER attachment_snapshot_immutable BEFORE UPDATE ON conversation_attachments
        WHEN old.message_id IS NOT NULL OR new.conversation_id<>old.conversation_id
          OR new.id<>old.id OR new.name<>old.name OR new.media_type<>old.media_type
          OR new.kind<>old.kind OR new.byte_length<>old.byte_length OR new.sha256<>old.sha256
          OR new.created_at_ms<>old.created_at_ms OR new.bytes<>old.bytes
        BEGIN SELECT RAISE(ABORT,'Attachment snapshots are immutable'); END;",
    )?;
    connection.pragma_update(None, "user_version", 10u32)?;
    Ok(())
}

fn migrate_to_v9(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(
        "CREATE TABLE conversation_composers (
            conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
            revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 2147483647),
            draft TEXT NOT NULL,
            selection_json TEXT,
            updated_at_ms INTEGER NOT NULL
        );",
    )?;
    connection.pragma_update(None, "user_version", 9u32)?;
    Ok(())
}

fn migrate_to_v8(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(
        "ALTER TABLE membership_accounts ADD COLUMN session_version INTEGER NOT NULL DEFAULT 1 CHECK(session_version BETWEEN 1 AND 2147483647);",
    )?;
    connection.pragma_update(None, "user_version", 8u32)?;
    Ok(())
}

fn migrate_to_v1(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(
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
    connection.pragma_update(None, "user_version", 1u32)?;
    Ok(())
}

fn migrate_to_v2(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(
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
    connection.pragma_update(None, "user_version", 2u32)?;
    Ok(())
}

fn migrate_to_v3(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(
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
    connection.pragma_update(None, "user_version", 3u32)?;
    Ok(())
}

fn migrate_to_v4(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE memory_embedding_profiles (
            id TEXT PRIMARY KEY NOT NULL,
            config_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL
        );
        CREATE TABLE memory_vectors (
            chunk_id TEXT NOT NULL REFERENCES memory_chunks(id) ON DELETE CASCADE,
            profile_id TEXT NOT NULL REFERENCES memory_embedding_profiles(id),
            dimensions INTEGER NOT NULL CHECK(dimensions BETWEEN 1 AND 4096),
            codec TEXT NOT NULL CHECK(codec='f32le-normalized-v1'),
            data BLOB NOT NULL,
            content_hash TEXT NOT NULL,
            quarantined INTEGER NOT NULL DEFAULT 0 CHECK(quarantined IN (0,1)),
            PRIMARY KEY(chunk_id,profile_id)
        );
    "#,
    )?;
    connection.pragma_update(None, "user_version", 4u32)?;
    Ok(())
}

fn migrate_to_v5(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(r#"
        INSERT OR IGNORE INTO memory_exclusions
            SELECT 'legacy_crowquant:' || substr(logical_key,length('user_note:')+1)
            FROM memory_exclusions WHERE substr(logical_key,1,length('user_note:'))='user_note:';
        INSERT OR IGNORE INTO memory_exclusions
            SELECT 'user_note:' || substr(logical_key,length('legacy_crowquant:')+1)
            FROM memory_exclusions WHERE substr(logical_key,1,length('legacy_crowquant:'))='legacy_crowquant:';
        DELETE FROM memory_chunks WHERE source_id IN (
            SELECT id FROM memory_sources s WHERE s.source_kind IN ('user_note','legacy_crowquant')
            AND EXISTS(SELECT 1 FROM memory_exclusions x WHERE x.logical_key=s.logical_key));
        UPDATE memory_sources SET state='withdrawn' WHERE source_kind IN ('user_note','legacy_crowquant')
            AND EXISTS(SELECT 1 FROM memory_exclusions x WHERE x.logical_key=memory_sources.logical_key);
        DELETE FROM memory_jobs WHERE source_kind IN ('user_note','legacy_crowquant')
            AND EXISTS(SELECT 1 FROM memory_exclusions x WHERE x.logical_key='user_note:'||memory_jobs.origin_id);
        DELETE FROM memory_chunks WHERE source_id IN (
            SELECT legacy.id FROM memory_sources legacy WHERE legacy.source_kind='legacy_crowquant'
            AND EXISTS(SELECT 1 FROM memory_sources native WHERE native.source_kind='user_note'
                AND native.origin_id=legacy.origin_id AND native.state='active'));
        UPDATE memory_sources SET state='superseded' WHERE source_kind='legacy_crowquant' AND state='active'
            AND EXISTS(SELECT 1 FROM memory_sources native WHERE native.source_kind='user_note'
                AND native.origin_id=memory_sources.origin_id AND native.state='active');
        INSERT OR IGNORE INTO memory_jobs
            SELECT source_kind,origin_id FROM memory_sources s
            WHERE s.state='active' AND s.source_kind IN ('user_note','approved_file')
            AND NOT EXISTS(SELECT 1 FROM memory_chunks c WHERE c.source_id=s.id);

        CREATE TRIGGER memory_note_exclusion AFTER INSERT ON memory_exclusions
        WHEN substr(new.logical_key,1,instr(new.logical_key,':')-1) IN ('user_note','legacy_crowquant') BEGIN
            INSERT OR IGNORE INTO memory_exclusions VALUES(
                CASE substr(new.logical_key,1,instr(new.logical_key,':')-1)
                    WHEN 'user_note' THEN 'legacy_crowquant:' ELSE 'user_note:' END
                || substr(new.logical_key,instr(new.logical_key,':')+1));
            DELETE FROM memory_chunks WHERE source_id IN (SELECT id FROM memory_sources
                WHERE source_kind IN ('user_note','legacy_crowquant')
                AND origin_id=substr(new.logical_key,instr(new.logical_key,':')+1));
            UPDATE memory_sources SET state='withdrawn' WHERE source_kind IN ('user_note','legacy_crowquant')
                AND origin_id=substr(new.logical_key,instr(new.logical_key,':')+1);
            DELETE FROM memory_jobs WHERE source_kind IN ('user_note','legacy_crowquant')
                AND origin_id=substr(new.logical_key,instr(new.logical_key,':')+1);
        END;
    "#)?;
    connection.pragma_update(None, "user_version", 5u32)?;
    Ok(())
}

fn migrate_to_v6(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(r#"
        CREATE TABLE evolution_revisions (
            revision INTEGER PRIMARY KEY CHECK(revision >= 0 AND revision <= 2147483647),
            title TEXT NOT NULL, instructions TEXT NOT NULL, reason TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL
        );
        INSERT INTO evolution_revisions VALUES(0,'Initial behavior','','No adopted working guidelines',CAST(strftime('%s','now') AS INTEGER)*1000);
        CREATE TABLE evolution_head (
            id INTEGER PRIMARY KEY CHECK(id=1),
            revision INTEGER NOT NULL REFERENCES evolution_revisions(revision)
        );
        INSERT INTO evolution_head VALUES(1,0);
        CREATE TABLE evolution_feedback (
            task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
            rating TEXT NOT NULL CHECK(rating IN ('useful','needs_improvement','uncertain')),
            note TEXT NOT NULL, updated_at_ms INTEGER NOT NULL
        );
        CREATE TABLE evolution_proposals (
            id TEXT PRIMARY KEY NOT NULL,
            base_revision INTEGER NOT NULL REFERENCES evolution_revisions(revision),
            title TEXT NOT NULL, rationale TEXT NOT NULL, instructions TEXT NOT NULL,
            source_task_ids_json TEXT NOT NULL, model TEXT, reported_model TEXT, reflection_context_json TEXT,
            status TEXT NOT NULL CHECK(status IN ('draft','applied','rejected')),
            created_at_ms INTEGER NOT NULL, decided_at_ms INTEGER,
            applied_revision INTEGER REFERENCES evolution_revisions(revision)
        );
        CREATE TABLE evolution_evaluations (
            id TEXT PRIMARY KEY NOT NULL,
            proposal_id TEXT NOT NULL REFERENCES evolution_proposals(id) ON DELETE CASCADE,
            baseline_revision INTEGER NOT NULL REFERENCES evolution_revisions(revision), model TEXT NOT NULL, baseline_model TEXT, candidate_model TEXT,
            candidate_instructions TEXT NOT NULL, prompt TEXT NOT NULL,
            baseline_response TEXT NOT NULL, candidate_response TEXT NOT NULL,
            preference TEXT CHECK(preference IN ('baseline','candidate','tie','neither')),
            created_at_ms INTEGER NOT NULL
        );
        CREATE INDEX evolution_proposals_recent ON evolution_proposals(created_at_ms DESC,id);
        CREATE INDEX evolution_evaluations_recent ON evolution_evaluations(created_at_ms DESC,id);
    "#)?;
    connection.pragma_update(None, "user_version", 6u32)?;
    Ok(())
}

fn migrate_to_v7(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(r#"
        CREATE TABLE membership_host (id INTEGER PRIMARY KEY CHECK(id=1),host_id TEXT NOT NULL UNIQUE);
        CREATE TABLE membership_accounts (
            id TEXT PRIMARY KEY, provider TEXT NOT NULL CHECK(provider IN ('chatgpt','claude')),
            label TEXT NOT NULL,issuer TEXT NOT NULL,subject TEXT NOT NULL,client_id TEXT NOT NULL,
            email TEXT,host_id TEXT NOT NULL,credential_blob BLOB,
            credential_version INTEGER NOT NULL DEFAULT 0 CHECK(credential_version BETWEEN 0 AND 2147483647),
            catalog_json TEXT,selection_json TEXT,created_at_ms INTEGER NOT NULL,updated_at_ms INTEGER NOT NULL,
            UNIQUE(provider,issuer,subject,client_id), UNIQUE(provider,label)
        );
    "#)?;
    connection.pragma_update(None, "user_version", 7u32)?;
    Ok(())
}
