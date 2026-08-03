//! Phase 2 — Hub middleware: Bearer extraction + audience allowlist
//! + dual-credential 400 + CSRF/Origin gating by credential kind.
//!
//! These tests spin up a [`crate::support::MockOidcProvider`] (axum
//! server on a random localhost port serving discovery + JWKS) and a
//! [`crate::support::TestHub`] configured with two allowlisted
//! audiences — the SPA's `client_id` and the hub-mcp
//! `additional_audiences` entry — plus an issuer pointing at the mock
//! OIDC provider.
//!
//! JWTs are minted in-process with a RS256 keypair held by the
//! provider. The corresponding public JWK is served at the JWKS URL,
//! and `RemoteJwksDecoder` fetches it once at hub startup. The hub
//! itself is built via `build_router_with_state` with auth-state
//! injected through `build_auth_state_from_parts`, which skips OIDC
//! discovery (the discovery path requires HTTPS).
//!
//! Plan: claude-notes/plans/2026-05-05-hub-mcp-device-flow-implementation.md
//! §Phase 2.

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, pkcs8::LineEnding};
use serde_json::json;

use crate::support::{
    AUTH_COOKIE_NAME_SECURE, ClaimsBuilder, MCP_CLIENT_ID, MockOidcProvider, SPA_CLIENT_ID,
    TestHub, TestHubBuilder, install_tracing_once, snapshot_events,
};

// ── shared test fixtures ──────────────────────────────────────────

async fn shared_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new().start(&provider).await;
            (provider, hub)
        })
        .await
}

/// Separate hub with allowed_domains = ["posit.co"]; lets us assert
/// 403 vs 401 distinction on the allowlist path.
async fn allowlist_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .allowed_domains(&["posit.co"])
                .start(&provider)
                .await;
            (provider, hub)
        })
        .await
}

/// Secure (allow_insecure_auth=false) hub for the WS-Origin regression.
async fn secure_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new().secure().start(&provider).await;
            (provider, hub)
        })
        .await
}

// ── Bearer audience tests ─────────────────────────────────────────

#[tokio::test]
async fn bearer_with_spa_audience_authenticates() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("spa-aud-sub")
            .aud(json!(SPA_CLIENT_ID))
            .to_value(),
    );

    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 200, "body: {:?}", resp.text().await);
}

