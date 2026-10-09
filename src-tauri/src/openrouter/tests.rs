use super::*;
use crate::agent::{AssistantToolCall, ChatMessage, ToolDefinition};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

fn row(id: &str) -> Value {
    json!({
        "id": id, "name": "Fixture model", "context_length": 8192,
        "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]},
        "pricing": {"prompt": "0", "completion": "0"},
        "supported_parameters": ["tools", "reasoning"]
    })
}

fn request() -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "fixture/free".into(),
        messages: vec![ChatMessage::user("hello")],
        tools: vec![ToolDefinition {
            name: "read_text_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type":"object", "properties":{}}),
        }],
        temperature: None,
        max_tokens: None,
    }
}

impl OpenRouterProvider {
    fn fixture(base_url: String) -> Self {
        let mut provider = Self::new("fixture-secret".into(), "fixture/free".into(), None).unwrap();
        provider.client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(TIMEOUT)
            .build()
            .unwrap();
        provider.test_base_url = Some(base_url);
        provider
    }
}

fn free_model() -> FreeModel {
    parse_free_catalog(json!({"data": [row("fixture/free")]}))
        .unwrap()
        .remove(0)
}

#[test]
fn zero_token_prices_do_not_admit_media_generation_to_free_chat() {
    for outputs in [
        json!(["text", "audio"]),
        json!(["text", "image"]),
        json!(["text", "video"]),
        json!(["speech"]),
        json!(["image"]),
        json!(["text", null]),
    ] {
        let mut model = row("fixture/media");
        model["architecture"]["output_modalities"] = outputs;
        assert!(parse_free_catalog(json!({"data":[model]}))
            .unwrap()
            .is_empty());
    }
    let mut vision = row("fixture/vision");
    vision["architecture"]["input_modalities"] = json!(["text", "image", "audio", "video"]);
    assert_eq!(
        parse_free_catalog(json!({"data":[vision]})).unwrap().len(),
        1
    );
}

#[test]
fn invalid_json_cannot_be_repaired_into_a_free_catalog() {
    let valid = serde_json::to_string(&json!({"data":[row("fixture/free")]})).unwrap();
    let unquoted = valid.replace("\"pricing\":{", "\"pricing\":{0:0,");
    assert!(catalog_json(unquoted.as_bytes()).is_err());
}

#[test]
fn every_supplied_price_must_be_explicit_exact_zero() {
    for value in [
        json!(0),
        json!(0.0),
        json!("0"),
        json!("0.000"),
        json!("0e-999"),
        json!("0E+8"),
    ] {
        let mut model = row("fixture/free");
        model["pricing"] = json!({"prompt":value,"completion":0,"request":"0","image":0,"internal_reasoning":0,"input_cache_read":0});
        assert_eq!(
            parse_free_catalog(json!({"data":[model]})).unwrap().len(),
            1
        );
    }
    for value in [
        json!("1e-999"),
        json!("0.00000001"),
        json!(1),
        json!(-1),
        json!("-0"),
        json!("-0.0"),
        json!("NaN"),
        json!("Infinity"),
        json!(""),
        json!("unknown"),
        json!(" 0"),
        json!("0 "),
        json!("+0"),
        json!("00"),
        json!("0."),
        json!("0e"),
        json!(false),
        json!(null),
        json!({}),
        json!([]),
    ] {
        for field in ["prompt", "completion", "request", "image", "future_price"] {
            let mut model = row("fixture/free");
            model["pricing"][field] = value.clone();
            assert!(
                parse_free_catalog(json!({"data":[model]}))
                    .unwrap()
                    .is_empty(),
                "field {field} value {value}"
            );
        }
    }
    for prices in [
        json!({}),
        json!({"prompt":0}),
        json!({"completion":0}),
        json!(null),
    ] {
        let mut model = row("fixture/free");
        model["pricing"] = prices;
        assert!(parse_free_catalog(json!({"data":[model]}))
            .unwrap()
            .is_empty());
    }
}

