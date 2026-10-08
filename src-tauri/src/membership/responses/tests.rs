use super::*;
use crate::agent::ToolDefinition;

#[test]
fn compact_terminal_uses_completed_items_without_accepting_partial_or_failed_streams() {
    let item = text_output("Completed item response")[0].clone();
    let item_event = json!({"type":"response.output_item.done","output_index":0,"item":item});
    let item_bytes = format!("data: {item_event}\n\n");
    let mut decoder = ResponseStream::new(selection());
    assert!(decoder.push(item_bytes.as_bytes()).unwrap().is_none());
    let terminal = format!("data: {}\n\n", completed(json!([])));
    let result = decoder.push(terminal.as_bytes()).unwrap().unwrap();
    assert_eq!(
        result.message.content.as_deref(),
        Some("Completed item response")
    );
    let mut unfinished = ResponseStream::new(selection());
    unfinished.push(item_bytes.as_bytes()).unwrap();
    assert!(unfinished.finish().is_err());
    let mut failed = ResponseStream::new(selection());
    failed.push(item_bytes.as_bytes()).unwrap();
    assert!(failed.push(b"data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"server_error\"}}}\n\n").is_err());
    let mut duplicate = ResponseStream::new(selection());
    duplicate.push(item_bytes.as_bytes()).unwrap();
    assert!(duplicate.push(item_bytes.as_bytes()).is_err());
    let mut sparse = ResponseStream::new(selection());
    sparse
        .push(
            format!(
                "data: {}\n\n",
                json!({"type":"response.output_item.done","output_index":1,"item":item})
            )
            .as_bytes(),
        )
        .unwrap();
    assert!(sparse.push(terminal.as_bytes()).is_err());
}

#[test]
fn missing_stream_content_type_accepts_only_valid_completed_events() {
    assert!(stream_content_type_supported(None));
    for media in ["text/event-stream", "Text/Event-Stream; charset=utf-8"] {
        assert!(stream_content_type_supported(Some(&media.parse().unwrap())));
    }
    for media in [
        "application/json",
        "text/html",
        "text/event-stream-invalid",
        "",
    ] {
        assert!(!stream_content_type_supported(Some(
            &media.parse().unwrap()
        )));
    }
    let event = completed(text_output("Headerless membership response"));
    let stream = format!("data: {event}\n\n");
    let mut decoder = ResponseStream::new(selection());
    let result = decoder.push(stream.as_bytes()).unwrap().unwrap();
    assert_eq!(
        result.message.content.as_deref(),
        Some("Headerless membership response")
    );
    for non_stream in [
        serde_json::to_vec(&event["response"]).unwrap(),
        b"<html>not a response</html>".to_vec(),
    ] {
        let mut decoder = ResponseStream::new(selection());
        assert!(decoder.push(&non_stream).unwrap().is_none());
        assert!(decoder.finish().is_err());
    }
}

#[test]
fn unexpected_response_diagnostics_do_not_echo_credentials_or_body_text() {
    for (body, shape) in [
        (
            br#"{"error":{"message":"SECRET"}}"#.as_slice(),
            "JSON error",
        ),
        (
            br#"{"object":"response","output":"SECRET"}"#.as_slice(),
            "JSON response object",
        ),
        (b"data: SECRET\n\n".as_slice(), "event-stream body"),
        (b"<html>SECRET</html>".as_slice(), "unrecognized body"),
    ] {
        let message = unexpected_response(200, "JSON", body);
        assert!(message.contains(shape));
        assert!(message.contains("HTTP 200"));
        assert!(!message.contains("SECRET"));
    }
}

#[test]
fn rotation_preserves_live_session_but_reconnect_and_signout_invalidate_it() {
    use crate::membership::tests::{add, protected};
    let directory = tempfile::TempDir::new().unwrap();
    let storage = Arc::new(crate::storage::Storage::open(directory.path()).unwrap());
    let account = add(&storage, "existing-client", "Personal");
    let service = Arc::new(MembershipService::new(storage.clone()).unwrap());
    let selected = MembershipSelection {
        account_id: account.id.clone(),
        model: "offered-model".into(),
        reasoning_effort: None,
    };
    let running = MembershipProvider::new(service.clone(), selected.clone()).unwrap();
    running.check_connection().unwrap();
    let rotated = storage
        .membership_replace_credentials(
            &account.id,
            account.credential_version,
            &protected(&account.identity, "ROTATED"),
        )
        .unwrap();
    assert_ne!(rotated.credential_version, account.credential_version);
    running.check_connection().unwrap();
    let reconnected = storage
        .membership_reconnect(
            &account.id,
            rotated.credential_version,
            "Personal",
            &protected(&account.identity, "RECONNECTED"),
        )
        .unwrap();
    assert!(matches!(
        running.check_connection(),
        Err(ProviderError::Cancelled)
    ));
    let next = MembershipProvider::new(service, selected).unwrap();
    next.check_connection().unwrap();
    storage
        .membership_clear_credentials(&account.id, reconnected.credential_version)
        .unwrap();
    assert!(matches!(
        next.check_connection(),
        Err(ProviderError::Cancelled)
    ));
}

fn selection() -> MembershipSelection {
    MembershipSelection {
        account_id: "synthetic-account".into(),
        model: "offered-model".into(),
        reasoning_effort: Some("high".into()),
    }
}
fn request(messages: Vec<ChatMessage>) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "offered-model".into(),
        messages,
        tools: Vec::new(),
        temperature: None,
        max_tokens: None,
    }
}
fn completed(output: Value) -> Value {
    json!({"type":"response.completed","response":{"id":"synthetic-response","model":"reported-model-version","status":"completed","output":output,"usage":{"input_tokens":4,"output_tokens":6,"total_tokens":10}}})
}
fn text_output(text: &str) -> Value {
    json!([{"id":"synthetic-message","type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":text,"annotations":[]}]}])
}

