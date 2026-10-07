use crowclaw_desktop_lib::{
    agent::CancellationToken,
    memory::{
        EmbeddingProfile, EmbeddingProvider, MemoryQuery, MemoryService, MemorySettings, SearchMode,
    },
    storage::Storage,
};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

// Each standalone fixture is the sole embedding provider in its test. Separate
// runtimes starting providers concurrently hit reproducible loopback deadlines
// on the Windows runner. Shared-service queue concurrency is tested explicitly.
static FIXTURE_OWNER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Fixture {
    _owner: tokio::sync::MutexGuard<'static, ()>,
    url: String,
    calls: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(provider: EmbeddingProvider, mode: &str) -> Fixture {
    let owner = FIXTURE_OWNER.lock().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = calls.clone();
    let mode = mode.to_owned();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let requests = requests.clone();
            let mode = mode.clone();
            let provider = provider.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 4096];
                let body_start = loop {
                    let n = stream.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_string)
                            })
                            .unwrap()
                            .parse::<usize>()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break end + 4;
                        }
                    }
                };
                let path = String::from_utf8_lossy(&bytes[..body_start]);
                match provider {
                    EmbeddingProvider::OpenAi => assert!(path.starts_with("POST /v1/embeddings ")),
                    EmbeddingProvider::Ollama => assert!(path.starts_with("POST /api/embed ")),
                };
                assert!(!path.to_ascii_lowercase().contains("authorization:"));
                let input: Value = serde_json::from_slice(&bytes[body_start..]).unwrap();
                requests.fetch_add(1, Ordering::SeqCst);
                if mode == "hold" {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    return;
                }
                if mode == "redirect" {
                    let _=stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://example.com/embeddings\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                }
                let texts = input["input"].as_array().unwrap();
                assert!((1..=8).contains(&texts.len()));
                let vectors = texts
                    .iter()
                    .map(|t| {
                        let text = t.as_str().unwrap().to_lowercase();
                        if text.contains("car")
                            || text.contains("vehicle")
                            || text.contains("automobile")
                        {
                            json!([1.0, 0.0])
                        } else {
                            json!([0.0, 1.0])
                        }
                    })
                    .collect::<Vec<_>>();
                let output = match provider {
                    EmbeddingProvider::OpenAi => {
                        json!({"model":input["model"],"data":vectors.iter().enumerate().rev().map(|(index,v)|json!({"index":index,"embedding":v})).collect::<Vec<_>>()})
                    }
                    EmbeddingProvider::Ollama => {
                        assert_eq!(input["truncate"], false);
                        json!({"model":input["model"],"embeddings":vectors})
                    }
                };
                let output = if mode == "wrong_dimensions" {
                    json!({"model":input["model"],"data":[{"index":0,"embedding":[1.0]}]})
                } else {
                    output
                };
                let body = serde_json::to_vec(&output).unwrap();
                let header=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                let _ = stream.write_all(header.as_bytes()).await;
                let _ = stream.write_all(&body).await;
            });
        }
    });
    Fixture {
        _owner: owner,
        url,
        calls,
        task,
    }
}

fn profile(server: &Fixture, provider: EmbeddingProvider) -> EmbeddingProfile {
    EmbeddingProfile {
        base_url: if provider == EmbeddingProvider::OpenAi {
            format!("{}/v1", server.url)
        } else {
            server.url.clone()
        },
        provider,
        model: "fixture-embeddings".into(),
        dimensions: 2,
    }
}

fn open() -> (TempDir, Arc<Storage>, Arc<MemoryService>) {
    let dir = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(dir.path()).unwrap());
    let service = Arc::new(MemoryService::new(storage.clone()));
    service.configure(MemorySettings::default()).unwrap();
    (dir, storage, service)
}
fn query(text: &str) -> MemoryQuery {
    MemoryQuery {
        query: text.into(),
        limit: 5,
        source_kind: None,
        mode: SearchMode::Semantic,
    }
}