#[tokio::test]
async fn bearer_with_mcp_audience_authenticates() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("mcp-aud-sub")
            .aud(json!(MCP_CLIENT_ID))
            .azp(MCP_CLIENT_ID) // real Google tokens always carry azp
            .to_value(),
    );

    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn bearer_with_unknown_audience_returns_401() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("unknown-aud-sub")
            .aud(json!("attacker.apps.googleusercontent.com"))
            .to_value(),
    );

    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn bearer_with_no_audience_returns_401() {
    let (provider, hub) = shared_setup().await;
    let now = chrono::Utc::now().timestamp();
    // Hand-build claims with NO `aud` at all.
    let claims = json!({
        "iss": provider.issuer,
        "sub": "no-aud-sub",
        "email": "user@posit.co",
        "email_verified": true,
        "exp": now + 600,
        "iat": now - 5,
    });
    let token = provider.sign(&claims);
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

// ── OIDC §3.1.3.7 azp tests ───────────────────────────────────────

#[tokio::test]
async fn bearer_with_aud_array_and_matching_azp_authenticates() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("multi-aud-azp-ok")
            .aud(json!([SPA_CLIENT_ID, MCP_CLIENT_ID]))
            .azp(SPA_CLIENT_ID)
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn bearer_with_aud_array_and_missing_azp_returns_401() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("multi-aud-no-azp")
            .aud(json!([SPA_CLIENT_ID, MCP_CLIENT_ID]))
            .no_azp()
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn bearer_with_aud_array_and_mismatched_azp_returns_401() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("multi-aud-bad-azp")
            .aud(json!([SPA_CLIENT_ID, MCP_CLIENT_ID]))
            .azp("attacker.apps.googleusercontent.com")
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn bearer_with_single_aud_and_present_azp_validates_azp() {
    let (provider, hub) = shared_setup().await;
    // aud single-valued, azp present but bad → must reject.
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("single-aud-bad-azp")
            .aud(json!(SPA_CLIENT_ID))
            .azp("attacker.apps.googleusercontent.com")
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn bearer_with_single_aud_and_absent_azp_authenticates() {
    let (provider, hub) = shared_setup().await;
    // Common case for OIDC providers that omit azp on single-aud tokens.
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("single-aud-no-azp")
            .aud(json!(SPA_CLIENT_ID))
            .no_azp()
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

// ── issuer / exp / iat / signature ────────────────────────────────

#[tokio::test]
async fn bearer_with_wrong_issuer_returns_401() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .iss("https://evil.example.com")
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn bearer_with_expired_token_returns_401() {
    let (provider, hub) = shared_setup().await;
    let now = chrono::Utc::now().timestamp();
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .exp(now - 3600)
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn bearer_with_future_iat_returns_401() {
    let (provider, hub) = shared_setup().await;
    let now = chrono::Utc::now().timestamp();
    // iat well beyond the default 60-second leeway; nbf absent so the
    // rejection unambiguously comes from our iat check.
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .iat(now + 3600)
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn bearer_with_future_iat_within_skew_authenticates() {
    let (provider, hub) = shared_setup().await;
    let now = chrono::Utc::now().timestamp();
    // 30s in the future, within the 60s default leeway.
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .iat(now + 30)
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 200, "body: {:?}", resp.text().await);
}

#[tokio::test]
async fn bearer_with_invalid_signature_returns_401() {
    let (_provider, hub) = shared_setup().await;
    // Sign with a fresh keypair the JWKS doesn't know about.
    let mut rng = rsa::rand_core::OsRng;
    let foreign = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let foreign_pem = foreign.to_pkcs1_pem(LineEnding::LF).unwrap().to_string();
    let foreign_key = EncodingKey::from_rsa_pem(foreign_pem.as_bytes()).unwrap();
    // Use the same kid the JWKS advertises; the signature itself is wrong.
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-kid-1".to_string());
    let now = chrono::Utc::now().timestamp();
    let claims = json!({
        "iss": hub.base_url, // any iss — signature fails before issuer check
        "sub": "bad-sig",
        "aud": SPA_CLIENT_ID,
        "email": "user@posit.co",
        "email_verified": true,
        "exp": now + 600,
    });
    let token = jsonwebtoken::encode(&header, &claims, &foreign_key).unwrap();
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

// ── allowlist parity (Bearer path runs check_allowlists) ─────────

#[tokio::test]
async fn bearer_with_unverified_email_returns_401() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("unverified-email")
            .email_verified(false)
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn bearer_with_unallowlisted_email_returns_403() {
    let (provider, hub) = allowlist_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("not-allowed-domain")
            .email("user@gmail.com")
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 403, "expected 403 for not-allowlisted email");
}

#[tokio::test]
async fn bearer_with_allowed_domain_authenticates() {
    let (provider, hub) = allowlist_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("allowed-domain")
            .email("user@posit.co")
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn bearer_with_mcp_audience_but_unverified_email_returns_401() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("mcp-aud-unverified")
            .aud(json!(MCP_CLIENT_ID))
            .azp(MCP_CLIENT_ID)
            .email_verified(false)
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(
        resp.status(),
        401,
        "MCP-audience tokens must still pass the email_verified gate"
    );
}

// ── WS Bearer ─────────────────────────────────────────────────────

#[tokio::test]
async fn ws_upgrade_with_bearer_outside_allowlist_returns_403() {
    let (provider, hub) = allowlist_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("ws-not-allowed")
            .email("user@gmail.com")
            .to_value(),
    );
    let resp = hub
        .ws_upgrade()
        .header("origin", &hub.base_url)
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn ws_upgrade_with_bearer_works() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("ws-bearer-ok")
            .to_value(),
    );
    let resp = hub
        .ws_upgrade()
        // No `Origin` header — Bearer auth must skip the origin check.
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        101,
        "expected 101 Switching Protocols; got body: {:?}",
        resp.text().await
    );
}

#[tokio::test]
async fn ws_upgrade_rejects_dual_credentials() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("ws-dual-cred")
            .to_value(),
    );
    let resp = hub
        .ws_upgrade()
        .header("origin", &hub.base_url)
        .header("cookie", format!("quarto_hub_token={token}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "dual credentials must reject with 400");
}

#[tokio::test]
async fn ws_upgrade_with_bearer_skips_origin_check() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("ws-bearer-cross-origin")
            .to_value(),
    );
    let resp = hub
        .ws_upgrade()
        // Cross-origin Origin header — cookie auth would reject, Bearer must not.
        .header("origin", "https://attacker.example.com")
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 101);
}

