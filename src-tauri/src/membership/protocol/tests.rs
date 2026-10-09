use super::*;
use jsonwebtoken::{encode, EncodingKey, Header};
use rsa::{pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts, RsaPrivateKey};
use serde_json::{json, Value};
use std::sync::OnceLock;

struct Fixture {
    key: EncodingKey,
    jwks: JwkSet,
}
fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let private = RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
        let public = private.to_public_key();
        Fixture { key: EncodingKey::from_rsa_der(private.to_pkcs1_der().unwrap().as_bytes()),jwks: serde_json::from_value(json!({"keys":[{"kty":"RSA","kid":"synthetic-key","alg":"RS256","use":"sig","key_ops":["verify"],"n":URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(public.e().to_bytes_be())}]})).unwrap() }
    })
}
fn host() -> String {
    format!("urn:uuid:{}", Uuid::new_v4())
}
fn pending() -> PendingSignIn {
    PendingSignIn::new(1455, host(), None).unwrap()
}
fn claims(attempt: &PendingSignIn, client: &str) -> Value {
    let now = unix_seconds().unwrap();
    json!({"iss":ISSUER,"aud":client,"sub":"synthetic-user","iat":now,"exp":now+300,"nonce":attempt.nonce,"email":"fixture@example.invalid"})
}
fn sign(claims: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("synthetic-key".into());
    encode(&header, claims, &fixture().key).unwrap()
}
fn callback(attempt: &PendingSignIn, suffix: &str) -> String {
    format!(
        "{}?code=synthetic-code&state={}&client_id=oaiapp_fixture{suffix}",
        attempt.redirect_uri, attempt.state
    )
}
fn reply(id_token: String) -> TokenReply {
    TokenReply {
        id_token: Some(id_token),
        access_token: "synthetic-access".into(),
        refresh_token: Some("synthetic-refresh".into()),
        token_type: "Bearer".into(),
        scope: Some(SCOPE.into()),
        expires_in: 3600,
    }
}

fn registered_session() -> (MembershipIdentity, MembershipCredentials) {
    let mut attempt = pending();
    let code = attempt.consume_callback(&callback(&attempt, "")).unwrap();
    let token = sign(&claims(&attempt, &code.client_id));
    attempt
        .validate_reply(&code, reply(token), &fixture().jwks)
        .unwrap()
}

fn refresh_claims(identity: &MembershipIdentity) -> Value {
    let now = unix_seconds().unwrap();
    json!({"iss":identity.issuer,"aud":identity.client_id,"sub":identity.subject,
        "iat":now,"exp":now+300})
}

#[test]
fn refresh_accepts_signed_identity_without_a_new_nonce_or_unchanged_display_metadata() {
    let (identity, _) = registered_session();
    let mut refreshed = refresh_claims(&identity);
    assert!(verify_refreshed_identity(&sign(&refreshed), &identity, &fixture().jwks).is_ok());

    // A refresh can retain an earlier nonce; it is not a new browser sign-in.
    refreshed["nonce"] = json!("synthetic-original-sign-in-nonce");
    refreshed["email"] = json!("updated-display@example.invalid");
    assert!(verify_refreshed_identity(&sign(&refreshed), &identity, &fixture().jwks).is_ok());
}

#[test]
fn initial_sign_in_still_requires_the_attempt_nonce_and_complete_grants() {
    let mut attempt = pending();
    let code = attempt.consume_callback(&callback(&attempt, "")).unwrap();
    let valid = claims(&attempt, &code.client_id);
    for nonce in [
        None,
        Some(json!("different-sign-in-nonce")),
        Some(Value::Null),
    ] {
        let mut invalid = valid.clone();
        invalid.as_object_mut().unwrap().remove("nonce");
        if let Some(nonce) = nonce {
            invalid["nonce"] = nonce;
        }
        assert!(attempt
            .validate_reply(&code, reply(sign(&invalid)), &fixture().jwks)
            .is_err());
    }
    for omitted in ["id_token", "refresh_token", "scope"] {
        let mut incomplete = reply(sign(&valid));
        match omitted {
            "id_token" => incomplete.id_token = None,
            "refresh_token" => incomplete.refresh_token = None,
            "scope" => incomplete.scope = None,
            _ => unreachable!(),
        }
        assert!(
            attempt
                .validate_reply(&code, incomplete, &fixture().jwks)
                .is_err(),
            "initial sign-in must require {omitted}"
        );
    }
    assert!(attempt
        .validate_reply(&code, reply(sign(&valid)), &fixture().jwks)
        .is_ok());
}

