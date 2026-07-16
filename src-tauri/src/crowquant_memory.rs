use std::{cmp::Ordering, sync::Arc};

use uuid::Uuid;

use crate::{
    crowquant,
    storage::{CrowQuantMemory, CrowQuantMemoryInput, Storage},
    tools::{MemoryBackend, MemorySearchMatch, RememberedMemory, ToolError, MEMORY_TEXT_MAX_BYTES},
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
        let text = require_memory_text("Memory text", text)?;
        let vector = crowquant::vectorize_text(text)?;
        let block = crowquant::quantize(&vector)?;
        let serialized = crowquant::serialize(&block);
        self.storage
            .create_crowquant_memory(&CrowQuantMemoryInput {
                id: Uuid::new_v4().to_string(),
                text: text.into(),
                block: serialized,
                format_version: 1,
                algorithm: crowquant::ALGORITHM.into(),
                dimension: block.dimension,
                seed: block.seed,
                bits: block.bits,
                original_bytes: (vector.len() * std::mem::size_of::<f64>()) as u64,
            })
            .map_err(|error| error.to_string())
    }

    pub fn search_records(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CrowQuantSearchHit>, String> {
        let query = require_memory_text("Memory query", query)?;
        let limit = limit.clamp(1, 20);
        let query_block = crowquant::quantize(&crowquant::vectorize_text(query)?)?;
        let mut hits = self
            .storage
            .list_crowquant_memories()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|memory| {
                let block = crowquant::deserialize(&memory.block)?;
                if block.dimension != query_block.dimension
                    || block.seed != query_block.seed
                    || block.bits != query_block.bits
                {
                    return Err("Stored CrowQuant memory uses incompatible settings".to_string());
                }
                let score = crowquant::compressed_cosine(&query_block, &block)?;
                Ok(CrowQuantSearchHit { memory, score })
            })
            .collect::<Result<Vec<_>, String>>()?;
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
    fn remember(&self, text: &str) -> Result<RememberedMemory, ToolError> {
        let memory = self
            .remember_record(text)
            .map_err(|message| ToolError::MemoryOperation {
                operation: "remember".into(),
                message,
            })?;
        Ok(RememberedMemory {
            id: memory.id,
            text: memory.text,
            created_at_ms: memory.created_at_ms,
            original_bytes: memory.original_bytes,
            compressed_bytes: memory.block.len() as u64,
            algorithm: memory.algorithm,
        })
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<MemorySearchMatch>, ToolError> {
        self.search_records(query, limit)
            .map_err(|message| ToolError::MemoryOperation {
                operation: "search".into(),
                message,
            })
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
    use crate::{storage::Storage, tools::MemoryBackend};

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

        let output = service.remember("remember this locally").unwrap();
        assert_eq!(output.text, "remember this locally");
        assert_eq!(output.original_bytes, 2048);
        assert!(output.compressed_bytes < output.original_bytes);
        assert_eq!(service.list_records().unwrap().len(), 1);
    }
}
