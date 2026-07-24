//! Hub-minted sliding sessions — integration tests (epic `bd-ey6jg70f`).
//!
//! A fixture hub is started with a **known session secret**
//! ([`crate::support::TEST_SESSION_SECRET`]), so tests can mint session
//! tokens out-of-band via `quarto_hub::session` and drive the hub's
//! HTTP surface with them.
//!
//! Phase coverage:
//! * C2 — cookie path routes to session verify: session cookie accepted,
//!   legacy Google-JWT cookie rejected (the §6 hard break), cross-path
//!   rejection, expired/tampered rejection, allowlist re-check,
//!   distinguishable failure logging, dual-credential 400 preserved.
//! * C3/C5 tests land with their phases (sliding re-issue wiring,
//!   revocation store).
//!
//! Plan: claude-notes/plans/2026-07-06-hub-server-minted-sliding-sessions.md

use quarto_hub::session::{
    SessionIdentity, SessionKeys, SessionLifetimes, mint_session, sign_claims,
};

use crate::support::{
    ClaimsBuilder, MockOidcProvider, TEST_SESSION_SECRET, TestHub, TestHubBuilder,
    install_tracing_once, snapshot_events,
};

// ── fixtures ──────────────────────────────────────────────────────

/// Default hub (no allowlist) with the known session secret.
async fn session_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .session_secret(TEST_SESSION_SECRET)
                .start(&provider)
                .await;
            (provider, hub)
        })
        .await
}

/// Allowlist hub (`allowed_domains = ["posit.co"]`) with the known
/// session secret — exercises the per-request allowlist re-check.
async fn session_allowlist_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .allowed_domains(&["posit.co"])
                .session_secret(TEST_SESSION_SECRET)
                .start(&provider)
                .await;
            (provider, hub)
        })
        .await
}

fn test_keys() -> SessionKeys {
    SessionKeys::new(TEST_SESSION_SECRET)
}

fn epoch_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn identity(sub: &str, email: &str) -> SessionIdentity {
    SessionIdentity {
        sub: sub.to_string(),
        email: email.to_string(),
        email_verified: true,
        name: Some("Session Test User".to_string()),
        picture: None,
    }
}

/// Mint a session token the fixture hub must accept.
fn mint_test_session(sub: &str, email: &str) -> String {
    mint_session(
        &test_keys(),
        SessionLifetimes::default(),
        &identity(sub, email),
        epoch_now(),
    )
    .expect("mint session token")
}

fn cookie_header(token: &str) -> String {
    format!("quarto_hub_token={token}")
}

// ── C2: session cookie accepted on the central path ───────────────

#[tokio::test]
async fn session_cookie_authenticates_on_extractor_endpoint() {
    let (_provider, hub) = session_setup().await;
    let token = mint_test_session("session-ok-sub", "user@posit.co");

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "body: {:?}", resp.text().await);
}

#[tokio::test]
async fn session_cookie_authenticates_ws_upgrade() {
    let (_provider, hub) = session_setup().await;
    let token = mint_test_session("session-ws-sub", "user@posit.co");

    let resp = hub
        .ws_upgrade()
        .header("origin", &hub.base_url)
        .header("cookie", cookie_header(&token))
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

// ── C2/C4: hard break — legacy Google-JWT cookies rejected ────────

#[tokio::test]
async fn legacy_google_jwt_cookie_rejected_401() {
    let (provider, hub) = session_setup().await;
    // A Google-style RS256 ID token — exactly what pre-cutover cookies
    // hold. The session verifier must fail closed (unknown kid), 401.
    let google_token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("legacy-cookie-sub")
            .to_value(),
    );

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&google_token))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "legacy Google-JWT cookie must 401 (hard break, §6)"
    );
    // Clean JSON error body — the SPA's normal logged-out flow, no
    // redirect loop.
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "unauthorized");
}

// ── C2: cross-path rejection ──────────────────────────────────────

#[tokio::test]
async fn session_token_as_bearer_rejected_401() {
    let (_provider, hub) = session_setup().await;
    let token = mint_test_session("cross-path-sub", "user@posit.co");

    // A hub session token presented as Authorization: Bearer must fail
    // JWKS verify (HS256 is never a JWKS-declared algorithm).
    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(
        resp.status(),
        401,
        "session token on the Bearer path must be rejected"
    );
}

// ── C2: expired / tampered session cookies ────────────────────────

#[tokio::test]
async fn expired_session_cookie_rejected_401() {
    let (_provider, hub) = session_setup().await;
    let lt = SessionLifetimes::default();
    // Minted far enough in the past that exp (idle-bound) has lapsed.
    let token = mint_session(
        &test_keys(),
        lt,
        &identity("expired-sub", "user@posit.co"),
        epoch_now() - lt.idle_secs - 3600,
    )
    .unwrap();

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn tampered_session_cookie_rejected_401() {
    let (_provider, hub) = session_setup().await;
    let token = mint_test_session("tampered-sub", "user@posit.co");
    // Bit-flip inside the payload segment.
    let mut parts: Vec<String> = token.split('.').map(String::from).collect();
    let mut payload = parts[1].clone().into_bytes();
    let i = payload.len() / 2;
    payload[i] = if payload[i] == b'A' { b'B' } else { b'A' };
    parts[1] = String::from_utf8(payload).unwrap();
    let tampered = parts.join(".");

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&tampered))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

// ── C2: absolute cap enforced on the HTTP path ────────────────────