#[test]
fn refresh_rejects_wrong_signed_registration_and_invalid_validity_claims() {
    let (identity, _) = registered_session();
    let valid = refresh_claims(&identity);
    assert!(verify_refreshed_identity(&sign(&valid), &identity, &fixture().jwks).is_ok());
    for (claim, value) in [
        ("iss", json!("https://wrong-issuer.invalid")),
        ("aud", json!("oaiapp_other")),
        ("sub", json!("another-synthetic-user")),
        ("sub", json!("")),
        ("azp", json!("oaiapp_other")),
        ("exp", json!(unix_seconds().unwrap() - 60)),
        ("iat", json!(unix_seconds().unwrap() + 120)),
        ("nbf", json!(unix_seconds().unwrap() + 120)),
        ("exp", valid["iat"].clone()),
    ] {
        let mut invalid = valid.clone();
        invalid[claim] = value;
        assert!(
            verify_refreshed_identity(&sign(&invalid), &identity, &fixture().jwks).is_err(),
            "refresh accepted invalid {claim}: {}",
            invalid[claim]
        );
    }
    for missing in ["iss", "aud", "sub", "exp", "iat"] {
        let mut invalid = valid.clone();
        invalid.as_object_mut().unwrap().remove(missing);
        assert!(
            verify_refreshed_identity(&sign(&invalid), &identity, &fixture().jwks).is_err(),
            "refresh accepted missing {missing}"
        );
    }
}

#[test]
fn refresh_checks_rsa_signature_and_authorized_party_for_multiple_audiences() {
    let (identity, _) = registered_session();
    let valid = refresh_claims(&identity);
    let token = sign(&valid);
    assert!(verify_refreshed_identity(&token, &identity, &fixture().jwks).is_ok());
    let mut parts = token.split('.').map(str::to_owned).collect::<Vec<_>>();
    let mut signature = URL_SAFE_NO_PAD.decode(&parts[2]).unwrap();
    signature[0] ^= 1;
    parts[2] = URL_SAFE_NO_PAD.encode(signature);
    assert!(verify_refreshed_identity(&parts.join("."), &identity, &fixture().jwks).is_err());

    let mut multiple = valid;
    multiple["aud"] = json!([identity.client_id, "oaiapp_other"]);
    for authorized_party in [
        None,
        Some("oaiapp_other"),
        Some(identity.client_id.as_str()),
    ] {
        multiple.as_object_mut().unwrap().remove("azp");
        if let Some(party) = authorized_party {
            multiple["azp"] = json!(party);
        }
        let result = verify_refreshed_identity(&sign(&multiple), &identity, &fixture().jwks);
        assert_eq!(
            result.is_ok(),
            authorized_party == Some(identity.client_id.as_str())
        );
    }
}

#[test]
fn codex_image_identity_requires_the_codex_client_signature_and_same_chatgpt_subject() {
    let (identity, _) = registered_session();
    let mut claims = refresh_claims(&identity);
    claims["aud"] = json!(CODEX_IMAGE_CLIENT_ID);
    claims["azp"] = json!(CODEX_IMAGE_CLIENT_ID);
    let host = "urn:uuid:82dd5019-df90-411c-a3a7-53755c799c04";
    let signed = sign(&claims);
    let accepted = verify_codex_image_identity(&signed, host, &fixture().jwks).unwrap();
    assert_eq!(accepted.subject, identity.subject);
    assert_eq!(accepted.client_id, CODEX_IMAGE_CLIENT_ID);
    let mut wrong = claims;
    wrong["aud"] = json!("some-other-client");
    wrong["azp"] = json!("some-other-client");
    assert!(verify_codex_image_identity(&sign(&wrong), host, &fixture().jwks).is_err());
}