#[test]
fn wire_numbers_cannot_underflow_into_free_prices() {
    for literal in [
        "1e-999",
        "-1e-999",
        "1.0e-9999",
        "0.0000000000000000000000001",
    ] {
        let body = json!({"data":[row("fixture/free")]})
            .to_string()
            .replace("\"prompt\":\"0\"", &format!("\"prompt\":{literal}"));
        let parsed = catalog_json(body.as_bytes()).unwrap();
        assert_eq!(parsed["data"][0]["pricing"]["prompt"], literal);
        assert!(parse_free_catalog(parsed).unwrap().is_empty());
    }
    let body = json!({"data":[row("fixture/free")]})
        .to_string()
        .replace("\"prompt\":\"0\"", "\"prompt\":0e-999");
    assert_eq!(
        parse_free_catalog(catalog_json(body.as_bytes()).unwrap()).unwrap()[0].context_length,
        8192
    );
    for body in [
        b"{\"data\":[01]}".as_slice(),
        b"{\"data\":[0.]}".as_slice(),
        b"{broken}".as_slice(),
    ] {
        assert!(catalog_json(body).is_err());
    }
}

#[test]
fn catalog_preserves_live_order_requires_text_and_does_not_invent_efforts() {
    let mut audio = row("audio");
    audio["architecture"]["output_modalities"] = json!(["audio"]);
    let mut image_only = row("image");
    image_only["architecture"]["input_modalities"] = json!(["image"]);
    let mut supported = row("alpha");
    supported["supported_reasoning_efforts"] = json!(["low", "high"]);
    let models =
        parse_free_catalog(json!({"data":[row("zeta"),audio,image_only,supported]})).unwrap();
    assert_eq!(
        models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["zeta", "alpha"]
    );
    assert!(models[0].reasoning_efforts.is_empty());
    assert_eq!(models[1].reasoning_efforts, ["low", "high"]);
    let wire = serde_json::to_value(&models[0]).unwrap();
    assert_eq!(wire["contextLength"], 8192);
    assert!(wire.get("inputModalities").is_some());
    for bad in [
        json!({}),
        json!({"data":null}),
        json!({"data":{}}),
        json!({"error":{"message":"fixture-secret"},"data":[]}),
    ] {
        let error = parse_free_catalog(bad).unwrap_err();
        assert!(!format!("{error:?}").contains("fixture-secret"));
    }
    assert!(parse_free_catalog(json!({"data":[]})).unwrap().is_empty());
}

#[test]
fn request_is_free_only_and_preserves_history_when_new_tools_are_omitted() {
    let provider = OpenRouterProvider::fixture("http://127.0.0.1:9/v1".into());
    let mut model = free_model();
    let body = provider.request_body(request(), &model).unwrap();
    assert_eq!(
        body["provider"],
        json!({"max_price":{"prompt":0,"completion":0,"request":0,"image":0},"allow_fallbacks":false,"require_parameters":true})
    );
    assert_eq!(body["plugins"], json!([]));
    assert!(body.get("models").is_none());
    assert!(body.get("route").is_none());
    assert!(body.get("reasoning").is_none());
    assert_eq!(body["tools"].as_array().unwrap().len(), 1);
    model.supported_parameters.clear();
    let mut req = request();
    req.messages.push(ChatMessage::assistant_with_tool_calls(
        None,
        vec![AssistantToolCall {
            id: "call-1".into(),
            name: "read_text_file".into(),
            arguments: json!({"path":"fixture.txt"}),
        }],
    ));
    req.messages.push(ChatMessage::tool(
        "call-1",
        "read_text_file",
        "fixture result",
    ));
    let body = provider.request_body(req, &model).unwrap();
    assert!(body.get("tools").is_none());
    assert!(body.get("tool_choice").is_none());
    assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "call-1");
    assert_eq!(body["messages"][2]["tool_call_id"], "call-1");
    assert_eq!(body["messages"][2]["content"], "fixture result");
}

