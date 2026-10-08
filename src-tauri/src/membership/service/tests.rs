use super::*;
use serde_json::json;
use tempfile::TempDir;

fn saved_account(storage: &Storage) -> MembershipAccount {
    let identity = crate::membership::MembershipIdentity {
        provider: "chatgpt".into(),
        issuer: protocol::ISSUER.into(),
        subject: "synthetic-user".into(),
        client_id: "oaiapp_fixture".into(),
        host_id: storage.membership_host_id().unwrap(),
        email: None,
    };
    let credentials = MembershipCredentials {
        issuer: identity.issuer.clone(),
        subject: identity.subject.clone(),
        client_id: identity.client_id.clone(),
        ext_agent_host_id: identity.host_id.clone(),
        id_token: "synthetic-id".into(),
        access_token: "synthetic-access".into(),
        refresh_token: "synthetic-refresh".into(),
        token_type: "Bearer".into(),
        scopes: protocol::SCOPE
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        access_expires_at_ms: (protocol::unix_seconds().unwrap() as i64 + 3600) * 1000,
    };
    storage
        .membership_add_account(
            "Personal",
            &protect_credentials(&identity, &credentials).unwrap(),
        )
        .unwrap()
}

async fn write_response(stream: &mut TcpStream, status: u16, body: &str) {
    stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
}

#[tokio::test]
async fn catalog_refresh_is_serialized_across_instances_through_publication() {
    let directory = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(directory.path()).unwrap());
    let account = saved_account(&storage);
    let first = Arc::new(MembershipService::new(storage.clone()).unwrap());
    let second = Arc::new(
        MembershipService::new(Arc::new(Storage::open(directory.path()).unwrap())).unwrap(),
    );
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let url = format!("http://{}/models", listener.local_addr().unwrap());
    let (first_seen_tx, first_seen) = tokio::sync::oneshot::channel();
    let (second_seen_tx, mut second_seen) = tokio::sync::oneshot::channel();
    let (release_tx, release) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut older, _) = listener.accept().await.unwrap();
        read_headers(&mut older).await.unwrap();
        first_seen_tx.send(()).unwrap();
        let older_reply = tokio::spawn(async move {
            release.await.unwrap();
            write_response(
                &mut older,
                200,
                r#"{"models":[{"slug":"older","display_name":"Older","visibility":"list"}]}"#,
            )
            .await;
        });
        let (mut newer, _) = listener.accept().await.unwrap();
        read_headers(&mut newer).await.unwrap();
        second_seen_tx.send(()).unwrap();
        write_response(
            &mut newer,
            200,
            r#"{"models":[{"slug":"newer","display_name":"Newer","visibility":"list"}]}"#,
        )
        .await;
        older_reply.await.unwrap();
    });
    let id = account.id.clone();
    let first_url = url.clone();
    let older = tokio::spawn(async move {
        first
            .refresh_catalog_request(&id, first.client.get(first_url))
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), first_seen)
        .await
        .unwrap()
        .unwrap();
    let id = account.id.clone();
    let newer = tokio::spawn(async move {
        second
            .refresh_catalog_request(&id, second.client.get(url))
            .await
    });
    let overlapped = tokio::time::timeout(Duration::from_millis(150), &mut second_seen)
        .await
        .is_ok();
    release_tx.send(()).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), older)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .catalog
            .unwrap()
            .models[0]
            .slug,
        "older"
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), newer)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .catalog
            .unwrap()
            .models[0]
            .slug,
        "newer"
    );
    server.await.unwrap();
    assert!(
        !overlapped,
        "A second catalog request started before the first published"
    );
    assert_eq!(
        storage
            .membership_account(&account.id)
            .unwrap()
            .catalog
            .unwrap()
            .models[0]
            .slug,
        "newer"
    );
}

#[tokio::test]
async fn revocation_requires_the_documented_empty_200_response() {
    for (status, body, accepted) in [
        (200, "", true),
        (200, "SYNTHETIC unexpected body", false),
        (204, "", false),
    ] {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}/revoke", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_headers(&mut stream).await.unwrap();
            write_response(&mut stream, status, body).await;
        });
        let response = Client::new().get(url).send().await.unwrap();
        let result = confirm_revocation(response, &CancellationToken::new()).await;
        assert_eq!(result.as_ref().ok().copied(), Some(accepted));
        if let Err(error) = result {
            assert!(!error.contains("SYNTHETIC"));
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn revocation_retries_a_truncated_response_before_confirming_success() {
    let directory = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(directory.path()).unwrap());
    let account = saved_account(&storage);
    let (record, _) = storage
        .membership_protected_credentials(&account.id)
        .unwrap();
    let credentials = open_credentials(&record).unwrap();
    let service = MembershipService::new(storage).unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let url = format!("http://{}/revoke", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        read_headers(&mut first).await.unwrap();
        first
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nx")
            .await
            .unwrap();
        drop(first);
        let (mut second, _) = listener.accept().await.unwrap();
        read_headers(&mut second).await.unwrap();
        write_response(&mut second, 200, "").await;
    });
    tokio::time::timeout(
        Duration::from_secs(3),
        service.revoke_at(&credentials, &CancellationToken::new(), &url),
    )
    .await
    .unwrap()
    .unwrap();
    server.await.unwrap();
}