#[test]
fn chatgpt_account_id_is_extracted_only_as_validated_identity_metadata() {
    let (identity, _) = registered_session();
    let mut claims = refresh_claims(&identity);
    claims["aud"] = json!(CODEX_IMAGE_CLIENT_ID);
    claims["azp"] = json!(CODEX_IMAGE_CLIENT_ID);
    claims["https://api.openai.com/auth"] = json!({"chatgpt_account_id":"acct_123"});
    let token = sign(&claims);
    verify_codex_image_identity(&token, &host(), &fixture().jwks).unwrap();
    assert_eq!(
        chatgpt_account_id_from_oauth_token(&token)
            .unwrap()
            .as_deref(),
        Some("acct_123")
    );
}

#[test]
fn explicit_image_link_accepts_client_scoped_subjects_and_rejects_wrong_identity() {
    let (primary, _) = registered_session();
    let mut secondary = refresh_claims(&primary);
    secondary["aud"] = json!(CODEX_IMAGE_CLIENT_ID);
    secondary["azp"] = json!(CODEX_IMAGE_CLIENT_ID);
    secondary["sub"] = json!("different-client-subject");
    secondary["email"] = json!(primary.email);
    secondary["email_verified"] = json!(true);
    let routing = |id: Option<&str>| {
        let value = id
            .map(|id| json!({"https://api.openai.com/auth":{"chatgpt_account_id":id}}))
            .unwrap_or_else(
                || json!({"https://api.openai.com/auth":{"encrypted_auth_metadata":"opaque"}}),
            );
        format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&value).unwrap())
        )
    };
    let missing = routing(None);
    let codex = routing(Some("workspace_one"));
    let accepted = verify_codex_image_link(
        &sign(&secondary),
        &primary,
        &missing,
        &codex,
        &fixture().jwks,
    )
    .unwrap();
    assert_ne!(accepted.subject, primary.subject);
    assert_eq!(accepted.email, primary.email);
    secondary["email"] = json!("wrong@example.invalid");
    assert!(verify_codex_image_link(
        &sign(&secondary),
        &primary,
        &missing,
        &codex,
        &fixture().jwks
    )
    .is_err());
    secondary["email"] = json!(primary.email);
    secondary["email_verified"] = json!(false);
    assert!(verify_codex_image_link(
        &sign(&secondary),
        &primary,
        &missing,
        &codex,
        &fixture().jwks
    )
    .is_err());
    secondary["email_verified"] = json!(true);
    assert!(verify_codex_image_link(
        &sign(&secondary),
        &primary,
        &routing(Some("workspace_other")),
        &codex,
        &fixture().jwks
    )
    .is_err());
    secondary["aud"] = json!("wrong-client");
    assert!(verify_codex_image_link(
        &sign(&secondary),
        &primary,
        &missing,
        &codex,
        &fixture().jwks
    )
    .is_err());
}

