use std::collections::{HashMap, HashSet};

use super::embedding;
use super::{
    MemoryChannel, MemoryHit, MemoryQuery, MemorySearchResult, MemorySettings, SearchMode,
    QUERY_MAX_BYTES, SCAN_MAX_CHUNKS,
};
use crate::{agent::CancellationToken, crowquant, storage::Storage};

pub(super) struct SemanticQuery {
    pub profile_id: String,
    pub dimensions: u32,
    pub vector: Vec<f32>,
}

pub(super) fn check_cancelled(token: &CancellationToken) -> Result<(), String> {
    if token.is_cancelled() {
        Err("Memory operation cancelled".into())
    } else {
        Ok(())
    }
}

pub(super) fn fts_expression(query: &str) -> String {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|v| !v.is_empty())
        .take(128)
        .map(|v| format!("\"{v}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub(super) fn search(
    storage: &Storage,
    settings: &MemorySettings,
    query: &MemoryQuery,
    token: &CancellationToken,
) -> Result<MemorySearchResult, String> {
    search_ranked(storage, settings, query, token, None)
}

pub(super) fn validate_query(query: &MemoryQuery, token: &CancellationToken) -> Result<(), String> {
    check_cancelled(token)?;
    if query.query.trim().is_empty() || query.query.len() > QUERY_MAX_BYTES {
        return Err(format!(
            "Memory query must contain 1–{QUERY_MAX_BYTES} UTF-8 bytes"
        ));
    }
    if !(1..=20).contains(&query.limit) {
        return Err("Memory result limit must be 1–20".into());
    }
    if let Some(kind) = &query.source_kind {
        if ![
            "conversation_message",
            "approved_action",
            "user_note",
            "approved_file",
            "legacy_crowquant",
        ]
        .contains(&kind.as_str())
        {
            return Err("Unsupported memory source filter".into());
        }
    }
    Ok(())
}

pub(super) fn search_ranked(
    storage: &Storage,
    settings: &MemorySettings,
    query: &MemoryQuery,
    token: &CancellationToken,
    semantic: Option<&SemanticQuery>,
) -> Result<MemorySearchResult, String> {
    validate_query(query, token)?;
    let conversations = settings.index_conversations == Some(true);
    let mut chunks = storage
        .memory_active_chunks(conversations, settings.index_actions, SCAN_MAX_CHUNKS + 1)
        .map_err(|e| e.to_string())?;
    if chunks.len() > SCAN_MAX_CHUNKS {
        return Err(format!(
            "Memory scan exceeds {SCAN_MAX_CHUNKS} chunks; narrow the indexed sources"
        ));
    }
    let source_ids = chunks
        .iter()
        .map(|chunk| chunk.source_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    check_cancelled(token)?;
    let sources = storage
        .memory_source_metadata(&source_ids)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|s| (s.id.clone(), s))
        .collect::<HashMap<_, _>>();
    chunks.retain(|c| {
        sources.get(&c.source_id).is_some_and(|s| {
            query
                .source_kind
                .as_ref()
                .is_none_or(|k| &s.source_kind == k)
        })
    });
    let allowed = chunks.iter().map(|c| c.id.as_str()).collect::<HashSet<_>>();
    let mut rankings: HashMap<String, Vec<MemoryChannel>> = HashMap::new();
    let mut warnings = Vec::new();
    let mut semantic_ready = false;
    if let Some(semantic) = semantic.filter(|s| {
        settings
            .embedding
            .as_ref()
            .and_then(|p| p.id().ok())
            .as_deref()
            == Some(s.profile_id.as_str())
    }) {
        let vectors = storage
            .memory_semantic_vectors(
                &semantic.profile_id,
                conversations,
                settings.index_actions,
                SCAN_MAX_CHUNKS + 1,
            )
            .map_err(|e| e.to_string())?;
        let mut scored = Vec::new();
        for vector in vectors {
            check_cancelled(token)?;
            if !allowed.contains(vector.chunk_id.as_str()) {
                continue;
            }
            let decoded = if vector.codec == embedding::EMBEDDING_CODEC
                && vector.dimensions == semantic.dimensions
            {
                embedding::decode(&vector.data, vector.dimensions)
            } else {
                Err("Stored semantic profile/codec is incompatible".into())
            };
            match decoded {
                Ok(values) => {
                    let score = semantic
                        .vector
                        .iter()
                        .zip(values.iter())
                        .map(|(a, b)| *a as f64 * *b as f64)
                        .sum::<f64>()
                        .clamp(-1.0, 1.0);
                    if score > 0.0 {
                        scored.push((vector.chunk_id.clone(), score));
                    }
                }
                Err(_) => {
                    storage
                        .memory_quarantine_vector(&vector)
                        .map_err(|e| e.to_string())?;
                    warnings.push(format!("Semantic vector {} is corrupt or incompatible and was excluded; rebuild or update the index",vector.chunk_id));
                }
            }
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        semantic_ready = !scored.is_empty();
        for (rank, (id, score)) in scored.into_iter().take(100).enumerate() {
            rankings.entry(id).or_default().push(MemoryChannel {
                channel: "semantic".into(),
                rank: rank + 1,
                score: Some(score),
            });
        }
        if !semantic_ready {
            warnings.push("No matching usable vectors in the selected semantic profile; used offline retrieval.".into());
        }
    }
    let offline_mode = if query.mode == SearchMode::Semantic && !semantic_ready {
        SearchMode::Hybrid
    } else {
        query.mode
    };
    if offline_mode != SearchMode::Lexical && offline_mode != SearchMode::Semantic {
        let expression = fts_expression(&query.query);
        if expression.is_empty() {
            let literal = query.query.trim();
            for (rank, chunk) in chunks
                .iter()
                .filter(|c| c.text.contains(literal))
                .take(100)
                .enumerate()
            {
                rankings
                    .entry(chunk.id.clone())
                    .or_default()
                    .push(MemoryChannel {
                        channel: "exact_text".into(),
                        rank: rank + 1,
                        score: None,
                    });
            }
        }
        if !expression.is_empty() {
            let ids = storage
                .memory_fts_rank(
                    &expression,
                    conversations,
                    settings.index_actions,
                    SCAN_MAX_CHUNKS,
                )
                .map_err(|e| e.to_string())?;
            for (rank, id) in ids
                .into_iter()
                .filter(|id| allowed.contains(id.as_str()))
                .take(100)
                .enumerate()
            {
                rankings.entry(id).or_default().push(MemoryChannel {
                    channel: "full_text".into(),
                    rank: rank + 1,
                    score: None,
                });
            }
        }
    }
    if offline_mode != SearchMode::FullText && offline_mode != SearchMode::Semantic {
        if let Ok(block) =
            crowquant::vectorize_text(&query.query).and_then(|v| crowquant::quantize(&v))
        {
            let mut scored = Vec::new();
            for chunk in &chunks {
                check_cancelled(token)?;
                if let Some(bytes) = &chunk.lexical_block {
                    match crowquant::deserialize(bytes).and_then(|b| {
                        if b.padded_dimension > crowquant::TEXT_VECTOR_DIMENSION as u32
                            || b.dimension != block.dimension
                        {
                            return Err("Unsupported lexical profile".into());
                        }
                        crowquant::compressed_cosine(&block, &b)
                    }) {
                        Ok(score) if score > 0.0 => scored.push((chunk.id.clone(), score)),
                        Ok(_) => {}
                        Err(_) => warnings.push(format!(
                            "Lexical vector {} is corrupt or incompatible; rebuild the index",
                            chunk.id
                        )),
                    }
                }
            }
            scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            for (rank, (id, score)) in scored.into_iter().take(100).enumerate() {
                rankings.entry(id).or_default().push(MemoryChannel {
                    channel: "lexical".into(),
                    rank: rank + 1,
                    score: Some(score),
                });
            }
        }
    }
    check_cancelled(token)?;
    let mut hits = Vec::new();
    for chunk in chunks {
        if let Some(channels) = rankings.remove(&chunk.id) {
            let source = &sources[&chunk.source_id];
            if source.state != "active"
                || !storage
                    .memory_source_active(&source.id)
                    .map_err(|e| e.to_string())?
            {
                continue;
            }
            let score = channels.iter().map(|r| 1.0 / (60.0 + r.rank as f64)).sum();
            hits.push(MemoryHit {
                chunk_id: chunk.id,
                source_id: source.id.clone(),
                source_kind: source.source_kind.clone(),
                origin_id: source.origin_id.clone(),
                title: source.title.clone(),
                authorship: source.authorship.clone(),
                text: chunk.text,
                created_at_ms: source.created_at_ms,
                start_byte: chunk.start_byte,
                end_byte: chunk.end_byte,
                score,
                channels,
            });
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.created_at_ms.cmp(&a.created_at_ms))
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
    });
    // A source appears once in a result packet; chunks remain available by offset.
    let mut seen = HashSet::new();
    hits.retain(|h| seen.insert(h.source_id.clone()));
    hits.truncate(query.limit);
    Ok(MemorySearchResult {
        hits,
        mode: query.mode,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fts_operators_are_terms_and_never_control_syntax() {
        assert_eq!(
            fts_expression("mirror\" OR telescope* NEAR(x)"),
            "\"mirror\" OR \"OR\" OR \"telescope\" OR \"NEAR\" OR \"x\""
        );
        assert_eq!(fts_expression("🌒"), "");
    }
}