#[tokio::test]
async fn ws_upgrade_with_cookie_still_requires_origin() {
    // Hubs started with allow_insecure_auth=true skip the Origin check,
    // so this regression needs the secure fixture — which is also why
    // the cookie carries the `__Host-` prefixed name (H3).
    let (provider, hub) = secure_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("ws-cookie-bad-origin")
            .to_value(),
    );
    let resp = hub
        .ws_upgrade()
        .header("origin", "https://attacker.example.com")
        .header("cookie", format!("{AUTH_COOKIE_NAME_SECURE}={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "cookie-authenticated WS upgrade with bad Origin must 403"
    );
}

// NOTE: `cookie_still_authenticates` and `auth_me_returns_token_exp`
// (both driving /auth/me with a Google JWT in the cookie) were retired
// by the sliding-sessions cutover — Google-JWT cookies now 401 (§6
// hard break). Their successors live in `session_auth.rs`:
// `session_cookie_authenticates_on_extractor_endpoint`,
// `auth_me_returns_sliding_exp_from_session`, and
// `auth_me_rejects_legacy_google_cookie`.

// ── Dual-credential 400 (bd-wzhsf CVE) ───────────────────────────

#[tokio::test]
async fn cookie_and_bearer_returns_400() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("dual-cred-cve")
            .to_value(),
    );

    let resp = hub
        .get_health()
        .bearer_auth(&token)
        .header("cookie", format!("quarto_hub_token={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "dual credentials must reject with 400 — bd-wzhsf CVE-prevention"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "conflicting_credentials");
}

#[tokio::test]
async fn bearer_wrong_scheme_returns_401() {
    let (_provider, hub) = shared_setup().await;
    // `Basic`, `Token`, etc. must be 401, never 400 (no dual credential).
    // The credential value after the scheme is intentionally non-base64
    // and non-JWT-shaped so secret-scanners don't flag it; the hub's
    // extractor rejects on scheme prefix without parsing the rest.
    for scheme in ["Basic placeholder", "Token placeholder"] {
        let resp = hub
            .get_health()
            .header("authorization", scheme)
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            401,
            "scheme {scheme:?} must yield 401 (saw {})",
            resp.status()
        );
    }
}

// ── Audit logging ────────────────────────────────────────────────

#[tokio::test]
async fn audit_event_on_auth_ok() {
    let (provider, hub) = shared_setup().await;
    let sub = "audit-ok-bearer";
    let token = provider.sign(&ClaimsBuilder::from_provider(provider).sub(sub).to_value());

    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let events = snapshot_events();
    let hit = events.iter().find(|e| {
        e.fields.get("action").map(|s| s.as_str()) == Some("auth_ok")
            && e.fields.get("sub").map(|s| s.as_str()) == Some(sub)
    });
    let hit = hit.expect("expected auth_ok audit event with matching sub");
    assert_eq!(
        hit.fields.get("credential_kind").map(|s| s.as_str()),
        Some("bearer")
    );
    assert_eq!(hit.fields.get("outcome").map(|s| s.as_str()), Some("allow"));
}

