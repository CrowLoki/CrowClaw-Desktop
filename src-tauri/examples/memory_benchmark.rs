//! Measures the actual native offline index and query paths over synthetic data.
use crowclaw_desktop_lib::{
    agent::CancellationToken,
    memory::{MemoryQuery, MemoryService, MemorySettings, SearchMode, SCAN_MAX_CHUNKS},
    storage::{ConversationInput, Storage},
};
use rusqlite::params;
use serde_json::json;
use std::{error::Error, sync::Arc, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let count = std::env::args()
        .nth(1)
        .map(|v| v.parse::<usize>())
        .transpose()?
        .unwrap_or(SCAN_MAX_CHUNKS);
    if !(1..=SCAN_MAX_CHUNKS).contains(&count) {
        return Err("Count must be within the declared native scan bound".into());
    }
    let directory = tempfile::tempdir()?;
    let storage = Arc::new(Storage::open(directory.path())?);
    storage.create_conversation(&ConversationInput {
        id: "benchmark".into(),
        title: "Synthetic archive".into(),
        provider_profile_id: None,
    })?;
    let mut fixture = rusqlite::Connection::open(storage.database_path())?;
    let tx = fixture.transaction()?;
    let selected = count / 2;
    for i in 0..count {
        let text = if i == selected {
            "The calibration sentinel x7421 identifies the telescope mirror".to_string()
        } else {
            format!("Archive record {i} discusses garden weather and project scheduling")
        };
        tx.execute("INSERT INTO messages(id,conversation_id,ordinal,role,content,metadata_json,created_at_ms) VALUES(?1,'benchmark',?2,'user',?3,'null',?2)",params![format!("entry-{i}"),i as i64,text])?;
    }
    tx.commit()?;
    drop(fixture);
    let service = MemoryService::new(storage.clone());
    service.configure(MemorySettings::default())?;
    let token = CancellationToken::new();
    let start = Instant::now();
    let mut indexed = 0;
    loop {
        let report = service.sync(64, &token)?;
        if !report.warnings.is_empty() {
            return Err(format!("Index failed: {:?}", report.warnings).into());
        }
        indexed += report.indexed;
        if report.pending == 0 {
            break;
        }
    }
    let index_ms = start.elapsed().as_millis();
    let mut queries = Vec::new();
    for mode in [
        SearchMode::FullText,
        SearchMode::Lexical,
        SearchMode::Hybrid,
    ] {
        let start = Instant::now();
        let result = service.search(
            &MemoryQuery {
                query: "calibration sentinel x7421".into(),
                limit: 5,
                source_kind: None,
                mode,
            },
            &token,
        )?;
        if result
            .hits
            .first()
            .is_none_or(|h| h.origin_id != format!("entry-{selected}"))
        {
            return Err("Benchmark failed to retrieve the known source".into());
        }
        queries.push(json!({"mode":mode,"elapsedMs":start.elapsed().as_millis(),"topSourceCorrect":true,"returned":result.hits.len()}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"boundary":"synthetic native service benchmark; not UI latency","records":count,"indexed":indexed,"indexMs":index_ms,"queries":queries,"databaseBytes":std::fs::metadata(storage.database_path())?.len()})
        )?
    );
    Ok(())
}
