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

fn expiring_service(
    storage: Arc<Storage>,
    endpoint: &str,
) -> (Arc<MembershipService>, MembershipAccount) {
    let account = saved_account(&storage);
    let (record, version) = storage
        .membership_protected_credentials(&account.id)
        .unwrap();
    let mut credentials = open_credentials(&record).unwrap();
    credentials.access_expires_at_ms = 1;
    let account = storage
        .membership_replace_credentials(
            &account.id,
            version,
            &protect_credentials(&record.identity, &credentials).unwrap(),
        )
        .unwrap();
    let mut service = MembershipService::new(storage).unwrap();
    service.test_discovery = Some(Discovery {
        issuer: protocol::ISSUER.into(),
        authorization_endpoint: protocol::AUTHORIZE.into(),
        token_endpoint: format!("{endpoint}/token"),
        jwks_uri: format!("{endpoint}/keys"),
        revocation_endpoint: format!("{endpoint}/revoke"),
        id_token_signing_alg_values_supported: vec!["RS256".into()],
    });
    (Arc::new(service), account)
}

#[test]
fn reconnect_in_another_instance_replaces_a_cancelled_local_session_handle() {
    let directory = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(directory.path()).unwrap());
    let account = saved_account(&storage);
    let service = MembershipService::new(storage.clone()).unwrap();
    let old = service.session_cancellation(&account.id).unwrap();
    old.cancel();
    let second = Storage::open(directory.path()).unwrap();
    let (record, version) = second
        .membership_protected_credentials(&account.id)
        .unwrap();
    let cleared = second
        .membership_clear_credentials(&account.id, version)
        .unwrap();
    second
        .membership_reconnect(&account.id, cleared.credential_version, "Personal", &record)
        .unwrap();
    let current = service.session_cancellation(&account.id).unwrap();
    assert!(!current.is_cancelled());
    assert!(old.is_cancelled());
    current.cancel();
    assert!(service
        .session_cancellation(&account.id)
        .unwrap()
        .is_cancelled());
}

#[tokio::test]
async fn cancelled_request_does_not_discard_an_inflight_rotating_token() {
    let directory = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(directory.path()).unwrap());
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (service, account) = expiring_service(storage.clone(), &endpoint);
    let (seen_tx, seen) = tokio::sync::oneshot::channel();
    let (release_tx, release) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_headers(&mut stream).await.unwrap();
        seen_tx.send(()).unwrap();
        release.await.unwrap();
        write_response(&mut stream, 200, r#"{"access_token":"synthetic-next-access","refresh_token":"synthetic-next-refresh","token_type":"Bearer","expires_in":3600}"#).await;
    });
    let caller_service = service.clone();
    let id = account.id.clone();
    let caller = tokio::spawn(async move {
        caller_service
            .credentials(&id, &CancellationToken::new())
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), seen)
        .await
        .unwrap()
        .unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    release_tx.send(()).unwrap();
    server.await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let (record, version) = storage
                .membership_protected_credentials(&account.id)
                .unwrap();
            if version > account.credential_version {
                assert_eq!(
                    open_credentials(&record).unwrap().refresh_token,
                    "synthetic-next-refresh"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The shared renewal must persist after its requesting task is dropped");
    assert_eq!(
        storage
            .membership_account(&account.id)
            .unwrap()
            .session_version,
        account.session_version
    );
    let (credentials, _) = service
        .credentials(&account.id, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(credentials.access_token, "synthetic-next-access");
}

#[tokio::test]
async fn signout_waits_for_rotation_and_revokes_the_replacement_token() {
    let directory = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(directory.path()).unwrap());
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (service, account) = expiring_service(storage.clone(), &endpoint);
    let (seen_tx, seen) = tokio::sync::oneshot::channel();
    let (release_tx, release) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut renewal, _) = listener.accept().await.unwrap();
        read_headers(&mut renewal).await.unwrap();
        seen_tx.send(()).unwrap();
        release.await.unwrap();
        write_response(&mut renewal, 200, r#"{"access_token":"synthetic-next-access","refresh_token":"synthetic-next-refresh","token_type":"Bearer","expires_in":3600}"#).await;
        let (mut revocation, _) = listener.accept().await.unwrap();
        let mut bytes = read_headers(&mut revocation).await.unwrap();
        let end = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap() + 4;
        let length = std::str::from_utf8(&bytes[..end])
            .unwrap()
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        while bytes.len() < end + length {
            let mut more = [0; 1024];
            let count = revocation.read(&mut more).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&more[..count]);
        }
        assert!(std::str::from_utf8(&bytes[end..])
            .unwrap()
            .contains("token=synthetic-next-refresh"));
        write_response(&mut revocation, 200, "").await;
    });
    let renewal_service = service.clone();
    let id = account.id.clone();
    let session = service.session_cancellation(&id).unwrap();
    let renewal = tokio::spawn(async move { renewal_service.credentials(&id, &session).await });
    tokio::time::timeout(Duration::from_secs(3), seen)
        .await
        .unwrap()
        .unwrap();
    let signout_service = service.clone();
    let id = account.id.clone();
    let signout = tokio::spawn(async move { signout_service.sign_out(&id).await });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !service
            .session_cancellation(&account.id)
            .unwrap()
            .is_cancelled()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    release_tx.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), signout)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(result.remote_revoked);
    assert!(!result.account.has_credentials);
    assert!(result.account.session_version > account.session_version);
    renewal.await.unwrap().unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn signout_keeps_ownership_past_the_normal_account_request_deadline() {
    let directory = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(directory.path()).unwrap());
    let account = saved_account(&storage);
    let service = Arc::new(MembershipService::new(storage.clone()).unwrap());
    let session = service.session_cancellation(&account.id).unwrap();
    let lock = service
        .account_lock(Some(&account.id), &CancellationToken::new())
        .await
        .unwrap();
    let signing_out = service.clone();
    let id = account.id.clone();
    let mut operation = tokio::spawn(async move { signing_out.sign_out(&id).await });
    session.cancelled().await;
    // A legitimate token request plus key lookup can outlast the ordinary
    // 30-second acquisition limit. Sign-out must retain cleanup ownership.
    let premature = tokio::time::timeout(Duration::from_secs(31), &mut operation).await;
    storage
        .membership_clear_credentials(&account.id, account.credential_version)
        .unwrap();
    drop(lock);
    assert!(
        premature.is_err(),
        "Sign-out abandoned cleanup while the account owner was still working"
    );
    let result = tokio::time::timeout(Duration::from_secs(3), operation)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!result.account.has_credentials);
}

#[tokio::test]
async fn dropped_signout_caller_does_not_abandon_revocation_and_local_clear() {
    let directory = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(directory.path()).unwrap());
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (service, account) = expiring_service(storage.clone(), &endpoint);
    let (seen_tx, seen) = tokio::sync::oneshot::channel();
    let (release_tx, release) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_headers(&mut stream).await.unwrap();
        seen_tx.send(()).unwrap();
        release.await.unwrap();
        write_response(&mut stream, 200, "").await;
    });
    let id = account.id.clone();
    let caller = tokio::spawn(async move { service.sign_out(&id).await });
    tokio::time::timeout(Duration::from_secs(3), seen)
        .await
        .unwrap()
        .unwrap();
    caller.abort();
    assert!(caller.await.is_err());
    release_tx.send(()).unwrap();
    server.await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while storage
            .membership_account(&account.id)
            .unwrap()
            .has_credentials
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Sign-out must finish clearing local credentials after losing its caller");
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
