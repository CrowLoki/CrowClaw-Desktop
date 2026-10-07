use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use sha2::{Digest, Sha256};

use super::{
    now_ms, require_non_empty, CrowQuantMemory, CrowQuantMemoryInput, Storage, StorageResult,
};

impl Storage {
    pub fn create_crowquant_memory(
        &self,
        input: &CrowQuantMemoryInput,
    ) -> StorageResult<CrowQuantMemory> {
        require_non_empty("memory id", &input.id)?;
        require_non_empty("memory text", &input.text)?;
        require_non_empty("memory algorithm", &input.algorithm)?;
        let created_at_ms = now_ms()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO crowquant_memories
             (id, text, block, format_version, algorithm, dimension, seed, bits, original_bytes, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                input.id,
                input.text,
                input.block,
                input.format_version,
                input.algorithm,
                input.dimension,
                input.seed,
                input.bits,
                i64::try_from(input.original_bytes).map_err(|_| {
                    super::StorageError::InvalidData("memory size exceeds SQLite integer range".into())
                })?,
                created_at_ms,
            ],
        )?;
        // The insert trigger queues indexing. Publish its source identity in
        // this same transaction, before any worker can classify the new row as
        // a legacy note. Chunks remain independently rebuildable.
        let key = format!("user_note:{}", input.id);
        let legacy_key = format!("legacy_crowquant:{}", input.id);
        let content_hash = format!("{:x}", Sha256::digest(input.text.as_bytes()));
        let source_id = format!(
            "{:x}",
            Sha256::digest(format!("crowclaw.source.v1\0{key}\0{content_hash}").as_bytes())
        );
        let agent = input.id.starts_with("agent-action-");
        transaction.execute(
            "INSERT INTO memory_sources(id,logical_key,source_kind,origin_id,title,authorship,content_hash,snapshot,state,created_at_ms,updated_at_ms)
             VALUES(?1,?2,'user_note',?3,?4,?5,?6,NULL,CASE WHEN EXISTS(SELECT 1 FROM memory_exclusions WHERE logical_key IN (?2,?8)) THEN 'withdrawn' ELSE 'active' END,?7,?7)
             ON CONFLICT(id) DO NOTHING",
            params![source_id,key,input.id,if agent {"Approved remembered note"} else {"Your note"},if agent {"assistant"} else {"user"},content_hash,created_at_ms,legacy_key],
        )?;
        transaction.commit()?;
        Ok(CrowQuantMemory {
            id: input.id.clone(),
            text: input.text.clone(),
            block: input.block.clone(),
            format_version: input.format_version,
            algorithm: input.algorithm.clone(),
            dimension: input.dimension,
            seed: input.seed,
            bits: input.bits,
            original_bytes: input.original_bytes,
            created_at_ms,
        })
    }

    pub fn list_crowquant_memories(&self) -> StorageResult<Vec<CrowQuantMemory>> {
        let connection = self.connection()?;
        list_crowquant_memories_from(&connection)
    }

    pub fn get_crowquant_memory(&self, id: &str) -> StorageResult<Option<CrowQuantMemory>> {
        require_non_empty("memory id", id)?;
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT id, text, block, format_version, algorithm, dimension, seed, bits, original_bytes, created_at_ms
                 FROM crowquant_memories WHERE id = ?1",
                [id],
                row_to_memory,
            )
            .optional()
            .map_err(Into::into)
    }
}

pub(crate) fn list_crowquant_memories_from(
    connection: &Connection,
) -> StorageResult<Vec<CrowQuantMemory>> {
    let mut statement = connection.prepare(
            "SELECT id, text, block, format_version, algorithm, dimension, seed, bits, original_bytes, created_at_ms
             FROM crowquant_memories ORDER BY created_at_ms DESC, id ASC",
        )?;
    let rows = statement.query_map([], row_to_memory)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn row_to_memory(row: &Row<'_>) -> rusqlite::Result<CrowQuantMemory> {
    Ok(CrowQuantMemory {
        id: row.get(0)?,
        text: row.get(1)?,
        block: row.get(2)?,
        format_version: row.get(3)?,
        algorithm: row.get(4)?,
        dimension: row.get(5)?,
        seed: row.get(6)?,
        bits: row.get(7)?,
        original_bytes: u64::try_from(row.get::<_, i64>(8)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                8,
                rusqlite::types::Type::Integer,
                Box::new(error),
            )
        })?,
        created_at_ms: row.get(9)?,
    })
}