#[test]
fn refresh_retains_only_omitted_identity_refresh_token_and_scopes() {
    let (identity, mut previous) = registered_session();
    previous.codex_images = Some(super::super::CodexImageCredentials {
        linked_primary_subject: identity.subject.clone(),
        linked_primary_client_id: identity.client_id.clone(),
        verified_email: identity.email.clone().unwrap(),
        issuer: identity.issuer.clone(),
        subject: identity.subject.clone(),
        client_id: super::CODEX_IMAGE_CLIENT_ID.into(),
        chatgpt_account_id: Some("acct_synthetic".into()),
        access_token: "synthetic-codex-access".into(),
        refresh_token: "synthetic-codex-refresh".into(),
        token_type: "Bearer".into(),
        access_expires_at_ms: 1_900_000_000_000,
    });
    let token = sign(&refresh_claims(&identity));
    verify_refreshed_identity(&token, &identity, &fixture().jwks).unwrap();
    let reduced_scopes = "openid resource.invoke chatgpt.tokens.use.direct";
    // Every combination proves omission is field-specific, not all-or-nothing.
    for omissions in 0..8 {
        let mut refreshed = reply(token.clone());
        refreshed.access_token = "synthetic-next-access".into();
        refreshed.token_type = "bEaReR".into();
        refreshed.id_token = (omissions & 1 == 0).then(|| token.clone());
        refreshed.refresh_token = (omissions & 2 == 0).then(|| "synthetic-rotated-refresh".into());
        refreshed.scope = (omissions & 4 == 0).then(|| reduced_scopes.into());
        let before = (unix_seconds().unwrap() * 1000) as i64;
        let credentials = credentials_from_reply(&identity, refreshed, Some(&previous)).unwrap();
        let after = (unix_seconds().unwrap() * 1000) as i64;
        assert_eq!(
            credentials.id_token,
            if omissions & 1 == 0 {
                &token
            } else {
                &previous.id_token
            }
            .as_str()
        );
        assert_eq!(
            credentials.refresh_token,
            if omissions & 2 == 0 {
                "synthetic-rotated-refresh"
            } else {
                &previous.refresh_token
            }
        );
        let expected_scopes = if omissions & 4 == 0 {
            reduced_scopes
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        } else {
            previous.scopes.clone()
        };
        assert_eq!(credentials.scopes, expected_scopes);
        assert_eq!(credentials.access_token, "synthetic-next-access");
        assert_eq!(credentials.token_type, "Bearer");
        assert_eq!(credentials.issuer, identity.issuer);
        assert_eq!(credentials.subject, identity.subject);
        assert_eq!(credentials.client_id, identity.client_id);
        assert_eq!(credentials.ext_agent_host_id, identity.host_id);
        assert_eq!(
            credentials.codex_images.as_ref().unwrap().refresh_token,
            "synthetic-codex-refresh"
        );
        assert!(
            (before + 3_600_000..=after + 3_600_000).contains(&credentials.access_expires_at_ms)
        );
    }
}

#[test]
fn refresh_explicitly_invalid_grants_do_not_fall_back_to_previous_credentials() {
    let (identity, previous) = registered_session();
    let token = sign(&refresh_claims(&identity));
    verify_refreshed_identity(&token, &identity, &fixture().jwks).unwrap();
    for invalid in [
        "empty-id",
        "blank-id",
        "empty-refresh",
        "blank-refresh",
        "empty-access",
        "blank-access",
        "wrong-token-type",
        "empty-token-type",
        "empty-scope",
        "missing-openid",
        "missing-resource",
        "missing-direct",
        "zero-expiry",
        "expiry-conversion-overflow",
        "expiry-multiplication-overflow",
        "expiry-addition-overflow",
    ] {
        let mut refreshed = reply(token.clone());
        match invalid {
            "empty-id" => refreshed.id_token = Some(String::new()),
            "blank-id" => refreshed.id_token = Some(" \t\n".into()),
            "empty-refresh" => refreshed.refresh_token = Some(String::new()),
            "blank-refresh" => refreshed.refresh_token = Some(" \t\n".into()),
            "empty-access" => refreshed.access_token.clear(),
            "blank-access" => refreshed.access_token = " \t\n".into(),
            "wrong-token-type" => refreshed.token_type = "Basic".into(),
            "empty-token-type" => refreshed.token_type.clear(),
            "empty-scope" => refreshed.scope = Some(String::new()),
            "missing-openid" => {
                refreshed.scope = Some("resource.invoke chatgpt.tokens.use.direct".into())
            }
            "missing-resource" => refreshed.scope = Some("openid chatgpt.tokens.use.direct".into()),
            "missing-direct" => refreshed.scope = Some("openid resource.invoke".into()),
            "zero-expiry" => refreshed.expires_in = 0,
            "expiry-conversion-overflow" => refreshed.expires_in = u64::MAX,
            "expiry-multiplication-overflow" => refreshed.expires_in = i64::MAX as u64 / 1000 + 1,
            "expiry-addition-overflow" => refreshed.expires_in = i64::MAX as u64 / 1000,
            _ => unreachable!(),
        }
        assert!(
            credentials_from_reply(&identity, refreshed, Some(&previous)).is_err(),
            "refresh silently accepted {invalid} using retained credentials"
        );
    }
}

