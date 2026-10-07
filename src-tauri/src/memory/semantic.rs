use serde::{Deserialize, Serialize};

use super::{
    embedding::{self, EmbeddingClient, EMBEDDING_BATCH, EMBEDDING_CODEC, EMBEDDING_TIMEOUT},
    search, EmbeddingProfile, MemoryQuery, MemorySearchResult, MemoryService, MemorySettings,
    SearchMode, SemanticIndexReport, SemanticStatus,
};
use crate::{agent::CancellationToken, storage::StoredMemoryVector};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct SemanticHealth {
    pub profile_id: Option<String>,
    pub error: Option<String>,
    pub failures: u32,
}

impl MemoryService {
    pub(super) fn semantic_status(
        &self,
        settings: &MemorySettings,
    ) -> Result<SemanticStatus, String> {
        let Some(profile) = &settings.embedding else {
            return Ok(SemanticStatus {
                state: "disabled".into(),
                profile_id: None,
                vectors: 0,
                pending: 0,
                detail: None,
            });
        };
        let id = profile.id()?;
        let (vectors, pending) = self
            .storage
            .memory_semantic_counts(
                &id,
                settings.index_conversations == Some(true),
                settings.index_actions,
            )
            .map_err(|e| e.to_string())?;
        let health = self
            .semantic_health
            .lock()
            .map_err(|_| "Semantic status lock poisoned")?;
        let error = if health.profile_id.as_deref() == Some(&id) {
            health.error.clone()
        } else {
            None
        };
        Ok(SemanticStatus {
            state: if error.is_some() {
                "degraded"
            } else if pending > 0 {
                "pending"
            } else {
                "indexed"
            }
            .into(),
            profile_id: Some(id),
            vectors,
            pending,
            detail: error,
        })
    }

    fn record_semantic_health(&self, id: &str, error: Option<String>) -> Result<(), String> {
        let _guard = self
            .configuration
            .lock()
            .map_err(|_| "Memory configuration lock poisoned")?;
        if self
            .settings()?
            .embedding
            .as_ref()
            .map(EmbeddingProfile::id)
            .transpose()?
            .as_deref()
            != Some(id)
        {
            return Ok(());
        }
        let mut health = self
            .semantic_health
            .lock()
            .map_err(|_| "Semantic status lock poisoned")?;
        if health.profile_id.as_deref() != Some(id) {
            *health = SemanticHealth {
                profile_id: Some(id.into()),
                ..Default::default()
            };
        }
        health.failures = if error.is_some() {
            health.failures.saturating_add(1)
        } else {
            0
        };
        health.error = error;
        Ok(())
    }