#[test]
fn image_raw_file_reasoning_and_model_capability_boundaries() {
    let mut provider = OpenRouterProvider::fixture("http://127.0.0.1:9/v1".into());
    let mut model = free_model();
    let mut req = request();
    req.messages[0].attachments.push(AttachmentContent::Image {
        name: "fixture.png".into(),
        media_type: "image/png".into(),
        data_base64: "YQ==".into(),
    });
    assert!(matches!(
        provider.request_body(req.clone(), &model),
        Err(ProviderError::Unsupported { .. })
    ));
    model.input_modalities.push("image".into());
    let body = provider.request_body(req.clone(), &model).unwrap();
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["url"],
        "data:image/png;base64,YQ=="
    );
    req.messages[0].attachments[0] = AttachmentContent::Image {
        name: "fixture.png".into(),
        media_type: "image/png".into(),
        data_base64: "invalid".into(),
    };
    assert!(provider.request_body(req.clone(), &model).is_err());
    req.messages[0].attachments[0] = AttachmentContent::File {
        name: "fixture.pdf".into(),
        media_type: "application/pdf".into(),
        data_base64: "YQ==".into(),
    };
    assert!(matches!(
        provider.request_body(req, &model),
        Err(ProviderError::Unsupported { .. })
    ));
    provider.reasoning_effort = Some("high".into());
    assert!(provider.request_body(request(), &model).is_err());
    model.reasoning_efforts.push("high".into());
    assert_eq!(
        provider.request_body(request(), &model).unwrap()["reasoning"],
        json!({"effort":"high"})
    );
    let mut mismatch = request();
    mismatch.model = "another-model".into();
    assert!(provider.request_body(mismatch, &model).is_err());
    assert!(!format!("{provider:?}").contains("fixture-secret"));
    assert!(format!("{provider:?}").contains("redacted"));
    for bad in ["", " ", "fixture\r\nsecret"] {
        assert!(OpenRouterProvider::new(bad.into(), "fixture/free".into(), None).is_err());
    }
}

async fn server(replies: Vec<(u16, String)>) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, body) in replies {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
                    .await
                    .unwrap()
                    .unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|s| s.strip_prefix("content-length: "))
                        .map(|s| s.parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(bytes).unwrap());
            let response = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            // Oversize/error responses can be deliberately dropped before reading.
            let _ = stream.write_all(response.as_bytes()).await;
        }
        requests
    });
    (format!("http://{addr}/v1"), handle)
}

fn catalog() -> String {
    json!({"data":[row("fixture/free")]}).to_string()
}
fn completion() -> String {
    json!({"model":"actual/reported-model","choices":[{"message":{"content":"fixture reply"},"finish_reason":"stop"}]}).to_string()
}

#[tokio::test]
async fn public_and_authenticated_catalog_use_distinct_routes() {
    for key in [None, Some("fixture-secret")] {
        let (url, received) = server(vec![(200, catalog())]).await;
        let provider = OpenRouterProvider::fixture(url.clone());
        let catalog = fetch_with(&provider.client, &url, key, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(catalog.models.len(), 1);
        assert!(catalog.fetched_at_ms > 0);
        let requests = received.await.unwrap();
        assert!(requests[0].starts_with(if key.is_some() {
            "GET /v1/models/user "
        } else {
            "GET /v1/models "
        }));
        assert_eq!(
            requests[0]
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-secret"),
            key.is_some()
        );
    }
}

#[tokio::test]
async fn every_completion_rechecks_price_and_never_posts_after_price_change() {
    let mut paid = row("fixture/free");
    paid["pricing"]["request"] = json!("0.01");
    let (url, received) = server(vec![
        (200, catalog()),
        (200, completion()),
        (200, json!({"data":[paid]}).to_string()),
    ])
    .await;
    let provider = OpenRouterProvider::fixture(url);
    let result = provider
        .complete(request(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.model.as_deref(), Some("actual/reported-model"));
    let error = provider
        .complete(request(), &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::InvalidConfiguration { .. }));
    assert!(error.to_string().contains("no longer verified free"));
    let requests = received.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET /v1/models/user "));
    assert!(requests[1].starts_with("POST /v1/chat/completions "));
    assert!(requests[2].starts_with("GET /v1/models/user "));
    assert!(requests.iter().all(|r| r
        .to_ascii_lowercase()
        .contains("authorization: bearer fixture-secret")));
    let body: Value = serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        body["provider"]["max_price"],
        json!({"prompt":0,"completion":0,"request":0,"image":0})
    );
    assert_eq!(body["provider"]["allow_fallbacks"], false);
    assert_eq!(body["plugins"], json!([]));
}