#[test]
fn pkce_matches_rfc_vector_and_attempts_have_distinct_secrets() {
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let a = pending();
    let b = pending();
    assert_ne!(a.state, b.state);
    assert_ne!(a.nonce, b.nonce);
    assert_ne!(a.verifier, b.verifier);
    assert!((43..=128).contains(&a.verifier.len()));
    let url = a.authorization_url(None).unwrap();
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["agent_name_hint"], "CrowClaw");
    assert_eq!(query["client_id"], "dynamic_agent_client");
    assert_eq!(query["redirect_uri"], "http://127.0.0.1:1455/auth/callback");
    assert!(!url.as_str().contains(&a.verifier));
}

#[test]
fn callback_is_one_time_exactly_bound_and_requires_an_issued_client() {
    let mut a = pending();
    let url = callback(&a, "");
    let code = a.consume_callback(&url).unwrap();
    assert_eq!(code.client_id, "oaiapp_fixture");
    assert!(a.consume_callback(&url).is_err());
    let form: HashMap<_, _> = a.exchange_form(&code).into_iter().collect();
    assert_eq!(form["redirect_uri"], a.redirect_uri);
    assert_eq!(form["code_verifier"], a.verifier);
    for change in [
        "wrong-state",
        "wrong-host",
        "wrong-path",
        "duplicate",
        "missing-client",
        "dynamic-client",
        "denied",
    ] {
        let mut a = pending();
        let mut url = callback(&a, "");
        match change {
            "wrong-state" => url = url.replace(&a.state, "wrong"),
            "wrong-host" => url = url.replace("127.0.0.1", "localhost"),
            "wrong-path" => url = url.replace("/auth/callback", "/callback"),
            "duplicate" => url.push_str("&state=another"),
            "missing-client" => url = url.replace("&client_id=oaiapp_fixture", ""),
            "dynamic-client" => url = url.replace("oaiapp_fixture", "dynamic_agent_client"),
            "denied" => url.push_str("&error=access_denied"),
            _ => unreachable!(),
        }
        assert!(a.consume_callback(&url).is_err(), "{change}");
        assert!(a.consumed);
    }
    let mut expired = pending();
    expired.created = Instant::now() - AUTH_TIMEOUT;
    assert!(expired.consume_callback(&callback(&expired, "")).is_err());
}

#[test]
fn retry_uses_fresh_pkce_state_and_the_previously_issued_registration() {
    let mut a = pending();
    let code = a.consume_callback(&callback(&a, "")).unwrap();
    let retry = a.retry_after_invalid_grant(&code).unwrap();
    assert_ne!(a.state, retry.state);
    assert_ne!(a.nonce, retry.nonce);
    assert_ne!(a.verifier, retry.verifier);
    let query: HashMap<_, _> = retry
        .authorization_url(None)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(query["client_id"], "oaiapp_fixture");
    assert!(!query.contains_key("agent_name_hint"));
    assert_eq!(retry.host_id, a.host_id);
}

#[test]
fn signed_identity_and_granted_scopes_are_both_required() {
    let mut a = pending();
    let code = a.consume_callback(&callback(&a, "")).unwrap();
    let token = sign(&claims(&a, &code.client_id));
    let (identity, credentials) = a
        .validate_reply(&code, reply(token.clone()), &fixture().jwks)
        .unwrap();
    assert_eq!(identity.subject, "synthetic-user");
    assert_eq!(identity.client_id, code.client_id);
    assert_eq!(identity.host_id, a.host_id);
    assert!(credentials.access_expires_at_ms > (unix_seconds().unwrap() * 1000) as i64);
    let mut denied = reply(token);
    denied.scope = Some("openid profile email".into());
    assert!(a.validate_reply(&code, denied, &fixture().jwks).is_err());
}