    async fn embed_local(
        &self,
        settings: &MemorySettings,
        profile: &EmbeddingProfile,
        inputs: &[String],
        token: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, String> {
        let feature = {
            let _guard = self
                .configuration
                .lock()
                .map_err(|_| "Memory configuration lock poisoned")?;
            if self.settings()? != *settings {
                return Err("Indexing settings changed before the semantic request".into());
            }
            self.semantic_cancellation
                .lock()
                .map_err(|_| "Semantic cancellation lock poisoned")?
                .clone()
        };
        let client = EmbeddingClient::new(profile)?;
        let operation = async {
            let _permit = self
                .semantic_gate
                .acquire()
                .await
                .map_err(|_| "Semantic request queue is closed")?;
            client.embed(inputs, token, &feature).await
        };
        tokio::select! {
            _=token.cancelled()=>Err("Memory operation cancelled".into()),
            _=feature.cancelled()=>Err("Semantic profile changed or was disabled".into()),
            result=tokio::time::timeout(EMBEDDING_TIMEOUT,operation)=>result.map_err(|_|"Semantic request timed out after 15 seconds, including queue time")?,
        }
    }

    pub async fn sync_semantic(
        &self,
        token: &CancellationToken,
    ) -> Result<SemanticIndexReport, String> {
        search::check_cancelled(token)?;
        let settings = self.settings()?;
        let Some(profile) = &settings.embedding else {
            return Ok(SemanticIndexReport {
                indexed: 0,
                pending: 0,
                warnings: Vec::new(),
            });
        };
        let id = profile.id()?;
        let status = self.semantic_status(&settings)?;
        {
            let health = self
                .semantic_health
                .lock()
                .map_err(|_| "Semantic status lock poisoned")?;
            if health.profile_id.as_deref() == Some(&id) && health.failures >= 3 {
                return Ok(SemanticIndexReport {indexed:0,pending:status.pending,warnings:vec!["Semantic indexing is held after three failures. Save the profile or retry a search when the local server is available.".into()]});
            }
        }
        let chunks = self
            .storage
            .memory_semantic_candidates(
                &id,
                settings.index_conversations == Some(true),
                settings.index_actions,
                EMBEDDING_BATCH,
            )
            .map_err(|e| e.to_string())?;
        if chunks.is_empty() {
            return Ok(SemanticIndexReport {
                indexed: 0,
                pending: status.pending,
                warnings: Vec::new(),
            });
        }
        let input = chunks.iter().map(|c| c.text.clone()).collect::<Vec<_>>();
        let embeddings = match self.embed_local(&settings, profile, &input, token).await {
            Ok(vectors) => vectors,
            Err(error) if token.is_cancelled() => return Err(error),
            Err(error) => {
                self.record_semantic_health(&id, Some(error.clone()))?;
                return Ok(SemanticIndexReport {indexed:0,pending:status.pending,warnings:vec![format!("Semantic indexing unavailable: {error}. Offline indexing remains available.")]});
            }
        };
        search::check_cancelled(token)?;
        let vectors = chunks
            .iter()
            .zip(embeddings)
            .map(|(chunk, vector)| StoredMemoryVector {
                chunk_id: chunk.id.clone(),
                profile_id: id.clone(),
                dimensions: profile.dimensions,
                codec: EMBEDDING_CODEC.into(),
                data: embedding::encode(&vector),
                content_hash: chunk.content_hash.clone(),
                quarantined: false,
            })
            .collect::<Vec<_>>();
        let indexed = self
            .storage
            .memory_store_vectors(
                &serde_json::to_value(&settings).map_err(|_| "Cannot encode memory settings")?,
                &vectors,
            )
            .map_err(|e| e.to_string())?;
        self.record_semantic_health(&id, None)?;
        let latest = self.settings()?;
        let pending = self.semantic_status(&latest)?.pending;
        Ok(SemanticIndexReport {
            indexed,
            pending,
            warnings: Vec::new(),
        })
    }

    pub async fn search_async(
        &self,
        query: &MemoryQuery,
        token: &CancellationToken,
    ) -> Result<MemorySearchResult, String> {
        search::validate_query(query, token)?;
        let settings = self.settings()?;
        let mut semantic = None;
        let mut warnings = Vec::new();
        if matches!(query.mode, SearchMode::Hybrid | SearchMode::Semantic) {
            if let Some(profile) = &settings.embedding {
                let id = profile.id()?;
                match self
                    .embed_local(&settings, profile, &[query.query.clone()], token)
                    .await
                {
                    Ok(mut vectors) => {
                        search::check_cancelled(token)?;
                        if self
                            .settings()?
                            .embedding
                            .as_ref()
                            .map(EmbeddingProfile::id)
                            .transpose()?
                            .as_deref()
                            == Some(&id)
                        {
                            semantic = Some(search::SemanticQuery {
                                profile_id: id.clone(),
                                dimensions: profile.dimensions,
                                vector: vectors.remove(0),
                            });
                            self.record_semantic_health(&id, None)?;
                        } else {
                            warnings.push(
                                "Semantic profile changed during search; used offline retrieval."
                                    .into(),
                            );
                        }
                    }
                    Err(error) if token.is_cancelled() => return Err(error),
                    Err(error) => {
                        self.record_semantic_health(&id, Some(error.clone()))?;
                        warnings.push(format!("Semantic retrieval unavailable: {error}. Used offline keyword and CrowQuant retrieval."));
                    }
                }
            } else if query.mode == SearchMode::Semantic {
                warnings.push(
                    "Semantic retrieval is disabled; used offline keyword and CrowQuant retrieval."
                        .into(),
                );
            }
        }
        search::check_cancelled(token)?;
        // Read the current indexing choices again after network work; a user
        // disabling a source while a request is in flight is respected here.
        let mut result = search::search_ranked(
            &self.storage,
            &self.settings()?,
            query,
            token,
            semantic.as_ref(),
        )?;
        result.warnings.extend(warnings);
        Ok(result)
    }
}