#[tokio::test]
async fn session_past_absolute_cap_rejected_401() {
    let (_provider, hub) = session_setup().await;
    let now = epoch_now();
    let lt = SessionLifetimes::default();
    // Forged shape a re-issue bug could produce: future exp, auth_time
    // past the absolute cap. Signed with the real hub secret.
    let claims = quarto_hub::session::SessionClaims {
        iss: quarto_hub::session::SESSION_ISSUER.to_string(),
        sub: "cap-sub".into(),
        email: "user@posit.co".into(),
        email_verified: true,
        name: None,
        picture: None,
        iat: now,
        auth_time: now - lt.absolute_secs - 1,
        exp: now + 600,
        sid: "f00df00df00df00df00df00df00df00d".into(),
    };
    let token = sign_claims(&test_keys(), &claims).unwrap();

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

// ── C2: per-request allowlist re-check ────────────────────────────

#[tokio::test]
async fn allowlist_removal_bites_on_next_request() {
    let (_provider, hub) = session_allowlist_setup().await;
    // Valid, unexpired session token whose user is not (or no longer)
    // in the allowlist — removal must bite on the next request, not at
    // absolute expiry.
    let token = mint_test_session("removed-user-sub", "user@gmail.com");

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "session claims must be re-checked against the allowlist per request"
    );
}

#[tokio::test]
async fn allowlisted_session_authenticates() {
    let (_provider, hub) = session_allowlist_setup().await;
    let token = mint_test_session("allowed-user-sub", "user@posit.co");

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

// ── C2: dual-credential 400 preserved with session cookies ────────

#[tokio::test]
async fn session_cookie_plus_bearer_still_400() {
    let (provider, hub) = session_setup().await;
    let session = mint_test_session("dual-session-sub", "user@posit.co");
    let bearer = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("dual-session-sub")
            .to_value(),
    );

    let resp = hub
        .get_health()
        .bearer_auth(&bearer)
        .header("cookie", cookie_header(&session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "conflicting_credentials");
}

// ── C2: cookie-kind CSRF gate still applies to session cookies ────

#[tokio::test]
async fn mutating_endpoint_with_session_cookie_still_requires_csrf() {
    let (_provider, hub) = session_setup().await;
    let token = mint_test_session("csrf-session-sub", "user@posit.co");

    let resp = hub
        .post_auth_logout()
        .header("cookie", cookie_header(&token))
        // No X-Requested-With.
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "cookie-authenticated POST without X-Requested-With must 403"
    );
}

#[tokio::test]
async fn mutating_endpoint_with_session_cookie_and_csrf_header_succeeds() {
    let (_provider, hub) = session_setup().await;
    let token = mint_test_session("csrf-ok-session-sub", "user@posit.co");

    let resp = hub
        .post_auth_logout()
        .header("cookie", cookie_header(&token))
        .header("x-requested-with", "XMLHttpRequest")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

// ── C2: distinguishable failure logging ───────────────────────────

#[tokio::test]
async fn session_verify_failures_are_logged_distinguishably() {
    let (provider, hub) = session_setup().await;

    // (1) kid mismatch: a Google-style token in the cookie (no hub kid).
    let google_token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("log-kid-mismatch")
            .to_value(),
    );
    let r = hub
        .get_health()
        .header("cookie", cookie_header(&google_token))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // (2) expired session token.
    let lt = SessionLifetimes::default();
    let expired = mint_session(
        &test_keys(),
        lt,
        &identity("log-expired", "user@posit.co"),
        epoch_now() - lt.idle_secs - 3600,
    )
    .unwrap();
    let r = hub
        .get_health()
        .header("cookie", cookie_header(&expired))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // (3) tampered session token.
    let token = mint_test_session("log-tampered", "user@posit.co");
    let mut parts: Vec<String> = token.split('.').map(String::from).collect();
    let mut payload = parts[1].clone().into_bytes();
    let i = payload.len() / 2;
    payload[i] = if payload[i] == b'A' { b'B' } else { b'A' };
    parts[1] = String::from_utf8(payload).unwrap();
    let r = hub
        .get_health()
        .header("cookie", cookie_header(&parts.join(".")))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    let events = snapshot_events();
    let has_detail = |needle: &str| {
        events.iter().any(|e| {
            e.fields.get("action").map(|s| s.as_str()) == Some("auth_fail")
                && e.fields.get("credential_kind").map(|s| s.as_str()) == Some("cookie")
                && e.fields.get("detail").is_some_and(|d| d.contains(needle))
        })
    };
    assert!(
        has_detail("kid_mismatch"),
        "expected a kid_mismatch auth_fail event"
    );
    assert!(has_detail("expired"), "expected an expired auth_fail event");
    assert!(
        has_detail("tampered"),
        "expected a tampered auth_fail event"
    );

    // Token contents must never be logged.
    for ev in &events {
        for (k, v) in &ev.fields {
            assert!(
                !v.contains(&google_token) && !v.contains(&token),
                "token contents leaked in field {k}"
            );
        }
    }
}

// ── C2: Bearer path untouched by session routing ──────────────────

#[tokio::test]
async fn bearer_google_token_still_authenticates_on_session_hub() {
    let (provider, hub) = session_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("bearer-untouched-sub")
            .to_value(),
    );

    let resp = hub.get_health().bearer_auth(&token).send().await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "MCP Bearer path must remain JWKS-verified and unaffected"
    );
}
