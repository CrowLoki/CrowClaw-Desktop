use std::sync::{Arc, Mutex};

use super::{
    chunker, search, IndexReport, MemoryQuery, MemorySearchResult, MemorySettings, MemoryStatus,
    INDEX_BATCH, SCAN_MAX_CHUNKS,
};
use crate::{
    agent::CancellationToken,
    crowquant_memory::{remembered_memory, CrowQuantMemoryService},
    storage::{ActionStatus, CrowQuantMemory, MemoryCandidate, NativeMemorySource, Storage},
    tools::{
        ActionId, MemoryBackend, MemorySearchMatch, MemorySearchResponse, RememberedMemory,
        ToolError, ToolExecution, ToolOutput,
    },
};

const SETTINGS_KEY: &str = "memory_settings";

pub struct MemoryService {
    pub(super) storage: Arc<Storage>,
    initial_choice: Option<bool>,
    indexing: Mutex<()>,
    pub(super) configuration: Mutex<()>,
    pub(super) semantic_gate: tokio::sync::Semaphore,
    pub(super) semantic_cancellation: Mutex<CancellationToken>,
    pub(super) semantic_health: Mutex<super::semantic::SemanticHealth>,
}

impl MemoryService {
    pub fn new(storage: Arc<Storage>) -> Self {
        let initial_choice = match storage.has_memory_history() {
            Ok(false) => Some(true),
            _ => None,
        };
        Self {
            storage,
            initial_choice,
            indexing: Mutex::new(()),
            configuration: Mutex::new(()),
            semantic_gate: tokio::sync::Semaphore::new(1),
            semantic_cancellation: Mutex::new(CancellationToken::new()),
            semantic_health: Mutex::new(Default::default()),
        }
    }

    pub fn settings(&self) -> Result<MemorySettings, String> {
        Ok(self
            .storage
            .get_setting(SETTINGS_KEY)
            .map_err(|e| e.to_string())?
            .unwrap_or(MemorySettings {
                index_conversations: self.initial_choice,
                index_actions: false,
                embedding: None,
            }))
    }

    pub fn configure(&self, mut settings: MemorySettings) -> Result<MemorySettings, String> {
        let _guard = self
            .configuration
            .lock()
            .map_err(|_| "Memory configuration lock poisoned")?;
        let previous = self.settings()?;
        if let Some(profile) = settings.embedding.as_mut() {
            *profile = profile.normalized()?;
            self.storage
                .memory_save_embedding_profile(
                    &profile.id()?,
                    &serde_json::to_value(&profile)
                        .map_err(|_| "Cannot encode embedding profile")?,
                )
                .map_err(|e| e.to_string())?;
        }
        self.storage
            .set_setting(SETTINGS_KEY, &settings)
            .map_err(|e| e.to_string())?;
        if previous != settings {
            let mut token = self
                .semantic_cancellation
                .lock()
                .map_err(|_| "Semantic cancellation lock poisoned")?;
            token.cancel();
            *token = CancellationToken::new();
        }
        *self
            .semantic_health
            .lock()
            .map_err(|_| "Semantic status lock poisoned")? = Default::default();
        Ok(settings)
    }

    pub fn sync(&self, limit: usize, token: &CancellationToken) -> Result<IndexReport, String> {
        search::check_cancelled(token)?;
        if !(1..=INDEX_BATCH).contains(&limit) {
            return Err(format!("Index batch must be 1–{INDEX_BATCH}"));
        }
        let _guard = self
            .indexing
            .lock()
            .map_err(|_| "Memory index lock poisoned")?;
        let settings = self.settings()?;
        if self
            .storage
            .get_setting::<MemorySettings>(SETTINGS_KEY)
            .map_err(|e| e.to_string())?
            .is_none()
        {
            self.configure(settings.clone())?;
        }
        let jobs = self
            .storage
            .memory_jobs(
                settings.index_conversations == Some(true),
                settings.index_actions,
                limit,
            )
            .map_err(|e| e.to_string())?;
        let mut report = IndexReport {
            indexed: 0,
            skipped: 0,
            pending: 0,
            warnings: Vec::new(),
        };
        for (kind, id) in jobs {
            search::check_cancelled(token)?;
            match self.index_job(&kind, &id, token) {
                Ok(true) => report.indexed += 1,
                Ok(false) => report.skipped += 1,
                Err(error) if token.is_cancelled() => return Err(error),
                Err(error) => {
                    self.storage
                        .memory_fail_job(&kind, &id, &error)
                        .map_err(|e| e.to_string())?;
                    report.warnings.push(error);
                }
            }
        }
        report.pending = self
            .storage
            .memory_pending(
                settings.index_conversations == Some(true),
                settings.index_actions,
            )
            .map_err(|e| e.to_string())?;
        Ok(report)
    }