#[test]
fn sends_the_selected_account_model_and_effort_without_server_storage_or_api_key_fallback() {
    let mut input = request(vec![
        ChatMessage::system("App instructions"),
        ChatMessage::user("Read my selected file"),
    ]);
    input.tools.push(ToolDefinition{name:"read_file".into(),description:"Approved read".into(),parameters:json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})});
    let body = request_body(&input, &selection()).unwrap();
    assert_eq!(body["model"], "offered-model");
    assert_eq!(body["reasoning"]["effort"], "high");
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert!(body.get("previous_response_id").is_none());
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["name"], "read_file");
    assert_eq!(body["input"][0]["role"], "developer");
    let mut wrong = selection();
    wrong.model = "another-model".into();
    assert!(request_body(&input, &wrong).is_err());
}

#[test]
fn byte_split_unicode_stream_requires_real_terminal_completion() {
    let mut decoder = ResponseStream::new(selection());
    let event = completed(text_output("こんにちは 🐦"));
    let bytes = format!("event: response.completed\r\ndata: {event}\r\n\r\n").into_bytes();
    let mut result = None;
    for byte in &bytes {
        if let Some(completion) = decoder.push(&[*byte]).unwrap() {
            result = Some(completion);
        }
    }
    let result = result.unwrap();
    assert_eq!(result.message.content.as_deref(), Some("こんにちは 🐦"));
    assert_eq!(result.model.as_deref(), Some("reported-model-version"));
    assert_eq!(result.usage.unwrap().total_tokens, 10);
    let mut incomplete = ResponseStream::new(selection());
    incomplete
        .push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"not complete\"}\n\n")
        .unwrap();
    assert!(incomplete.finish().is_err());
    let mut done = ResponseStream::new(selection());
    assert!(done.push(b"data: [DONE]\n\n").is_err());
}

#[test]
fn failure_after_text_is_not_success_and_error_bodies_are_redacted() {
    let mut decoder = ResponseStream::new(selection());
    decoder
        .push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial answer\"}\n\n")
        .unwrap();
    let error = decoder.push(b"data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\",\"message\":\"SYNTHETIC-SECRET\"}}}\n\n").unwrap_err();
    assert!(error.to_string().contains("plan limit"));
    assert!(!error.to_string().contains("SYNTHETIC-SECRET"));
    let mut other = ResponseStream::new(selection());
    let error = other.push(b"data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"server_error\"}}}\n\n").unwrap_err();
    assert!(!error.to_string().contains("limiting requests"));
    let mut unfinished = ResponseStream::new(selection());
    assert!(unfinished
        .push(b"data: {\"type\":\"response.incomplete\"}\n\n")
        .is_err());
}

#[test]
fn carries_encrypted_reasoning_and_original_call_id_through_the_approved_tool_loop() {
    let output = json!([
        {"id":"reasoning-id","type":"reasoning","encrypted_content":"synthetic-encrypted-reasoning","summary":[]},
        {"id":"function-item","type":"function_call","call_id":"original-call","name":"read_file","arguments":"{\"path\":\"selected.txt\"}","status":"completed"}
    ]);
    let completion =
        completed_response(&completed(output.clone())["response"], &selection()).unwrap();
    assert_eq!(completion.message.tool_calls[0].id, "original-call");
    assert_eq!(
        completion.message.tool_calls[0].arguments,
        json!({"path":"selected.txt"})
    );
    // The existing runtime owns approval; the provider only returns the call.
    let history = vec![
        ChatMessage::user("Read the selected file"),
        completion.message,
        ChatMessage::tool("original-call", "read_file", "approved file contents"),
    ];
    let body = request_body(&request(history.clone()), &selection()).unwrap();
    assert_eq!(body["input"][1], output[0]);
    assert_eq!(body["input"][2], output[1]);
    assert_eq!(body["input"][3]["call_id"], "original-call");
    assert_eq!(body["input"][3]["type"], "function_call_output");
    let serialized = serde_json::to_string(&history).unwrap();
    let restored: Vec<ChatMessage> = serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        request_body(&request(restored), &selection()).unwrap(),
        body
    );
    let mut other = selection();
    other.account_id = "another-account".into();
    assert!(request_body(&request(history), &other).is_err());
}

#[test]
fn refuses_invalid_tool_arguments_duplicate_calls_wrong_roles_and_oversized_streams() {
    let base = json!({"type":"function_call","call_id":"call","name":"read_file","arguments":"{}"});
    assert!(completed_response(
        &completed(json!([base.clone(), base.clone()]))["response"],
        &selection()
    )
    .is_err());
    let mut invalid_args = base;
    invalid_args["arguments"] = json!("not-json");
    assert!(
        completed_response(&completed(json!([invalid_args]))["response"], &selection()).is_err()
    );
    let mut wrong_role = text_output("text");
    wrong_role[0]["role"] = json!("developer");
    assert!(completed_response(&completed(wrong_role)["response"], &selection()).is_err());
    let mut decoder = ResponseStream::new(selection());
    assert!(matches!(
        decoder.push(&vec![b'x'; RESPONSE_LIMIT + 1]),
        Err(ProviderError::ResponseTooLarge { .. })
    ));
}

#[test]
fn terminal_event_at_eof_is_parsed_without_counting_buffered_bytes_twice() {
    let mut decoder = ResponseStream::new(selection());
    let bytes = format!("data: {}", completed(text_output("complete"))).into_bytes();
    decoder.push(&bytes).unwrap();
    assert_eq!(decoder.consumed, bytes.len());
    assert_eq!(
        decoder.finish().unwrap().message.content.as_deref(),
        Some("complete")
    );
}