#[test]
fn rejects_wrong_signature_issuer_audience_nonce_expiry_iat_and_authorized_party() {
    let a = pending();
    let valid = claims(&a, "oaiapp_fixture");
    for (name, value) in [
        ("iss", json!("https://wrong.invalid")),
        ("aud", json!("another-client")),
        ("nonce", json!("different")),
        ("exp", json!(unix_seconds().unwrap() - 60)),
        ("iat", json!(unix_seconds().unwrap() + 120)),
        ("azp", json!("another-client")),
        ("sub", json!("")),
    ] {
        let mut invalid = valid.clone();
        invalid[name] = value;
        assert!(
            verify_identity(
                &sign(&invalid),
                "oaiapp_fixture",
                &a.nonce,
                &a.host_id,
                &fixture().jwks
            )
            .is_err(),
            "{name}"
        );
    }
    let mut token = sign(&valid).into_bytes();
    let dot = token.iter().rposition(|b| *b == b'.').unwrap();
    token[dot + 4] = if token[dot + 4] == b'A' { b'B' } else { b'A' };
    assert!(verify_identity(
        std::str::from_utf8(&token).unwrap(),
        "oaiapp_fixture",
        &a.nonce,
        &a.host_id,
        &fixture().jwks
    )
    .is_err());
    let mut multi = valid.clone();
    multi["aud"] = json!(["oaiapp_fixture", "another-client"]);
    assert!(verify_identity(
        &sign(&multi),
        "oaiapp_fixture",
        &a.nonce,
        &a.host_id,
        &fixture().jwks
    )
    .is_err());
    multi["azp"] = json!("oaiapp_fixture");
    assert!(verify_identity(
        &sign(&multi),
        "oaiapp_fixture",
        &a.nonce,
        &a.host_id,
        &fixture().jwks
    )
    .is_ok());
}

#[test]
fn rejects_algorithm_confusion_and_ambiguous_keys() {
    let a = pending();
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("synthetic-key".into());
    let token = encode(
        &header,
        &claims(&a, "oaiapp_fixture"),
        &EncodingKey::from_secret(b"synthetic-hmac-key"),
    )
    .unwrap();
    assert!(verify_identity(
        &token,
        "oaiapp_fixture",
        &a.nonce,
        &a.host_id,
        &fixture().jwks
    )
    .is_err());
    let mut duplicate = fixture().jwks.clone();
    duplicate.keys.push(duplicate.keys[0].clone());
    assert!(verify_identity(
        &sign(&claims(&a, "oaiapp_fixture")),
        "oaiapp_fixture",
        &a.nonce,
        &a.host_id,
        &duplicate
    )
    .is_err());
}

#[test]
fn returning_account_retains_client_and_refuses_a_different_signed_subject() {
    let expected = MembershipIdentity {
        provider: "chatgpt".into(),
        issuer: ISSUER.into(),
        subject: "original-user".into(),
        client_id: "oaiapp_saved".into(),
        host_id: host(),
        email: Some("same@example.invalid".into()),
    };
    let mut a =
        PendingSignIn::new(45678, expected.host_id.clone(), Some(expected.clone())).unwrap();
    let query: HashMap<_, _> = a
        .authorization_url(Some("synthetic-hint"))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(query["client_id"], expected.client_id);
    assert!(!query.contains_key("agent_name_hint"));
    assert_eq!(query["id_token_hint"], "synthetic-hint");
    let url = format!("{}?code=synthetic-code&state={}", a.redirect_uri, a.state);
    let code = a.consume_callback(&url).unwrap();
    assert_eq!(code.client_id, expected.client_id);
    assert!(a
        .validate_reply(
            &code,
            reply(sign(&claims(&a, &expected.client_id))),
            &fixture().jwks
        )
        .is_err());
    let mut valid = claims(&a, &expected.client_id);
    valid["sub"] = json!(expected.subject);
    assert!(a
        .validate_reply(&code, reply(sign(&valid)), &fixture().jwks)
        .is_ok());
    let mut another = PendingSignIn::new(45678, expected.host_id.clone(), Some(expected)).unwrap();
    assert!(another.consume_callback(&callback(&another, "")).is_err());
}
