use serde::{Deserialize, Serialize};

pub const SOURCE_MAX_BYTES: usize = 1024 * 1024;
pub const CHUNK_MAX_BYTES: usize = 2048;
pub const CHUNK_OVERLAP_BYTES: usize = 256;
pub const QUERY_MAX_BYTES: usize = 4096;
pub const INDEX_BATCH: usize = 64;
pub const SCAN_MAX_CHUNKS: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySettings {
    /// None is the upgrade choice, before any historical indexing is allowed.
    pub index_conversations: Option<bool>,
    pub index_actions: bool,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            index_conversations: Some(true),
            index_actions: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    #[default]
    Hybrid,
    FullText,
    Lexical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryQuery {
    pub query: String,
    pub limit: usize,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub mode: SearchMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryChannel {
    pub channel: String,
    pub rank: usize,
    pub score: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryHit {
    pub chunk_id: String,
    pub source_id: String,
    pub source_kind: String,
    pub origin_id: String,
    pub title: String,
    pub authorship: String,
    pub text: String,
    pub created_at_ms: i64,
    pub start_byte: usize,
    pub end_byte: usize,
    pub score: f64,
    pub channels: Vec<MemoryChannel>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchResult {
    pub hits: Vec<MemoryHit>,
    pub mode: SearchMode,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStatus {
    pub settings: MemorySettings,
    pub active_sources: usize,
    pub chunks: usize,
    pub pending: usize,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexReport {
    pub indexed: usize,
    pub skipped: usize,
    pub pending: usize,
    pub warnings: Vec<String>,
}