#[test]
fn profiles_reject_remote_hosts_credentials_and_invalid_dimensions() {
    for url in [
        "https://api.openai.com/v1",
        "http://192.168.1.2/v1",
        "http://127.0.0.1.example.com/v1",
        "http://key@127.0.0.1/v1",
        "file:///tmp/model",
        "http://127.0.0.1/v1?token=private",
    ] {
        let p = EmbeddingProfile {
            provider: EmbeddingProvider::OpenAi,
            base_url: url.into(),
            model: "fixture".into(),
            dimensions: 2,
        };
        assert!(p.validate().is_err(), "{url}");
    }
    let mut p = EmbeddingProfile {
        provider: EmbeddingProvider::OpenAi,
        base_url: "http://localhost:1234/v1".into(),
        model: "fixture".into(),
        dimensions: 2,
    };
    assert!(p.validate().is_ok());
    p.dimensions = 4097;
    assert!(p.validate().is_err());
    p.dimensions = 0;
    assert!(p.validate().is_err());
}

#[tokio::test]
async fn semantic_paraphrase_retrieval_persists_and_offline_search_survives_outage() {
    let server = fixture(EmbeddingProvider::OpenAi, "normal").await;
    let (dir, storage, service) = open();
    let car = service
        .remember("A motor vehicle needs regular maintenance")
        .unwrap();
    service
        .remember("Apples and oranges make fruit salad")
        .unwrap();
    service
        .configure(MemorySettings {
            embedding: Some(profile(&server, EmbeddingProvider::OpenAi)),
            ..MemorySettings::default()
        })
        .unwrap();
    let indexed = service
        .sync_semantic(&CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(indexed.indexed, 2, "{:?}", indexed.warnings);
    let result = service
        .search_async(&query("automobile"), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.hits[0].origin_id, car.id);
    assert!(result.hits[0]
        .channels
        .iter()
        .any(|c| c.channel == "semantic"));
    assert_eq!(service.status().unwrap().semantic.vectors, 2);
    drop(service);
    drop(storage);
    let reopened = MemoryService::new(Arc::new(Storage::open(dir.path()).unwrap()));
    assert_eq!(reopened.status().unwrap().semantic.vectors, 2);
    let result = reopened
        .search_async(&query("automobile"), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.hits[0].origin_id, car.id);
    drop(server);
    let result = reopened
        .search_async(&query("vehicle"), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.hits[0].origin_id, car.id);
    assert!(!result.warnings.is_empty());
    assert!(result.hits[0]
        .channels
        .iter()
        .all(|c| c.channel != "semantic"));
}

#[tokio::test]
async fn ollama_uses_native_batch_endpoint_and_validated_vectors() {
    let server = fixture(EmbeddingProvider::Ollama, "normal").await;
    let (_dir, _storage, service) = open();
    service.remember("car servicing").unwrap();
    service
        .configure(MemorySettings {
            embedding: Some(profile(&server, EmbeddingProvider::Ollama)),
            ..MemorySettings::default()
        })
        .unwrap();
    service
        .sync_semantic(&CancellationToken::new())
        .await
        .unwrap();
    let result = service
        .search_async(&query("automobile"), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(server.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn localhost_is_pinned_and_corrupt_vectors_are_quarantined() {
    let server = fixture(EmbeddingProvider::OpenAi, "normal").await;
    let (_dir, storage, service) = open();
    let note = service.remember("car servicing").unwrap();
    let mut p = profile(&server, EmbeddingProvider::OpenAi);
    p.base_url = p.base_url.replace("127.0.0.1", "localhost");
    service
        .configure(MemorySettings {
            embedding: Some(p),
            ..MemorySettings::default()
        })
        .unwrap();
    service
        .sync_semantic(&CancellationToken::new())
        .await
        .unwrap();
    let connection = rusqlite::Connection::open(storage.database_path()).unwrap();
    connection
        .execute(
            "UPDATE memory_vectors SET data=?1",
            [vec![0u8, 0, 192, 127, 0, 0, 0, 0]],
        )
        .unwrap();
    let result = service
        .search_async(&query("car"), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.hits[0].origin_id, note.id);
    assert!(result.warnings.iter().any(|w| w.contains("corrupt")));
    assert_eq!(service.status().unwrap().semantic.vectors, 0);
    service
        .sync_semantic(&CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(service.status().unwrap().semantic.vectors, 1);
    let export = storage.export_all().unwrap();
    assert_eq!(export.memory_embedding_profiles.len(), 1);
    assert_eq!(export.memory_vectors.len(), 1);
    storage
        .apply_retention_choice(crowclaw_desktop_lib::storage::RetentionChoice::Remove)
        .unwrap();
    assert_eq!(storage.stored_record_count().unwrap(), 0);
}

#[tokio::test]
async fn actual_agent_semantic_query_never_contacts_server_before_approval() {
    use crowclaw_desktop_lib::tools::{
        ApprovalDecision, ToolExecution, ToolExecutor, ToolOutput, ToolPolicy, ToolRequest,
    };
    let server = fixture(EmbeddingProvider::OpenAi, "normal").await;
    let (_dir, _storage, service) = open();
    let note = service.remember("car servicing").unwrap();
    service
        .configure(MemorySettings {
            embedding: Some(profile(&server, EmbeddingProvider::OpenAi)),
            ..MemorySettings::default()
        })
        .unwrap();
    service
        .sync_semantic(&CancellationToken::new())
        .await
        .unwrap();
    let before = server.calls.load(Ordering::SeqCst);
    let executor = ToolExecutor::new(ToolPolicy::default())
        .unwrap()
        .with_memory_backend(service);
    let denied = executor
        .propose(ToolRequest::SearchMemory {
            query: "automobile".into(),
            limit: 5,
        })
        .unwrap();
    assert_eq!(server.calls.load(Ordering::SeqCst), before);
    executor
        .resolve(
            &denied.approval_token,
            ApprovalDecision::Deny { reason: None },
        )
        .unwrap();
    executor
        .execute(&denied.approval_token, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(server.calls.load(Ordering::SeqCst), before);
    let approved = executor
        .propose(ToolRequest::SearchMemory {
            query: "automobile".into(),
            limit: 5,
        })
        .unwrap();
    executor
        .resolve(&approved.approval_token, ApprovalDecision::Approve)
        .unwrap();
    match executor
        .execute(&approved.approval_token, &CancellationToken::new())
        .await
        .unwrap()
    {
        ToolExecution::Executed {
            output: ToolOutput::MemorySearch { results, .. },
            ..
        } => {
            assert_eq!(results[0].id, note.id);
            assert!(results[0].provenance.as_ref().unwrap()["channels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["channel"] == "semantic"));
        }
        other => panic!("Unexpected search: {other:?}"),
    }
    assert_eq!(server.calls.load(Ordering::SeqCst), before + 1);
}

#[tokio::test]
async fn profile_change_does_not_mix_old_vectors() {
    let server = fixture(EmbeddingProvider::OpenAi, "normal").await;
    let (_dir, _storage, service) = open();
    service.remember("car servicing").unwrap();
    let mut p = profile(&server, EmbeddingProvider::OpenAi);
    service
        .configure(MemorySettings {
            embedding: Some(p.clone()),
            ..MemorySettings::default()
        })
        .unwrap();
    service
        .sync_semantic(&CancellationToken::new())
        .await
        .unwrap();
    p.model = "different-model".into();
    service
        .configure(MemorySettings {
            embedding: Some(p),
            ..MemorySettings::default()
        })
        .unwrap();
    assert_eq!(service.status().unwrap().semantic.vectors, 0);
    let result = service
        .search_async(&query("automobile"), &CancellationToken::new())
        .await
        .unwrap();
    assert!(result
        .hits
        .iter()
        .all(|h| h.channels.iter().all(|c| c.channel != "semantic")));
    service
        .sync_semantic(&CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(service.status().unwrap().semantic.vectors, 1);
}

#[tokio::test]
async fn cancellation_and_disable_discard_inflight_embeddings() {
    let server = fixture(EmbeddingProvider::OpenAi, "hold").await;
    let (_dir, _storage, service) = open();
    service.remember("car servicing").unwrap();
    service
        .configure(MemorySettings {
            embedding: Some(profile(&server, EmbeddingProvider::OpenAi)),
            ..MemorySettings::default()
        })
        .unwrap();
    let token = CancellationToken::new();
    let child = token.clone();
    let worker = service.clone();
    let task = tokio::spawn(async move { worker.sync_semantic(&child).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while server.calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    token.cancel();
    assert!(tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(service.status().unwrap().semantic.vectors, 0);
    let worker = service.clone();
    let task = tokio::spawn(async move { worker.sync_semantic(&CancellationToken::new()).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while server.calls.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    service.configure(MemorySettings::default()).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(service.status().unwrap().semantic.state, "disabled");
}

#[tokio::test]
async fn redirects_and_wrong_dimensions_fail_without_storing_vectors() {
    for mode in ["redirect", "wrong_dimensions"] {
        let server = fixture(EmbeddingProvider::OpenAi, mode).await;
        let (_dir, _storage, service) = open();
        service.remember("car servicing").unwrap();
        service
            .configure(MemorySettings {
                embedding: Some(profile(&server, EmbeddingProvider::OpenAi)),
                ..MemorySettings::default()
            })
            .unwrap();
        let report = service
            .sync_semantic(&CancellationToken::new())
            .await
            .unwrap();
        assert!(!report.warnings.is_empty());
        assert!(
            server.calls.load(Ordering::SeqCst) > 0,
            "The fixture must actually receive the request"
        );
        assert!(
            report.warnings.iter().any(|w| if mode == "redirect" {
                w.contains("HTTP 302")
            } else {
                w.contains("dimensions")
            }),
            "{:?}",
            report.warnings
        );
        assert_eq!(service.status().unwrap().semantic.vectors, 0);
        assert!(!service
            .search_async(&query("car"), &CancellationToken::new())
            .await
            .unwrap()
            .hits
            .is_empty());
    }
}

#[tokio::test]
async fn shared_service_serializes_requests_and_cancellation_releases_queue() {
    let server = fixture(EmbeddingProvider::OpenAi, "hold").await;
    let (_dir, _storage, service) = open();
    service.remember("car servicing").unwrap();
    service
        .configure(MemorySettings {
            embedding: Some(profile(&server, EmbeddingProvider::OpenAi)),
            ..MemorySettings::default()
        })
        .unwrap();
    let first = CancellationToken::new();
    let token = first.clone();
    let worker = service.clone();
    let a = tokio::spawn(async move { worker.sync_semantic(&token).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while server.calls.load(Ordering::SeqCst) != 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let second = CancellationToken::new();
    let token = second.clone();
    let worker = service.clone();
    let b = tokio::spawn(async move { worker.sync_semantic(&token).await });
    tokio::task::yield_now().await;
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    first.cancel();
    assert!(a.await.unwrap().is_err());
    tokio::time::timeout(Duration::from_secs(2), async {
        while server.calls.load(Ordering::SeqCst) != 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    second.cancel();
    assert!(b.await.unwrap().is_err());
    assert_eq!(service.status().unwrap().semantic.vectors, 0);
}

#[tokio::test]
async fn full_request_deadline_falls_back_without_returning_late_results() {
    let server = fixture(EmbeddingProvider::OpenAi, "hold").await;
    let (_dir, _storage, service) = open();
    let note = service.remember("car servicing").unwrap();
    service
        .configure(MemorySettings {
            embedding: Some(profile(&server, EmbeddingProvider::OpenAi)),
            ..MemorySettings::default()
        })
        .unwrap();
    let started = std::time::Instant::now();
    let result = service
        .search_async(&query("car"), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.hits[0].origin_id, note.id);
    assert!(result.warnings.iter().any(|w| w.contains("timed out")));
    assert!(started.elapsed() < Duration::from_secs(18));
    assert_eq!(service.status().unwrap().semantic.vectors, 0);
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
}
