use super::{
    NativeMemoryChunk, Storage, StorageError, StorageResult, StoredEmbeddingProfile,
    StoredMemoryVector,
};
use rusqlite::{params, Connection, Row, TransactionBehavior};
use serde_json::Value;

fn vector_row(row: &Row<'_>) -> rusqlite::Result<StoredMemoryVector> {
    Ok(StoredMemoryVector {
        chunk_id: row.get(0)?,
        profile_id: row.get(1)?,
        dimensions: row.get(2)?,
        codec: row.get(3)?,
        data: row.get(4)?,
        content_hash: row.get(5)?,
        quarantined: row.get(6)?,
    })
}

pub(crate) fn profiles_from(connection: &Connection) -> StorageResult<Vec<StoredEmbeddingProfile>> {
    let mut stmt = connection.prepare(
        "SELECT id,config_json,created_at_ms FROM memory_embedding_profiles ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (id, config, created_at_ms) = row?;
        Ok(StoredEmbeddingProfile {
            id,
            config: serde_json::from_str(&config)?,
            created_at_ms,
        })
    })
    .collect()
}

pub(crate) fn vectors_from(connection: &Connection) -> StorageResult<Vec<StoredMemoryVector>> {
    let mut stmt=connection.prepare("SELECT chunk_id,profile_id,dimensions,codec,data,content_hash,quarantined FROM memory_vectors ORDER BY profile_id,chunk_id")?;
    let rows = stmt.query_map([], vector_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

impl Storage {
    pub fn memory_save_embedding_profile(&self, id: &str, config: &Value) -> StorageResult<()> {
        self.connection()?.execute("INSERT OR IGNORE INTO memory_embedding_profiles(id,config_json,created_at_ms) VALUES(?1,?2,?3)",params![id,serde_json::to_string(config)?,super::now_ms()?])?;
        Ok(())
    }

    pub fn memory_semantic_counts(
        &self,
        profile: &str,
        conversations: bool,
        actions: bool,
    ) -> StorageResult<(usize, usize)> {
        let connection = self.connection()?;
        let (total,ready):(u32,u32)=connection.query_row("SELECT count(*),coalesce(sum(CASE WHEN v.chunk_id IS NOT NULL AND v.quarantined=0 AND v.content_hash=c.content_hash THEN 1 ELSE 0 END),0) FROM memory_chunks c JOIN memory_sources s ON s.id=c.source_id LEFT JOIN memory_vectors v ON v.chunk_id=c.id AND v.profile_id=?1 WHERE s.state='active' AND (s.source_kind!='conversation_message' OR ?2) AND (s.source_kind!='approved_action' OR ?3)",params![profile,conversations,actions],|r|Ok((r.get(0)?,r.get(1)?)))?;
        Ok((ready as usize, (total - ready) as usize))
    }

    pub fn memory_semantic_candidates(
        &self,
        profile: &str,
        conversations: bool,
        actions: bool,
        limit: usize,
    ) -> StorageResult<Vec<NativeMemoryChunk>> {
        let connection = self.connection()?;
        let mut stmt=connection.prepare("SELECT c.id,c.source_id,c.ordinal,c.text,c.start_byte,c.end_byte,c.content_hash,c.lexical_block FROM memory_chunks c JOIN memory_sources s ON s.id=c.source_id LEFT JOIN memory_vectors v ON v.chunk_id=c.id AND v.profile_id=?1 WHERE s.state='active' AND (s.source_kind!='conversation_message' OR ?2) AND (s.source_kind!='approved_action' OR ?3) AND (v.chunk_id IS NULL OR v.quarantined=1 OR v.content_hash!=c.content_hash) ORDER BY c.id LIMIT ?4")?;
        let rows = stmt.query_map(
            params![profile, conversations, actions, limit as i64],
            super::memory::chunk_row,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// A changed setting, withdrawn source or replaced chunk cannot accept a
    /// late vector. These checks share the same transaction as the insert.
    pub fn memory_store_vectors(
        &self,
        expected_settings: &Value,
        vectors: &[StoredMemoryVector],
    ) -> StorageResult<usize> {
        if vectors.len() > 8 {
            return Err(StorageError::InvalidData(
                "Semantic batch exceeds eight vectors".into(),
            ));
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: String = tx.query_row(
            "SELECT value_json FROM settings WHERE key='memory_settings'",
            [],
            |r| r.get(0),
        )?;
        if serde_json::from_str::<Value>(&current)? != *expected_settings {
            return Ok(0);
        }
        let mut stored = 0;
        for vector in vectors {
            if !(1..=4096).contains(&vector.dimensions)
                || vector.codec != "f32le-normalized-v1"
                || vector.data.len() != vector.dimensions as usize * 4
            {
                return Err(StorageError::InvalidData(
                    "Semantic vector violates the native codec contract".into(),
                ));
            }
            stored+=tx.execute("INSERT INTO memory_vectors(chunk_id,profile_id,dimensions,codec,data,content_hash,quarantined) SELECT c.id,?2,?3,?4,?5,c.content_hash,0 FROM memory_chunks c JOIN memory_sources s ON s.id=c.source_id WHERE c.id=?1 AND c.content_hash=?6 AND s.state='active' AND NOT EXISTS(SELECT 1 FROM memory_exclusions x WHERE x.logical_key=s.logical_key) ON CONFLICT(chunk_id,profile_id) DO UPDATE SET dimensions=excluded.dimensions,codec=excluded.codec,data=excluded.data,content_hash=excluded.content_hash,quarantined=0",params![vector.chunk_id,vector.profile_id,vector.dimensions,vector.codec,vector.data,vector.content_hash])?;
        }
        tx.commit()?;
        Ok(stored)
    }

    pub fn memory_semantic_vectors(
        &self,
        profile: &str,
        conversations: bool,
        actions: bool,
        limit: usize,
    ) -> StorageResult<Vec<StoredMemoryVector>> {
        let connection = self.connection()?;
        let mut stmt=connection.prepare("SELECT v.chunk_id,v.profile_id,v.dimensions,v.codec,v.data,v.content_hash,v.quarantined FROM memory_vectors v JOIN memory_chunks c ON c.id=v.chunk_id JOIN memory_sources s ON s.id=c.source_id WHERE v.profile_id=?1 AND v.quarantined=0 AND v.content_hash=c.content_hash AND s.state='active' AND (s.source_kind!='conversation_message' OR ?2) AND (s.source_kind!='approved_action' OR ?3) ORDER BY v.chunk_id LIMIT ?4")?;
        let rows = stmt.query_map(
            params![profile, conversations, actions, limit as i64],
            vector_row,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn memory_quarantine_vector(&self, vector: &StoredMemoryVector) -> StorageResult<()> {
        self.connection()?.execute("UPDATE memory_vectors SET quarantined=1 WHERE chunk_id=?1 AND profile_id=?2 AND data=?3",params![vector.chunk_id,vector.profile_id,vector.data])?;
        Ok(())
    }
}
