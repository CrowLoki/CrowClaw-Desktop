use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use sha2::{Digest, Sha256};

use super::{
    MemoryCandidate, NativeMemoryChunk, NativeMemorySource, Storage, StorageError, StorageResult,
};

const SOURCE_COLUMNS: &str = "id,logical_key,source_kind,origin_id,title,authorship,content_hash,snapshot,state,predecessor_id,created_at_ms,updated_at_ms";

fn source_row(row: &Row<'_>) -> rusqlite::Result<NativeMemorySource> {
    Ok(NativeMemorySource {
        id: row.get(0)?,
        logical_key: row.get(1)?,
        source_kind: row.get(2)?,
        origin_id: row.get(3)?,
        title: row.get(4)?,
        authorship: row.get(5)?,
        content_hash: row.get(6)?,
        snapshot: row.get(7)?,
        state: row.get(8)?,
        predecessor_id: row.get(9)?,
        created_at_ms: row.get(10)?,
        updated_at_ms: row.get(11)?,
    })
}

pub(super) fn chunk_row(row: &Row<'_>) -> rusqlite::Result<NativeMemoryChunk> {
    Ok(NativeMemoryChunk {
        id: row.get(0)?,
        source_id: row.get(1)?,
        ordinal: row.get::<_, u32>(2)? as usize,
        text: row.get(3)?,
        start_byte: row.get::<_, u32>(4)? as usize,
        end_byte: row.get::<_, u32>(5)? as usize,
        content_hash: row.get(6)?,
        lexical_block: row.get(7)?,
    })
}

fn sql_integer(value: usize) -> StorageResult<i64> {
    i64::try_from(value)
        .map_err(|_| StorageError::InvalidData("Memory integer exceeds storage range".into()))
}