#[tokio::test]
async fn audit_event_on_auth_fail() {
    // Three failure shapes, each with a distinct sub/correlation hook.

    // (1) bad credentials → 401, detail describes JWT failure.
    let (provider, hub) = shared_setup().await;
    let bad = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("audit-fail-badcreds")
            .iss("https://evil.example.com")
            .to_value(),
    );
    let r = hub.get_health().bearer_auth(&bad).send().await.unwrap();
    assert_eq!(r.status(), 401);

    // (2) good creds, not allowlisted → 403, detail = "user_not_allowlisted".
    let (provider_a, hub_a) = allowlist_setup().await;
    let t = provider_a.sign(
        &ClaimsBuilder::from_provider(provider_a)
            .sub("audit-fail-allowlist")
            .email("user@gmail.com")
            .to_value(),
    );
    let r = hub_a.get_health().bearer_auth(&t).send().await.unwrap();
    assert_eq!(r.status(), 403);

    // (3) dual credentials → 400, detail = "conflicting_credentials".
    let t2 = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("audit-fail-dual")
            .to_value(),
    );
    let r = hub
        .get_health()
        .bearer_auth(&t2)
        .header("cookie", format!("quarto_hub_token={t2}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    let events = snapshot_events();

    // (1) Bad credentials never reach OidcClaims so we identify via detail.
    assert!(
        events.iter().any(|e| {
            e.fields.get("action").map(|s| s.as_str()) == Some("auth_fail")
                && e.fields.get("outcome").map(|s| s.as_str()) == Some("deny")
                && e.fields.get("credential_kind").map(|s| s.as_str()) == Some("bearer")
                && e.fields.contains_key("detail")
        }),
        "expected auth_fail event for bad credentials"
    );

    // (2) Allowlist failure with literal detail.
    assert!(
        events.iter().any(|e| {
            e.fields.get("action").map(|s| s.as_str()) == Some("auth_fail")
                && e.fields.get("credential_kind").map(|s| s.as_str()) == Some("bearer")
                && e.fields.get("detail").map(|s| s.as_str()) == Some("user_not_allowlisted")
                && e.fields.get("sub").map(|s| s.as_str()) == Some("audit-fail-allowlist")
        }),
        "expected auth_fail event with detail=user_not_allowlisted"
    );

    // (3) Dual credential with literal detail.
    assert!(
        events.iter().any(|e| {
            e.fields.get("action").map(|s| s.as_str()) == Some("auth_fail")
                && e.fields.get("detail").map(|s| s.as_str()) == Some("conflicting_credentials")
        }),
        "expected auth_fail event with detail=conflicting_credentials"
    );
}

#[tokio::test]
async fn tracing_redacts_authorization_header() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("tracing-redaction")
            .to_value(),
    );

    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let events = snapshot_events();
    // Token must never appear verbatim anywhere in the trace stream.
    for ev in &events {
        for (k, v) in &ev.fields {
            assert!(!v.contains(&token), "token leaked in field {k}={v}");
            assert!(
                !v.contains("Bearer "),
                "raw `Bearer …` value present in field {k}={v}"
            );
        }
    }
}

/// Phase 10 regression: a request bearing Google-token-shaped substrings
/// (`ya29.*` access token, `1//*` refresh token) must not produce any
/// tracing event containing those substrings. The hub only ever sees
/// the ID token in real traffic, but a misconfigured client could send
/// any of these shapes — and a regression in either the request span
/// builder or the JWT-decode error formatter could surface them.
#[tokio::test]
async fn tracing_redacts_google_token_shapes() {
    let (_provider, hub) = shared_setup().await;

    // Synthetic strings shaped exactly like Google's access / refresh
    // tokens. Neither parses as a JWT, so the decoder will reject —
    // the point is to confirm no error path embeds the raw bytes.
    let access_shape = "ya29.fake-access-token-must-not-appear-in-logs";
    let refresh_shape = "1//0gfake-refresh-token-must-not-appear-in-logs";

    let r1 = hub
        .get_health()
        .bearer_auth(access_shape)
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 401);

    let r2 = hub
        .get_health()
        .header("cookie", format!("quarto_hub_token={refresh_shape}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 401);

    let events = snapshot_events();
    for ev in &events {
        for (k, v) in &ev.fields {
            assert!(
                !v.contains("ya29."),
                "Google access-token shape leaked in field {k}={v}"
            );
            assert!(
                !v.contains("1//"),
                "Google refresh-token shape leaked in field {k}={v}"
            );
            assert!(
                !v.contains(access_shape),
                "synthetic access token leaked in field {k}={v}"
            );
            assert!(
                !v.contains(refresh_shape),
                "synthetic refresh token leaked in field {k}={v}"
            );
            assert!(
                !v.contains("Bearer "),
                "raw `Bearer …` value present in field {k}={v}"
            );
        }
    }
}

// ── CSRF + Origin gating by credential kind ──────────────────────

#[tokio::test]
async fn mutating_endpoint_with_bearer_skips_csrf_check() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("csrf-bearer-skip")
            .to_value(),
    );

    // /auth/logout is a mutating POST that calls check_csrf in the cookie
    // path. Bearer must skip it.
    let resp = hub
        .post_auth_logout()
        .bearer_auth(&token)
        // Deliberately omit `X-Requested-With`.
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success() || resp.status() == 200,
        "Bearer-authenticated POST /auth/logout should not 403 on missing X-Requested-With (got {})",
        resp.status()
    );
}

