use std::{cmp::Ordering, sync::Arc};

use uuid::Uuid;

use crate::{
    agent::CancellationToken,
    crowquant,
    storage::{CrowQuantMemory, CrowQuantMemoryInput, Storage},
    tools::{
        ActionId, MemoryBackend, MemorySearchMatch, RememberedMemory, ToolError,
        MEMORY_TEXT_MAX_BYTES,
    },
};

#[derive(Clone, Debug, PartialEq)]
pub struct CrowQuantSearchHit {
    pub memory: CrowQuantMemory,
    pub score: f64,
}

/// One native CrowQuant memory service shared by the UI commands and the
/// approval-gated conversational tools.
pub struct CrowQuantMemoryService {
    storage: Arc<Storage>,
}

impl CrowQuantMemoryService {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }

    pub fn list_records(&self) -> Result<Vec<CrowQuantMemory>, String> {
        self.storage
            .list_crowquant_memories()
            .map_err(|error| error.to_string())
    }

    pub fn remember_record(&self, text: &str) -> Result<CrowQuantMemory, String> {
        self.remember_record_with_id(Uuid::new_v4().to_string(), text, &CancellationToken::new())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn remember_agent_record(
        &self,
        action_id: &ActionId,
        text: &str,
        cancellation: &CancellationToken,
    ) -> Result<CrowQuantMemory, ToolError> {
        self.remember_record_with_id(agent_memory_id(action_id), text, cancellation)
    }

    pub fn search_records(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CrowQuantSearchHit>, String> {
        self.search_records_cancellable(query, limit, &CancellationToken::new())
            .map_err(|error| error.to_string())
    }

    fn remember_record_with_id(
        &self,
        id: String,
        text: &str,
        cancellation: &CancellationToken,
    ) -> Result<CrowQuantMemory, ToolError> {
        ensure_not_cancelled(cancellation)?;
        let text = require_memory_text("Memory text", text).map_err(|message| {
            ToolError::MemoryOperation {
                operation: "remember".into(),
                message,
            }
        })?;
        let vector =
            crowquant::vectorize_text(text).map_err(|message| ToolError::MemoryOperation {
                operation: "remember".into(),
                message,
            })?;
        ensure_not_cancelled(cancellation)?;
        let block = crowquant::quantize(&vector).map_err(|message| ToolError::MemoryOperation {
            operation: "remember".into(),
            message,
        })?;
        let serialized = crowquant::serialize(&block);
        // This is the final cooperative boundary before SQLite mutates.
        ensure_not_cancelled(cancellation)?;
        self.storage
            .create_crowquant_memory(&CrowQuantMemoryInput {
                id,
                text: text.into(),
                block: serialized,
                format_version: 1,
                algorithm: crowquant::ALGORITHM.into(),
                dimension: block.dimension,
                seed: block.seed,
                bits: block.bits,
                original_bytes: (vector.len() * std::mem::size_of::<f64>()) as u64,
            })
            .map_err(|error| ToolError::MemoryOperation {
                operation: "remember".into(),
                message: error.to_string(),
            })
    }

    fn search_records_cancellable(
        &self,
        query: &str,
        limit: usize,
        cancellation: &CancellationToken,
    ) -> Result<Vec<CrowQuantSearchHit>, ToolError> {
        ensure_not_cancelled(cancellation)?;
        let query = require_memory_text("Memory query", query).map_err(|message| {
            ToolError::MemoryOperation {
                operation: "search".into(),
                message,
            }
        })?;
        let limit = limit.clamp(1, 20);
        let query_vector =
            crowquant::vectorize_text(query).map_err(|message| ToolError::MemoryOperation {
                operation: "search".into(),
                message,
            })?;
        ensure_not_cancelled(cancellation)?;
        let query_block =
            crowquant::quantize(&query_vector).map_err(|message| ToolError::MemoryOperation {
                operation: "search".into(),
                message,
            })?;
        ensure_not_cancelled(cancellation)?;
        let memories =
            self.storage
                .list_crowquant_memories()
                .map_err(|error| ToolError::MemoryOperation {
                    operation: "search".into(),
                    message: error.to_string(),
                })?;
        let mut hits = Vec::with_capacity(memories.len());
        for memory in memories {
            ensure_not_cancelled(cancellation)?;
            let block = crowquant::deserialize(&memory.block).map_err(|message| {
                ToolError::MemoryOperation {
                    operation: "search".into(),
                    message,
                }
            })?;
            if block.dimension != query_block.dimension
                || block.seed != query_block.seed
                || block.bits != query_block.bits
            {
                return Err(ToolError::MemoryOperation {
                    operation: "search".into(),
                    message: "Stored CrowQuant memory uses incompatible settings".into(),
                });
            }
            let score = crowquant::compressed_cosine(&query_block, &block).map_err(|message| {
                ToolError::MemoryOperation {
                    operation: "search".into(),
                    message,
                }
            })?;
            hits.push(CrowQuantSearchHit { memory, score });
        }
        hits.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| right.memory.created_at_ms.cmp(&left.memory.created_at_ms))
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

impl MemoryBackend for CrowQuantMemoryService {
    fn remember(
        &self,
        action_id: &ActionId,
        text: &str,
        cancellation: &CancellationToken,
    ) -> Result<RememberedMemory, ToolError> {
        self.remember_agent_record(action_id, text, cancellation)
            .map(remembered_memory)
    }

    fn search(
        &self,
        query: &str,
        limit: usize,
        cancellation: &CancellationToken,
    ) -> Result<Vec<MemorySearchMatch>, ToolError> {
        self.search_records_cancellable(query, limit, cancellation)
            .map(|hits| {
                hits.into_iter()
                    .map(|hit| MemorySearchMatch {
                        id: hit.memory.id,
                        text: hit.memory.text,
                        created_at_ms: hit.memory.created_at_ms,
                        score: hit.score,
                    })
                    .collect()
            })
    }
}

pub(crate) fn agent_memory_id(action_id: &ActionId) -> String {
    format!("agent-action-{action_id}")
}

pub(crate) fn remembered_memory(memory: CrowQuantMemory) -> RememberedMemory {
    RememberedMemory {
        id: memory.id,
        text: memory.text,
        created_at_ms: memory.created_at_ms,
        original_bytes: memory.original_bytes,
        compressed_bytes: memory.block.len() as u64,
        algorithm: memory.algorithm,
    }
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), ToolError> {
    if cancellation.is_cancelled() {
        Err(ToolError::Cancelled)
    } else {
        Ok(())
    }
}

fn require_memory_text<'a>(field: &str, value: &'a str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{field} cannot be empty"));
    }
    if value.len() > MEMORY_TEXT_MAX_BYTES {
        return Err(format!(
            "{field} cannot exceed {MEMORY_TEXT_MAX_BYTES} UTF-8 bytes"
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::CrowQuantMemoryService;
    use crate::{
        agent::CancellationToken,
        storage::Storage,
        tools::{ActionId, MemoryBackend},
    };

    #[test]
    fn native_service_persists_and_ranks_crowquant_memory() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(directory.path()).unwrap());
        let service = CrowQuantMemoryService::new(storage.clone());

        let quantum = service
            .remember_record("superconducting qubit calibration and phase coherence")
            .unwrap();
        service
            .remember_record("grocery list with apples bread and milk")
            .unwrap();

        let hits = service.search_records("qubit calibration", 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].memory.id, quantum.id);

        drop(service);
        drop(storage);
        let reopened =
            CrowQuantMemoryService::new(Arc::new(Storage::open(directory.path()).unwrap()));
        assert_eq!(reopened.list_records().unwrap().len(), 2);
    }

    #[test]
    fn tool_projection_reports_real_compressed_record() {
        let directory = tempdir().unwrap();
        let service =
            CrowQuantMemoryService::new(Arc::new(Storage::open(directory.path()).unwrap()));

        let output = service
            .remember(
                &ActionId::new(),
                "remember this locally",
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(output.text, "remember this locally");
        assert_eq!(output.original_bytes, 2048);
        assert!(output.compressed_bytes < output.original_bytes);
        assert_eq!(service.list_records().unwrap().len(), 1);
    }
}