pub(crate) fn sources_from(connection: &Connection) -> StorageResult<Vec<NativeMemorySource>> {
    let mut stmt = connection.prepare(&format!(
        "SELECT {SOURCE_COLUMNS} FROM memory_sources ORDER BY created_at_ms,id"
    ))?;
    let rows = stmt.query_map([], source_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn chunks_from(connection: &Connection) -> StorageResult<Vec<NativeMemoryChunk>> {
    let mut stmt = connection.prepare("SELECT id,source_id,ordinal,text,start_byte,end_byte,content_hash,lexical_block FROM memory_chunks ORDER BY source_id,ordinal")?;
    let rows = stmt.query_map([], chunk_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn exclusions_from(connection: &Connection) -> StorageResult<Vec<String>> {
    let mut stmt =
        connection.prepare("SELECT logical_key FROM memory_exclusions ORDER BY logical_key")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

impl Storage {
    pub fn has_memory_history(&self) -> StorageResult<bool> {
        Ok(self
            .connection()?
            .query_row("SELECT EXISTS(SELECT 1 FROM messages)", [], |r| r.get(0))?)
    }

    pub fn memory_pending(&self, conversations: bool, actions: bool) -> StorageResult<usize> {
        Ok(self.connection()?.query_row("SELECT count(*) FROM memory_jobs WHERE (source_kind='conversation_message' AND ?1) OR (source_kind='approved_action' AND ?2) OR source_kind IN ('legacy_crowquant','user_note')", params![conversations,actions], |r| r.get::<_,u32>(0))? as usize)
    }

    pub fn memory_jobs(
        &self,
        conversations: bool,
        actions: bool,
        limit: usize,
    ) -> StorageResult<Vec<(String, String)>> {
        let connection = self.connection()?;
        let mut stmt = connection.prepare("SELECT j.source_kind,j.origin_id FROM memory_jobs j WHERE ((j.source_kind='conversation_message' AND ?1) OR (j.source_kind='approved_action' AND ?2) OR j.source_kind IN ('legacy_crowquant','user_note')) AND NOT EXISTS(SELECT 1 FROM memory_job_errors e WHERE e.source_kind=j.source_kind AND e.origin_id=j.origin_id AND e.attempts>=3) ORDER BY j.source_kind,j.origin_id LIMIT ?3")?;
        let rows = stmt.query_map(params![conversations, actions, sql_integer(limit)?], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn memory_candidate(&self, kind: &str, id: &str) -> StorageResult<Option<MemoryCandidate>> {
        let connection = self.connection()?;
        let sql = match kind {
            "conversation_message" => "SELECT 'conversation_message',m.id,c.title,m.role,m.content,m.created_at_ms FROM messages m JOIN conversations c ON c.id=m.conversation_id WHERE m.id=?1",
            "approved_action" => "SELECT 'approved_action',id,tool_name,'tool',summary,created_at_ms FROM proposed_actions WHERE id=?1 AND status='succeeded' AND tool_name NOT IN ('remember_memory','search_memory')",
            "legacy_crowquant" | "user_note" => "SELECT coalesce((SELECT source_kind FROM memory_sources WHERE origin_id=q.id AND source_kind IN ('legacy_crowquant','user_note') ORDER BY created_at_ms LIMIT 1),'legacy_crowquant'),q.id,'Remembered note',coalesce((SELECT authorship FROM memory_sources WHERE origin_id=q.id AND source_kind IN ('legacy_crowquant','user_note') ORDER BY created_at_ms LIMIT 1),CASE WHEN q.id LIKE 'agent-action-%' THEN 'assistant' ELSE 'user' END),q.text,q.created_at_ms FROM crowquant_memories q WHERE q.id=?1",
            _ => return Err(StorageError::InvalidData("Unsupported memory source kind".into())),
        };
        Ok(connection
            .query_row(sql, [id], |r| {
                Ok(MemoryCandidate {
                    source_kind: r.get(0)?,
                    origin_id: r.get(1)?,
                    title: r.get(2)?,
                    authorship: r.get(3)?,
                    text: r.get(4)?,
                    created_at_ms: r.get(5)?,
                })
            })
            .optional()?)
    }

    pub fn memory_source_excluded(&self, key: &str) -> StorageResult<bool> {
        Ok(self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_exclusions WHERE logical_key=?1)",
            [key],
            |r| r.get(0),
        )?)
    }

    pub fn memory_complete_job(&self, kind: &str, origin_id: &str) -> StorageResult<()> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM memory_jobs WHERE source_kind IN (?1,?3) AND origin_id=?2",
            params![
                kind,
                origin_id,
                if kind == "user_note" {
                    "legacy_crowquant"
                } else {
                    kind
                }
            ],
        )?;
        tx.execute(
            "DELETE FROM memory_job_errors WHERE source_kind IN (?1,?3) AND origin_id=?2",
            params![
                kind,
                origin_id,
                if kind == "user_note" {
                    "legacy_crowquant"
                } else {
                    kind
                }
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn memory_fail_job(&self, kind: &str, id: &str, error: &str) -> StorageResult<()> {
        let safe_error: String = error.chars().take(512).collect();
        self.connection()?.execute("INSERT INTO memory_job_errors(source_kind,origin_id,attempts,error) VALUES(?1,?2,1,?3) ON CONFLICT(source_kind,origin_id) DO UPDATE SET attempts=attempts+1,error=excluded.error",params![kind,id,safe_error])?;
        Ok(())
    }

    pub fn memory_index_warnings(&self) -> StorageResult<Vec<String>> {
        let connection = self.connection()?;
        let mut stmt=connection.prepare("SELECT error,attempts FROM memory_job_errors e WHERE EXISTS(SELECT 1 FROM memory_jobs j WHERE j.source_kind=e.source_kind AND j.origin_id=e.origin_id) ORDER BY source_kind,origin_id LIMIT 20")?;
        let rows = stmt.query_map([], |r| {
            Ok(format!(
                "{} ({} failed attempt(s); rebuild retries held jobs)",
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Commits source revision and its entire derived chunk set together.
    /// Rechecks canonical text under the writer lock, so a concurrent update or
    /// deletion cannot install stale chunks or erase its newer durable job.
    pub fn memory_store_revision(
        &self,
        source: &NativeMemorySource,
        chunks: &[NativeMemoryChunk],
    ) -> StorageResult<bool> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let excluded: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_exclusions WHERE logical_key=?1)",
            [&source.logical_key],
            |r| r.get(0),
        )?;
        if excluded {
            return Ok(false);
        }
        let text: Option<String> = match source.source_kind.as_str() {
            "conversation_message" => tx
                .query_row(
                    "SELECT content FROM messages WHERE id=?1",
                    [&source.origin_id],
                    |r| r.get(0),
                )
                .optional()?,
            "legacy_crowquant" | "user_note" => tx
                .query_row(
                    "SELECT text FROM crowquant_memories WHERE id=?1",
                    [&source.origin_id],
                    |r| r.get(0),
                )
                .optional()?,
            "approved_action" => tx
                .query_row(
                    "SELECT summary FROM proposed_actions WHERE id=?1 AND status='succeeded'",
                    [&source.origin_id],
                    |r| r.get(0),
                )
                .optional()?,
            "approved_file" => {
                let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM proposed_actions WHERE id=?1 AND tool_name='read_text_file' AND status='succeeded')",[&source.origin_id],|r|r.get(0))?;
                if valid {
                    source.snapshot.clone()
                } else {
                    None
                }
            }
            _ => None,
        };
        if text.as_ref().is_none_or(|text| {
            format!("{:x}", Sha256::digest(text.as_bytes())) != source.content_hash
        }) {
            return Err(StorageError::Conflict(
                "Memory source changed during indexing; retry pending work".into(),
            ));
        }
        let previous: Option<String> = tx
            .query_row(
                "SELECT id FROM memory_sources WHERE logical_key=?1 AND state='active'",
                [&source.logical_key],
                |r| r.get(0),
            )
            .optional()?;
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_chunks WHERE source_id=?1)",
            [&source.id],
            |r| r.get(0),
        )?;
        if previous.as_deref() == Some(&source.id) && existing {
            tx.execute(
                "UPDATE memory_sources SET title=?2,authorship=?3,updated_at_ms=?4 WHERE id=?1",
                params![
                    source.id,
                    source.title,
                    source.authorship,
                    source.updated_at_ms
                ],
            )?;
            tx.execute(
                "UPDATE memory_chunks SET title=?2 WHERE source_id=?1 AND title!=?2",
                params![source.id, source.title],
            )?;
            tx.execute(
                "DELETE FROM memory_jobs WHERE origin_id=?1 AND source_kind IN (?2,?3)",
                params![
                    source.origin_id,
                    source.source_kind,
                    if source.source_kind == "user_note" {
                        "legacy_crowquant"
                    } else {
                        source.source_kind.as_str()
                    }
                ],
            )?;
            tx.execute(
                "DELETE FROM memory_job_errors WHERE origin_id=?1 AND source_kind IN (?2,?3)",
                params![
                    source.origin_id,
                    source.source_kind,
                    if source.source_kind == "user_note" {
                        "legacy_crowquant"
                    } else {
                        source.source_kind.as_str()
                    }
                ],
            )?;
            tx.commit()?;
            return Ok(false);
        }
        tx.execute("UPDATE memory_sources SET state='superseded',updated_at_ms=?2 WHERE logical_key=?1 AND state='active'", params![source.logical_key,source.updated_at_ms])?;
        tx.execute("INSERT INTO memory_sources(id,logical_key,source_kind,origin_id,title,authorship,content_hash,snapshot,state,predecessor_id,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'active',?9,?10,?11) ON CONFLICT(id) DO UPDATE SET state='active',updated_at_ms=excluded.updated_at_ms", params![source.id,source.logical_key,source.source_kind,source.origin_id,source.title,source.authorship,source.content_hash,source.snapshot,previous.filter(|p| p != &source.id),source.created_at_ms,source.updated_at_ms])?;
        tx.execute("DELETE FROM memory_chunks WHERE source_id=?1", [&source.id])?;
        for chunk in chunks {
            tx.execute("INSERT INTO memory_chunks(id,source_id,ordinal,text,start_byte,end_byte,content_hash,lexical_block,title) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![chunk.id,source.id,sql_integer(chunk.ordinal)?,chunk.text,sql_integer(chunk.start_byte)?,sql_integer(chunk.end_byte)?,chunk.content_hash,chunk.lexical_block,source.title])?;
        }
        tx.execute(
            "DELETE FROM memory_jobs WHERE origin_id=?1 AND source_kind IN (?2,?3)",
            params![
                source.origin_id,
                source.source_kind,
                if source.source_kind == "user_note" {
                    "legacy_crowquant"
                } else {
                    source.source_kind.as_str()
                }
            ],
        )?;
        tx.execute(
            "DELETE FROM memory_job_errors WHERE origin_id=?1 AND source_kind IN (?2,?3)",
            params![
                source.origin_id,
                source.source_kind,
                if source.source_kind == "user_note" {
                    "legacy_crowquant"
                } else {
                    source.source_kind.as_str()
                }
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn memory_sources(&self) -> StorageResult<Vec<NativeMemorySource>> {
        let connection = self.connection()?;
        sources_from(&connection)
    }

    pub fn memory_counts(
        &self,
        conversations: bool,
        actions: bool,
    ) -> StorageResult<(usize, usize)> {
        let connection = self.connection()?;
        let (sources,chunks):(u32,u32)=connection.query_row("SELECT count(DISTINCT s.id),count(c.id) FROM memory_sources s LEFT JOIN memory_chunks c ON c.source_id=s.id WHERE s.state='active' AND (s.source_kind!='conversation_message' OR ?1) AND (s.source_kind!='approved_action' OR ?2)",params![conversations,actions],|r|Ok((r.get(0)?,r.get(1)?)))?;
        Ok((sources as usize, chunks as usize))
    }

    pub fn memory_source_active(&self, id: &str) -> StorageResult<bool> {
        Ok(self.connection()?.query_row("SELECT EXISTS(SELECT 1 FROM memory_sources s WHERE s.id=?1 AND s.state='active' AND NOT EXISTS(SELECT 1 FROM memory_exclusions x WHERE x.logical_key=s.logical_key))",[id],|r|r.get(0))?)
    }

    pub fn memory_crowquant_withdrawn(&self, id: &str) -> StorageResult<bool> {
        Ok(self.connection()?.query_row("SELECT EXISTS(SELECT 1 FROM memory_exclusions WHERE logical_key IN ('legacy_crowquant:'||?1,'user_note:'||?1))",[id],|r|r.get(0))?)
    }

    pub fn memory_active_chunks(
        &self,
        conversations: bool,
        actions: bool,
        limit: usize,
    ) -> StorageResult<Vec<NativeMemoryChunk>> {
        let connection = self.connection()?;
        let mut stmt = connection.prepare("SELECT c.id,c.source_id,c.ordinal,c.text,c.start_byte,c.end_byte,c.content_hash,c.lexical_block FROM memory_chunks c JOIN memory_sources s ON s.id=c.source_id WHERE s.state='active' AND (s.source_kind!='conversation_message' OR ?1) AND (s.source_kind!='approved_action' OR ?2) ORDER BY c.id LIMIT ?3")?;
        let rows = stmt.query_map(
            params![conversations, actions, sql_integer(limit)?],
            chunk_row,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn memory_fts_rank(
        &self,
        expression: &str,
        conversations: bool,
        actions: bool,
        limit: usize,
    ) -> StorageResult<Vec<String>> {
        let connection = self.connection()?;
        let mut stmt = connection.prepare("SELECT c.id FROM memory_fts JOIN memory_chunks c ON c.rowid=memory_fts.rowid JOIN memory_sources s ON s.id=c.source_id WHERE memory_fts MATCH ?1 AND s.state='active' AND (s.source_kind!='conversation_message' OR ?2) AND (s.source_kind!='approved_action' OR ?3) ORDER BY bm25(memory_fts),c.id LIMIT ?4")?;
        let rows = stmt.query_map(
            params![expression, conversations, actions, sql_integer(limit)?],
            |r| r.get(0),
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn memory_withdraw(&self, id: &str) -> StorageResult<()> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key: String = tx
            .query_row(
                "SELECT logical_key FROM memory_sources WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::not_found("memory", id))?;
        tx.execute("INSERT OR IGNORE INTO memory_exclusions VALUES(?1)", [&key])?;
        tx.execute("DELETE FROM memory_chunks WHERE source_id IN (SELECT id FROM memory_sources WHERE logical_key=?1)",[&key])?;
        tx.execute(
            "UPDATE memory_sources SET state='withdrawn' WHERE logical_key=?1",
            [key],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn memory_reset_index(&self) -> StorageResult<()> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch("DELETE FROM memory_chunks; DELETE FROM memory_job_errors; INSERT INTO memory_fts(memory_fts) VALUES('rebuild'); INSERT OR IGNORE INTO memory_jobs SELECT 'conversation_message',id FROM messages; INSERT OR IGNORE INTO memory_jobs SELECT 'legacy_crowquant',id FROM crowquant_memories; INSERT OR IGNORE INTO memory_jobs SELECT 'approved_action',id FROM proposed_actions WHERE status='succeeded';")?;
        tx.commit()?;
        Ok(())
    }
}