#[test]
fn callback_request_checks_local_host_method_path_and_duplicate_hosts() {
    let redirect = "http://127.0.0.1:45678/auth/callback";
    let request = b"GET /auth/callback?code=synthetic&state=synthetic HTTP/1.1\r\nHost: 127.0.0.1:45678\r\n\r\n";
    assert_eq!(
        parse_callback_request(request, redirect).unwrap().unwrap(),
        format!("{redirect}?code=synthetic&state=synthetic")
    );
    assert!(parse_callback_request(
        b"GET /favicon.ico HTTP/1.1\r\nHost: 127.0.0.1:45678\r\n\r\n",
        redirect
    )
    .unwrap()
    .is_none());
    for request in [
        "POST /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:45678\r\n\r\n",
        "GET /auth/callback HTTP/1.1\r\nHost: localhost:45678\r\n\r\n",
        "GET /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n",
        "GET /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:45678\r\nHost: 127.0.0.1:45678\r\n\r\n",
    ] {
        assert!(parse_callback_request(request.as_bytes(), redirect).is_err());
    }
}

#[test]
fn catalog_preserves_account_server_order_and_advertised_efforts_only() {
    let catalog = parse_catalog("account",json!({"models":[
        {"slug":"hidden","display_name":"Hidden","visibility":"hide"},
        {"slug":"z-model","display_name":"First","visibility":"list","supported_reasoning_levels":[{"effort":"high","description":"Account option"},{"effort":"low"}]},
        {"slug":"a-model","display_name":"Second","visibility":"list"}
    ]})).unwrap();
    assert_eq!(catalog.account_id, "account");
    assert_eq!(catalog.models[0].slug, "z-model");
    assert_eq!(catalog.models[0].reasoning_efforts, ["high", "low"]);
    assert!(catalog.models[1].reasoning_efforts.is_empty());
    assert!(parse_catalog("account", json!({"data":[]})).is_err());
    assert!(parse_catalog("account",json!({"models":[{"slug":"model","display_name":"Model","visibility":"list","supported_reasoning_levels":["guessed-effort"]}]})).is_err());
}

#[test]
fn provider_errors_never_echo_tokens_prompt_text_or_response_bodies() {
    let body = br#"{"error":{"code":"invalid_grant","message":"SYNTHETIC-PRIVATE-TOKEN"}}"#;
    let message = provider_failure(401, body);
    assert!(!message.contains("SYNTHETIC"));
    assert!(message.contains("Reconnect"));
    assert!(!provider_failure(500, b"SYNTHETIC-PRIVATE-TOKEN").contains("SYNTHETIC"));
    assert!(provider_failure(
        429,
        br#"{"error":{"code":"subscription_sharing_usage_limit_exceeded"}}"#
    )
    .contains("no paid fallback"));
}

#[test]
fn discovery_refuses_cross_origin_or_unexpected_token_and_key_endpoints() {
    let mut document = Discovery {
        issuer: protocol::ISSUER.into(),
        authorization_endpoint: protocol::AUTHORIZE.into(),
        token_endpoint: protocol::TOKEN.into(),
        jwks_uri: protocol::JWKS.into(),
        revocation_endpoint: "https://auth.openai.com/api/accounts/oauth/revoke".into(),
        id_token_signing_alg_values_supported: vec!["RS256".into()],
    };
    document.validate().unwrap();
    document.revocation_endpoint = "https://example.invalid/revoke".into();
    assert!(document.validate().is_err());
    document.revocation_endpoint = "https://auth.openai.com/api/accounts/oauth/revoke".into();
    document.token_endpoint = "https://auth.openai.com/unexpected".into();
    assert!(document.validate().is_err());
}

#[tokio::test]
async fn session_lock_excludes_another_instance_and_cancellation_releases_waiter() {
    let directory = TempDir::new().unwrap();
    let first = MembershipService::new(Arc::new(Storage::open(directory.path()).unwrap())).unwrap();
    let second =
        MembershipService::new(Arc::new(Storage::open(directory.path()).unwrap())).unwrap();
    let id = Uuid::new_v4().to_string();
    let cancellation = CancellationToken::new();
    let lock = first.account_lock(Some(&id), &cancellation).await.unwrap();
    assert!(tokio::time::timeout(
        Duration::from_millis(70),
        second.account_lock(Some(&id), &cancellation)
    )
    .await
    .is_err());
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(second.account_lock(Some(&id), &cancel).await.is_err());
    drop(lock);
    assert!(second.account_lock(Some(&id), &cancellation).await.is_ok());
}

#[test]
fn sign_in_guard_cleanup_and_completion_arbitrate_cancel() {
    let directory = TempDir::new().unwrap();
    let service =
        MembershipService::new(Arc::new(Storage::open(directory.path()).unwrap())).unwrap();
    let token = CancellationToken::new();
    service.pending.lock().unwrap().insert(
        "request".into(),
        PendingRequest {
            cancellation: token.clone(),
            complete: false,
            account_id: None,
        },
    );
    assert!(service.cancel_sign_in("request").unwrap());
    assert!(token.is_cancelled());
    service
        .pending
        .lock()
        .unwrap()
        .get_mut("request")
        .unwrap()
        .complete = true;
    assert!(!service.cancel_sign_in("request").unwrap());
    drop(SignInGuard {
        service: &service,
        id: "request".into(),
    });
    assert!(!service.pending.lock().unwrap().contains_key("request"));
}