    fn index_job(&self, kind: &str, id: &str, token: &CancellationToken) -> Result<bool, String> {
        if kind == "approved_file" {
            if let Some(source) = self
                .storage
                .memory_file_source(id)
                .map_err(|e| e.to_string())?
            {
                let text = source
                    .snapshot
                    .as_ref()
                    .ok_or("Remembered file snapshot is missing")?;
                if chunker::hash(text) != source.content_hash {
                    return Err("Remembered file snapshot failed its recorded content hash; it was not indexed".into());
                }
                let chunks = chunker::chunks(&source.id, text)?;
                search::check_cancelled(token)?;
                return self
                    .storage
                    .memory_store_revision(&source, &chunks)
                    .map_err(|e| e.to_string());
            }
        } else if let Some(candidate) = self
            .storage
            .memory_candidate(kind, id)
            .map_err(|e| e.to_string())?
        {
            return self.index_candidate(candidate, token);
        }
        self.storage
            .memory_complete_job(kind, id)
            .map_err(|e| e.to_string())?;
        Ok(false)
    }

    fn index_candidate(
        &self,
        candidate: MemoryCandidate,
        token: &CancellationToken,
    ) -> Result<bool, String> {
        search::check_cancelled(token)?;
        let key = format!("{}:{}", candidate.source_kind, candidate.origin_id);
        if self
            .storage
            .memory_source_excluded(&key)
            .map_err(|e| e.to_string())?
        {
            self.storage
                .memory_complete_job(&candidate.source_kind, &candidate.origin_id)
                .map_err(|e| e.to_string())?;
            return Ok(false);
        }
        let content_hash = chunker::hash(&candidate.text);
        let id = chunker::hash(&format!("crowclaw.source.v1\0{key}\0{content_hash}"));
        let chunks = chunker::chunks(&id, &candidate.text)?;
        search::check_cancelled(token)?;
        let source = NativeMemorySource {
            id,
            logical_key: key,
            source_kind: candidate.source_kind,
            origin_id: candidate.origin_id,
            title: candidate.title,
            authorship: candidate.authorship,
            content_hash,
            snapshot: None,
            state: "active".into(),
            predecessor_id: None,
            created_at_ms: candidate.created_at_ms,
            updated_at_ms: crate::storage::now_ms().map_err(|e| e.to_string())?,
        };
        self.storage
            .memory_store_revision(&source, &chunks)
            .map_err(|e| e.to_string())
    }

    pub fn search(
        &self,
        query: &MemoryQuery,
        token: &CancellationToken,
    ) -> Result<MemorySearchResult, String> {
        search::check_cancelled(token)?;
        search::search(&self.storage, &self.settings()?, query, token)
    }

    pub fn remember(&self, text: &str) -> Result<CrowQuantMemory, String> {
        let record = CrowQuantMemoryService::new(self.storage.clone()).remember_record(text)?;
        // Canonical insertion already succeeded. Its durable job remains if
        // derived indexing fails; never claim the durable remember failed.
        let _ = self.index_candidate(
            MemoryCandidate {
                source_kind: "user_note".into(),
                origin_id: record.id.clone(),
                title: "Your note".into(),
                authorship: "user".into(),
                text: record.text.clone(),
                created_at_ms: record.created_at_ms,
            },
            &CancellationToken::new(),
        );
        Ok(record)
    }

    pub fn withdraw(&self, source_id: &str) -> Result<(), String> {
        self.storage
            .memory_withdraw(source_id)
            .map_err(|e| e.to_string())
    }

    /// Admits the retained exact result of an approved read. Never reopens the
    /// file and never gains access to any other path or action.
    pub fn admit_approved_file(&self, action_id: &str) -> Result<NativeMemorySource, String> {
        let action = self
            .storage
            .get_proposed_action(action_id)
            .map_err(|e| e.to_string())?
            .ok_or("Approved file action was not found")?;
        if action.status != ActionStatus::Succeeded || action.tool_name != "read_text_file" {
            return Err("Only a successful approved file read can be remembered".into());
        }
        let execution: ToolExecution = serde_json::from_value(
            action
                .result
                .ok_or("Approved file content was not retained")?,
        )
        .map_err(|_| "Approved file result is invalid")?;
        let text = match execution {
            ToolExecution::Executed {
                action_id: bound_id,
                output: ToolOutput::TextFile { content, .. },
            } if bound_id.to_string() == action_id => content,
            _ => return Err("Approved file result did not match this action".into()),
        };
        let key = format!("approved_file:{action_id}");
        let content_hash = chunker::hash(&text);
        let id = chunker::hash(&format!("crowclaw.source.v1\0{key}\0{content_hash}"));
        let chunks = chunker::chunks(&id, &text)?;
        let source = NativeMemorySource {
            id,
            logical_key: key,
            source_kind: "approved_file".into(),
            origin_id: action_id.into(),
            title: "Remembered approved file".into(),
            authorship: "tool".into(),
            content_hash,
            snapshot: Some(text),
            state: "active".into(),
            predecessor_id: None,
            created_at_ms: action.created_at_ms,
            updated_at_ms: crate::storage::now_ms().map_err(|e| e.to_string())?,
        };
        if self
            .storage
            .memory_source_excluded(&source.logical_key)
            .map_err(|e| e.to_string())?
        {
            return Err("This source was withdrawn from memory".into());
        }
        self.storage
            .memory_store_revision(&source, &chunks)
            .map_err(|e| e.to_string())?;
        Ok(source)
    }