// NOTE: the cookie-side CSRF regression (mutating POST with a valid
// cookie but no X-Requested-With must 403) lives in `session_auth.rs`
// now — since the sliding-sessions cutover (§6 hard break), a Google
// JWT in the cookie no longer authenticates, so exercising the CSRF
// gate requires a hub-minted session cookie.

#[tokio::test]
async fn dual_credential_400_wins_over_csrf_and_origin() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("dual-vs-csrf")
            .to_value(),
    );

    // Mutating endpoint, dual credential, **and** bad Origin/missing
    // X-Requested-With. The 400 must still win.
    let resp = hub
        .post_auth_logout()
        .bearer_auth(&token)
        .header("cookie", format!("quarto_hub_token={token}"))
        .header("origin", "https://attacker.example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

// ── Revocation ledger on the Bearer path (bd-jkih1ql7) ───────────
//
// Bans and logout-everywhere `not_before` floors must bite Bearer
// credentials too, not just session cookies — otherwise a banned user
// keeps full MCP access and a stolen Google ID token survives
// logout-everywhere for its remaining lifetime. The anchor is the
// Google token's `iat`; a missing `iat` fails closed. Plan:
// claude-notes/plans/2026-08-03-bearer-revocation-and-mcp-auth-followups.md (F1).

/// Hub with pre-written revocation events (the stopped-hub operator
/// procedure): one banned sub, plus per-test `not_before` floors
/// anchored to the fixture's start instant (returned as third element
/// so tests mint `iat`s relative to it, immune to test-order timing).
async fn revocation_setup() -> &'static (MockOidcProvider, TestHub, i64) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub, i64)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let now = chrono::Utc::now().timestamp();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .banned_subs(&["banned-bearer-sub"])
                .not_before_subs(&[
                    ("revoked-bearer-sub", now - 100),
                    ("no-iat-bearer-sub", now - 100),
                    ("self-heal-bearer-sub", now - 100),
                    // Future floor = the shape a live logout-everywhere
                    // writes (now + 1); lets the mint-path test present
                    // a token whose iat provably predates the floor.
                    ("mint-clamp-bearer-sub", now + 1),
                ])
                .start(&provider)
                .await;
            (provider, hub, now)
        })
        .await
}

#[tokio::test]
async fn bearer_banned_sub_returns_403() {
    let (provider, hub, now) = revocation_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("banned-bearer-sub")
            .iat(now - 5)
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(
        resp.status(),
        403,
        "banned sub must be refused on the Bearer path"
    );

    let events = snapshot_events();
    assert!(
        events.iter().any(|e| {
            e.fields.get("action").map(|s| s.as_str()) == Some("auth_fail")
                && e.fields.get("credential_kind").map(|s| s.as_str()) == Some("bearer")
                && e.fields.get("sub").map(|s| s.as_str()) == Some("banned-bearer-sub")
                && e.fields.get("detail").map(|s| s.as_str()) == Some("user_banned")
        }),
        "expected auth_fail with detail=user_banned, credential_kind=bearer"
    );
    // Ordering pin: the deny must happen before the auth_ok emission —
    // an allow-then-deny pair for one request would corrupt the audit log.
    assert!(
        !events.iter().any(|e| {
            e.fields.get("action").map(|s| s.as_str()) == Some("auth_ok")
                && e.fields.get("sub").map(|s| s.as_str()) == Some("banned-bearer-sub")
        }),
        "a denied Bearer must not leave an auth_ok event in the audit log"
    );
}