#[tokio::test]
async fn http_errors_are_plain_and_never_include_provider_body_or_key() {
    for status in [302, 401, 403, 402, 429, 404, 503] {
        let (url, received) = server(vec![(status, "private body fixture-secret".into())]).await;
        let provider = OpenRouterProvider::fixture(url);
        let error = provider
            .complete(request(), &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::HttpStatus { status: code, .. } if code == status));
        let detail = format!("{error:?} {error}");
        assert!(!detail.contains("private body"));
        assert!(!detail.contains("fixture-secret"));
        assert_eq!(received.await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn malformed_catalog_oversized_body_and_completion_errors_fail_closed() {
    for body in [
        "not json fixture-secret".into(),
        "{\"data\":null}".into(),
        "x".repeat(BODY_LIMIT + 1),
    ] {
        let oversized = body.len() > BODY_LIMIT;
        let (url, received) = server(vec![(200, body)]).await;
        let provider = OpenRouterProvider::fixture(url);
        let error = provider
            .complete(request(), &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(!format!("{error:?}").contains("fixture-secret"));
        if oversized {
            assert!(matches!(error, ProviderError::ResponseTooLarge { .. }));
        }
        assert_eq!(received.await.unwrap().len(), 1);
    }
    let (url, received) = server(vec![
        (200, catalog()),
        (
            200,
            json!({"error":{"code":429,"message":"fixture-secret"}}).to_string(),
        ),
    ])
    .await;
    let error = OpenRouterProvider::fixture(url)
        .complete(request(), &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ProviderError::HttpStatus { status: 429, .. }
    ));
    assert!(!format!("{error:?}").contains("fixture-secret"));
    assert_eq!(received.await.unwrap().len(), 2);
}

#[tokio::test]
async fn cancelled_token_prevents_network_and_cancellation_interrupts_pending_headers() {
    let token = CancellationToken::new();
    token.cancel();
    let provider = OpenRouterProvider::fixture("http://127.0.0.1:9/v1".into());
    assert!(matches!(
        provider.complete(request(), &token).await,
        Err(ProviderError::Cancelled)
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider =
        OpenRouterProvider::fixture(format!("http://{}/v1", listener.local_addr().unwrap()));
    let token = CancellationToken::new();
    let cancel = token.clone();
    let pending = tokio::spawn(async move { provider.complete(request(), &token).await });
    let (_connection, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    cancel.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap(),
        Err(ProviderError::Cancelled)
    ));
}

#[tokio::test]
async fn cancellation_interrupts_body_and_chunked_body_obeys_size_limit() {
    for oversized in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider =
            OpenRouterProvider::fixture(format!("http://{}/v1", listener.local_addr().unwrap()));
        let token = CancellationToken::new();
        let cancel = token.clone();
        let pending = tokio::spawn(async move { provider.complete(request(), &token).await });
        let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut request_bytes = Vec::new();
        while !request_bytes.windows(4).any(|w| w == b"\r\n\r\n") {
            let mut buffer = [0u8; 1024];
            let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
                .await
                .unwrap()
                .unwrap();
            assert!(n > 0);
            request_bytes.extend_from_slice(&buffer[..n]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
            .await
            .unwrap();
        if oversized {
            let chunk = vec![b'x'; BODY_LIMIT + 1];
            stream
                .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                .await
                .unwrap();
            let _ = stream.write_all(&chunk).await;
            let _ = stream.write_all(b"\r\n0\r\n\r\n").await;
        } else {
            stream.write_all(b"1\r\n{\r\n").await.unwrap();
            cancel.cancel();
        }
        let error = tokio::time::timeout(Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        if oversized {
            assert!(matches!(error, ProviderError::ResponseTooLarge { .. }));
        } else {
            assert!(matches!(error, ProviderError::Cancelled));
        }
    }
}