    pub fn rebuild(&self, token: &CancellationToken) -> Result<IndexReport, String> {
        search::check_cancelled(token)?;
        self.storage
            .memory_reset_index()
            .map_err(|e| e.to_string())?;
        self.sync(INDEX_BATCH, token)
    }

    pub fn status(&self) -> Result<MemoryStatus, String> {
        let settings = self.settings()?;
        let (source_count, count) = self
            .storage
            .memory_counts(
                settings.index_conversations == Some(true),
                settings.index_actions,
            )
            .map_err(|e| e.to_string())?;
        let mut warnings = self
            .storage
            .memory_index_warnings()
            .map_err(|e| e.to_string())?;
        if count > SCAN_MAX_CHUNKS {
            warnings.push(format!(
                "Index exceeds the {SCAN_MAX_CHUNKS}-chunk scan bound"
            ));
        }
        Ok(MemoryStatus {
            semantic: self.semantic_status(&settings)?,
            pending: self
                .storage
                .memory_pending(
                    settings.index_conversations == Some(true),
                    settings.index_actions,
                )
                .map_err(|e| e.to_string())?,
            active_sources: source_count,
            settings,
            chunks: count,
            warnings,
        })
    }
}

#[async_trait::async_trait]
impl MemoryBackend for MemoryService {
    fn remember(
        &self,
        action_id: &ActionId,
        text: &str,
        token: &CancellationToken,
    ) -> Result<RememberedMemory, ToolError> {
        let record = CrowQuantMemoryService::new(self.storage.clone())
            .remember_agent_record(action_id, text, token)?;
        let _ = self.index_candidate(
            MemoryCandidate {
                source_kind: "user_note".into(),
                origin_id: record.id.clone(),
                title: "Approved remembered note".into(),
                authorship: "assistant".into(),
                text: record.text.clone(),
                created_at_ms: record.created_at_ms,
            },
            token,
        );
        Ok(remembered_memory(record))
    }

    fn search(
        &self,
        query: &str,
        limit: usize,
        token: &CancellationToken,
    ) -> Result<Vec<MemorySearchMatch>, ToolError> {
        search::check_cancelled(token).map_err(|_| ToolError::Cancelled)?;
        let result = self
            .search(
                &MemoryQuery {
                    query: query.into(),
                    limit,
                    source_kind: None,
                    mode: super::SearchMode::Hybrid,
                },
                token,
            )
            .map_err(|message| {
                if token.is_cancelled() {
                    ToolError::Cancelled
                } else {
                    ToolError::MemoryOperation {
                        operation: "search".into(),
                        message,
                    }
                }
            })?;
        Ok(result.hits.into_iter().map(|h|MemorySearchMatch {
            id:if ["legacy_crowquant","user_note"].contains(&h.source_kind.as_str()) {h.origin_id.clone()}else{h.chunk_id.clone()},text:h.text,created_at_ms:h.created_at_ms,score:h.score,
            provenance:Some(serde_json::json!({"sourceId":h.source_id,"originId":h.origin_id,"sourceKind":h.source_kind,"authorship":h.authorship,"channels":h.channels,"startByte":h.start_byte,"endByte":h.end_byte,"historicalContext":true})),
        }).collect())
    }

    async fn search_async(
        &self,
        query: &str,
        limit: usize,
        token: &CancellationToken,
    ) -> Result<MemorySearchResponse, ToolError> {
        let result = MemoryService::search_async(
            self,
            &MemoryQuery {
                query: query.into(),
                limit,
                source_kind: None,
                mode: super::SearchMode::Hybrid,
            },
            token,
        )
        .await
        .map_err(|message| {
            if token.is_cancelled() {
                ToolError::Cancelled
            } else {
                ToolError::MemoryOperation {
                    operation: "search".into(),
                    message,
                }
            }
        })?;
        Ok(MemorySearchResponse {warnings:result.warnings,results:result.hits.into_iter().map(|h|MemorySearchMatch {
            id:if ["legacy_crowquant","user_note"].contains(&h.source_kind.as_str()){h.origin_id.clone()}else{h.chunk_id.clone()},text:h.text,created_at_ms:h.created_at_ms,score:h.score,
            provenance:Some(serde_json::json!({"sourceId":h.source_id,"originId":h.origin_id,"sourceKind":h.source_kind,"authorship":h.authorship,"channels":h.channels,"startByte":h.start_byte,"endByte":h.end_byte,"historicalContext":true})),
        }).collect()})
    }
}