#[tokio::test]
async fn ws_upgrade_with_banned_bearer_returns_403() {
    let (provider, hub, now) = revocation_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("banned-bearer-sub")
            .iat(now - 5)
            .to_value(),
    );
    let resp = hub.ws_upgrade().bearer_auth(&token).send().await.unwrap();
    assert_eq!(
        resp.status(),
        403,
        "banned sub must be refused on the WS upgrade too"
    );
}

#[tokio::test]
async fn bearer_with_iat_before_not_before_returns_401() {
    let (provider, hub, now) = revocation_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("revoked-bearer-sub")
            .iat(now - 200) // predates the now-100 floor
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(
        resp.status(),
        401,
        "a Bearer minted before the not_before floor must be refused"
    );

    let events = snapshot_events();
    assert!(
        events.iter().any(|e| {
            e.fields.get("action").map(|s| s.as_str()) == Some("auth_fail")
                && e.fields.get("credential_kind").map(|s| s.as_str()) == Some("bearer")
                && e.fields.get("sub").map(|s| s.as_str()) == Some("revoked-bearer-sub")
                && e.fields.get("detail").map(|s| s.as_str()) == Some("bearer_revoked")
        }),
        "expected auth_fail with detail=bearer_revoked (not session_revoked — it isn't a session)"
    );
    assert!(
        !events.iter().any(|e| {
            e.fields.get("action").map(|s| s.as_str()) == Some("auth_ok")
                && e.fields.get("sub").map(|s| s.as_str()) == Some("revoked-bearer-sub")
        }),
        "a denied Bearer must not leave an auth_ok event in the audit log"
    );
}

#[tokio::test]
async fn bearer_without_iat_fails_closed_when_not_before_exists() {
    let (provider, hub, _now) = revocation_setup().await;
    // OIDC requires iat and Google always sends it, but OidcClaims.iat
    // is Option<i64> — an iat-less token must anchor at 0 and die
    // against any not_before entry rather than sail past the check.
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("no-iat-bearer-sub")
            .no_iat()
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(
        resp.status(),
        401,
        "an iat-less Bearer must fail closed against a not_before entry"
    );
}

#[tokio::test]
async fn bearer_minted_after_revocation_authenticates() {
    let (provider, hub, now) = revocation_setup().await;
    // The self-heal path: a legitimate MCP client caught by
    // logout-everywhere refreshes, gets a fresh iat ≥ not_before, and
    // is back in — same "immediate re-login works" semantics as the
    // browser.
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("self-heal-bearer-sub")
            .iat(now - 5) // after the now-100 floor
            .to_value(),
    );
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "a Bearer minted after the revocation instant must authenticate; body: {:?}",
        resp.text().await
    );
}

#[tokio::test]
async fn bearer_revocation_does_not_leak_into_mint_path() {
    let (provider, hub, now) = revocation_setup().await;
    // A token whose iat predates the not_before floor: refused as a
    // request credential…
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("mint-clamp-bearer-sub")
            .iat(now - 5) // predates the now+1 floor
            .to_value(),
    );
    let r = hub.get_health().bearer_auth(&google).send().await.unwrap();
    assert_eq!(
        r.status(),
        401,
        "pre-revocation iat must be refused as a request credential"
    );

    // …but the same credential still mints a session: the mint path's
    // min_auth_time clamp (not a raw iat check) is what handles the
    // floor there, so same-second re-login keeps working.
    let r = hub
        .client
        .post(hub.url("/auth/session"))
        .header("x-requested-with", "XMLHttpRequest")
        .json(&serde_json::json!({ "credential": google }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        200,
        "mint must not enforce the Bearer iat floor (min_auth_time clamp)"
    );
    let (cookie, _) = TestHub::set_auth_cookie(&r).expect("fresh session cookie");
    let r = hub
        .get_health()
        .header("cookie", format!("quarto_hub_token={cookie}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "post-clamp session authenticates");
}

// ── Unauthenticated endpoints unaffected ─────────────────────────

#[tokio::test]
async fn unauthenticated_endpoint_unaffected() {
    let (_provider, hub) = shared_setup().await;
    // 404 endpoint: must return 404, not auth status. Confirms we're not
    // applying auth across the router.
    let resp = hub
        .client
        .get(hub.url("/does/not/exist"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
