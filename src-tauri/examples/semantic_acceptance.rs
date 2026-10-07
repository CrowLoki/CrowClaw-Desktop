//! Explicit local-runtime acceptance of the same native service used by Tauri.
//! Uses only synthetic text in a disposable database; no user profile is read.
use crowclaw_desktop_lib::{
    agent::CancellationToken,
    memory::{
        EmbeddingProfile, EmbeddingProvider, MemoryQuery, MemoryService, MemorySettings, SearchMode,
    },
    storage::Storage,
};
use serde_json::json;
use std::{error::Error, sync::Arc, time::Instant};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(
            "Usage: semantic_acceptance <loopback-base-url> <installed-model-id> <dimensions>"
                .into(),
        );
    }
    let profile = EmbeddingProfile {
        provider: EmbeddingProvider::OpenAi,
        base_url: args[0].clone(),
        model: args[1].clone(),
        dimensions: args[2].parse()?,
    };
    profile.validate()?;
    let dir = tempfile::tempdir()?;
    let storage = Arc::new(Storage::open(dir.path())?);
    let service = MemoryService::new(storage.clone());
    service.configure(MemorySettings {
        embedding: Some(profile.clone()),
        ..MemorySettings::default()
    })?;
    let car = service.remember("A motor vehicle should undergo regular mechanical servicing.")?;
    service.remember("Raised vegetable beds require regular irrigation.")?;
    service.remember("A pastry recipe combines flour, butter and sugar.")?;
    let started = Instant::now();
    let report = service.sync_semantic(&CancellationToken::new()).await?;
    if report.indexed != 3 {
        return Err(format!("Semantic indexing did not complete: {:?}", report.warnings).into());
    }
    let query = MemoryQuery {
        query: "When should an automobile get a tune-up?".into(),
        limit: 3,
        source_kind: None,
        mode: SearchMode::Semantic,
    };
    let result = service
        .search_async(&query, &CancellationToken::new())
        .await?;
    if result.hits.first().is_none_or(|h| {
        h.origin_id != car.id || !h.channels.iter().any(|c| c.channel == "semantic")
    }) {
        return Err(format!(
            "Real-model paraphrase retrieval did not rank the expected source: {:?}",
            result
        )
        .into());
    }
    let native_vectors = service.status()?.semantic.vectors;
    drop(service);
    drop(storage);
    let reopened = MemoryService::new(Arc::new(Storage::open(dir.path())?));
    let after = reopened
        .search_async(&query, &CancellationToken::new())
        .await?;
    if after.hits.first().is_none_or(|h| h.origin_id != car.id) {
        return Err("Recall after native storage reopen did not preserve ranking".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"accepted":true,"boundary":"native service with real local model; not installed UI acceptance","profile":profile,"codec":"f32le-normalized-v1","indexed":report.indexed,"storedVectors":native_vectors,"paraphraseTopSourceCorrect":true,"restartRankingPreserved":true,"elapsedMs":started.elapsed().as_millis(),"channels":result.hits[0].channels,"topScore":result.hits[0].score,"warnings":result.warnings})
        )?
    );
    Ok(())
}
