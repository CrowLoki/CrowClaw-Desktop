use super::{StorageError, StorageResult};
use serde::{Deserialize, Serialize};

pub const INSTRUCTION_BYTES: usize = 4096;
pub const RESPONSE_BYTES: usize = 16 * 1024;
pub const REFLECTION_CONTEXT_BYTES: usize = 40 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionFeedback {
    pub rating: String,
    pub note: String,
    pub updated_at_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionObservation {
    pub task_id: String,
    pub title: String,
    pub outcome: String,
    pub guideline_revision: Option<u32>,
    pub updated_at_ms: i64,
    pub feedback: Option<EvolutionFeedback>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionRevision {
    pub revision: u32,
    pub title: String,
    pub instructions: String,
    pub reason: String,
    pub created_at_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionProposal {
    pub id: String,
    pub base_revision: u32,
    pub title: String,
    pub rationale: String,
    pub instructions: String,
    pub source_task_ids: Vec<String>,
    pub model: Option<String>,
    pub reported_model: Option<String>,
    pub reflection_context: Option<serde_json::Value>,
    pub status: String,
    pub created_at_ms: i64,
    pub decided_at_ms: Option<i64>,
    pub applied_revision: Option<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionEvaluation {
    pub id: String,
    pub proposal_id: String,
    pub baseline_revision: u32,
    pub model: String,
    pub baseline_model: Option<String>,
    pub candidate_model: Option<String>,
    pub candidate_instructions: String,
    pub prompt: String,
    pub baseline_response: String,
    pub candidate_response: String,
    pub preference: Option<String>,
    pub created_at_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionSnapshot {
    pub active: EvolutionRevision,
    pub observations: Vec<EvolutionObservation>,
    pub proposals: Vec<EvolutionProposal>,
    pub revisions: Vec<EvolutionRevision>,
    pub evaluations: Vec<EvolutionEvaluation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvolutionDraft {
    pub title: String,
    pub rationale: String,
    pub instructions: String,
    pub source_task_ids: Vec<String>,
    pub base_revision: u32,
}

pub fn bounded(label: &str, text: &str, limit: usize, allow_empty: bool) -> StorageResult<()> {
    if (!allow_empty && text.trim().is_empty()) || text.len() > limit || text.contains('\0') {
        return Err(StorageError::InvalidData(format!(
            "{label} must {}contain at most {limit} UTF-8 bytes",
            if allow_empty { "" } else { "be non-empty and " }
        )));
    }
    Ok(())
}
