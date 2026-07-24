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
    SessionIdentity, SessionKeys, SessionLifetimes, mint_session, sign_claims, verify_session,
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

#[tokio::test]
async fn ws_upgrade_rejects_legacy_google_cookie() {
    let (provider, hub) = session_setup().await;
    let google_token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("legacy-ws-sub")
            .to_value(),
    );

    let resp = hub
        .ws_upgrade()
        .header("origin", &hub.base_url)
        .header("cookie", cookie_header(&google_token))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "hard break applies to the WS upgrade too"
    );
}

/// The failure path of the cutover is a clean logged-out flow: one
/// redirect to `/?auth_error`, no cookie set, no loop.
#[tokio::test]
async fn callback_failure_redirects_once_without_cookie() {
    let (_provider, hub) = google_session_setup().await;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .post(hub.url("/auth/callback"))
        .header("cookie", "g_csrf_token=csrf123")
        .header("content-type", "application/x-www-form-urlencoded")
        .body("credential=not-a-valid-jwt&g_csrf_token=csrf123")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_redirection());
    assert_eq!(resp.headers().get("location").unwrap(), "/?auth_error");
    assert!(
        TestHub::set_auth_cookie(&resp).is_none(),
        "no cookie may be set on a failed login"
    );
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

// ── C3: login mints a session cookie ──────────────────────────────

#[tokio::test]
async fn auth_refresh_mints_session_cookie() {
    let (provider, hub) = session_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("refresh-mint-sub")
            .email("minted@posit.co")
            .to_value(),
    );

    let resp = hub
        .client
        .post(hub.url("/auth/refresh"))
        .header("x-requested-with", "XMLHttpRequest")
        .json(&serde_json::json!({ "credential": google }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let (value, attrs) = TestHub::set_auth_cookie(&resp).expect("session cookie set");
    assert_ne!(
        value, google,
        "cookie must hold a hub session token, not the Google JWT"
    );

    // The minted token verifies under the hub's known session secret,
    // anchored at login time with the idle-bound sliding exp.
    let now = epoch_now();
    let lt = SessionLifetimes::default();
    let v = verify_session(&test_keys(), lt, &value, now).unwrap();
    assert_eq!(v.claims.sub, "refresh-mint-sub");
    assert_eq!(v.claims.email, "minted@posit.co");
    assert!(
        (v.claims.auth_time - now).abs() <= 5,
        "auth_time anchored at the login instant"
    );
    assert_eq!(v.claims.exp, v.claims.iat + lt.idle_secs);
    assert_eq!(v.claims.sid.len(), 32, "fresh random sid");

    // Cookie lifetime matches the token's sliding exp; attributes hold.
    assert!(attrs.contains("HttpOnly"));
    assert!(attrs.contains("SameSite=Lax"));
    assert!(attrs.contains("Path=/"));
    assert!(attrs.contains(&format!("Max-Age={}", lt.idle_secs)));
}

/// Google-provider hub (registers the form-POST `/auth/callback`).
async fn google_session_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .google_provider()
                .session_secret(TEST_SESSION_SECRET)
                .start(&provider)
                .await;
            (provider, hub)
        })
        .await
}

#[tokio::test]
async fn auth_callback_mints_session_cookie() {
    let (provider, hub) = google_session_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("callback-mint-sub")
            .to_value(),
    );

    // No-redirect client so the Set-Cookie on the redirect is observable.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    // Manual x-www-form-urlencoded body (reqwest is built without the
    // `form` feature here); JWT characters need no escaping.
    let resp = client
        .post(hub.url("/auth/callback"))
        .header("cookie", "g_csrf_token=csrf123")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("credential={google}&g_csrf_token=csrf123"))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_redirection(),
        "expected redirect, got {}",
        resp.status()
    );
    assert_eq!(resp.headers().get("location").unwrap(), "/");
    let (value, _) = TestHub::set_auth_cookie(&resp).expect("session cookie set");
    assert_ne!(value, google);
    let v = verify_session(
        &test_keys(),
        SessionLifetimes::default(),
        &value,
        epoch_now(),
    )
    .unwrap();
    assert_eq!(v.claims.sub, "callback-mint-sub");
}

#[tokio::test]
async fn large_google_token_no_longer_cookie_dropped() {
    let (provider, hub) = session_setup().await;
    // Pad a claim the session token does NOT carry (`nonce`) so the
    // Google JWT exceeds the ~3800-byte browser cookie limit while the
    // minted session token stays compact.
    let mut claims = ClaimsBuilder::from_provider(provider)
        .sub("large-token-sub")
        .to_value();
    claims
        .as_object_mut()
        .unwrap()
        .insert("nonce".into(), serde_json::json!("x".repeat(4000)));
    let google = provider.sign(&claims);
    assert!(google.len() > 3800, "fixture token must exceed the limit");

    let resp = hub
        .client
        .post(hub.url("/auth/refresh"))
        .header("x-requested-with", "XMLHttpRequest")
        .json(&serde_json::json!({ "credential": google }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let (value, _) = TestHub::set_auth_cookie(&resp).expect("session cookie set");
    assert!(
        value.len() < 1024,
        "session cookie stays compact ({} bytes)",
        value.len()
    );
}

// ── C3: /auth/me and /auth/actor on the session path ─────────────

#[tokio::test]
async fn auth_me_returns_sliding_exp_from_session() {
    let (_provider, hub) = session_setup().await;
    let before = epoch_now();
    let token = mint_test_session("me-session-sub", "user@posit.co");

    let resp = hub
        .get_auth_me()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["email"], "user@posit.co");
    assert_eq!(body["name"], "Session Test User");
    let exp = body["exp"].as_i64().unwrap();
    let idle = SessionLifetimes::default().idle_secs;
    assert!(
        exp >= before + idle && exp <= epoch_now() + idle,
        "exp must be the sliding session expiry (~now + idle), got {exp}"
    );
}

#[tokio::test]
async fn auth_me_supports_bearer() {
    // bd-3g0aijb3: /auth/me routes through shared credential extraction,
    // so the MCP Bearer path works there too.
    let (provider, hub) = session_setup().await;
    let exp = chrono::Utc::now().timestamp() + 600;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("me-bearer-sub")
            .exp(exp)
            .to_value(),
    );

    let resp = hub.get_auth_me().bearer_auth(&google).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["email"], "user@posit.co");
    assert_eq!(body["exp"], exp, "Bearer path reports the Google exp");
}

#[tokio::test]
async fn auth_me_rejects_dual_credentials() {
    let (provider, hub) = session_setup().await;
    let session = mint_test_session("me-dual-sub", "user@posit.co");
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("me-dual-sub")
            .to_value(),
    );

    let resp = hub
        .get_auth_me()
        .bearer_auth(&google)
        .header("cookie", cookie_header(&session))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "/auth/me must apply the dual-credential rule"
    );
}

#[tokio::test]
async fn auth_me_rejects_legacy_google_cookie() {
    let (provider, hub) = session_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("me-legacy-sub")
            .to_value(),
    );

    let resp = hub
        .get_auth_me()
        .header("cookie", cookie_header(&google))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "hard break applies to /auth/me too");
}

#[tokio::test]
async fn auth_actor_works_with_session_cookie() {
    let (_provider, hub) = session_setup().await;
    let token = mint_test_session("actor-session-sub", "user@posit.co");

    let get = |project: &str| {
        hub.client
            .get(hub.url(&format!("/auth/actor?project={project}")))
            .header("cookie", cookie_header(&token))
    };

    let r1 = get("proj-a").send().await.unwrap();
    assert_eq!(r1.status(), 200);
    let a1: serde_json::Value = r1.json().await.unwrap();
    let id_a = a1["actor_id"].as_str().unwrap().to_string();
    assert_eq!(id_a.len(), 64, "HMAC-SHA256 hex actor id");

    // Deterministic per (sub, project); different across projects.
    let r2 = get("proj-a").send().await.unwrap();
    let a2: serde_json::Value = r2.json().await.unwrap();
    assert_eq!(a2["actor_id"].as_str().unwrap(), id_a);

    let r3 = get("proj-b").send().await.unwrap();
    let a3: serde_json::Value = r3.json().await.unwrap();
    assert_ne!(a3["actor_id"].as_str().unwrap(), id_a);
}

#[tokio::test]
async fn auth_actor_supports_bearer() {
    // bd-3g0aijb3 regression: MCP sessions must obtain the per-project
    // HMAC actor id over Bearer — previously /auth/actor was
    // cookie-only and audit-logged missing_credential, so agent edits
    // fell back to random actors silently.
    let (provider, hub) = session_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("actor-bearer-sub")
            .to_value(),
    );

    let resp = hub
        .client
        .get(hub.url("/auth/actor?project=proj-mcp"))
        .bearer_auth(&google)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "actor acquisition over Bearer");
    let body: serde_json::Value = resp.json().await.unwrap();
    let actor_id = body["actor_id"].as_str().unwrap();
    assert_eq!(actor_id.len(), 64, "HMAC-SHA256 hex actor id");
}

// ── C3: sliding re-issue on authenticated activity ───────────────

#[tokio::test]
async fn old_session_reissued_on_activity() {
    let (_provider, hub) = session_setup().await;
    let lt = SessionLifetimes::default();
    // Minted 2h ago: past the Google 1h token lifetime (no One-Tap
    // involved) and past the 1h re-issue threshold, well within idle.
    let minted_at = epoch_now() - 2 * 3600;
    let token = mint_session(
        &test_keys(),
        lt,
        &identity("reissue-sub", "user@posit.co"),
        minted_at,
    )
    .unwrap();
    let original = verify_session(&test_keys(), lt, &token, epoch_now())
        .unwrap()
        .claims;

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "session survives past 1h, no One-Tap");

    let (value, attrs) = TestHub::set_auth_cookie(&resp).expect("re-issued cookie");
    let now = epoch_now();
    let re = verify_session(&test_keys(), lt, &value, now)
        .unwrap()
        .claims;
    assert_eq!(re.auth_time, original.auth_time, "auth_time is immutable");
    assert_eq!(re.sid, original.sid, "sid is immutable");
    assert!((re.iat - now).abs() <= 5, "iat advances to re-issue time");
    assert_eq!(re.exp, re.iat + lt.idle_secs, "exp slides");

    // Attributes preserved on re-issue (insecure fixture → no Secure).
    assert!(attrs.contains("HttpOnly"));
    assert!(attrs.contains("SameSite=Lax"));
    assert!(attrs.contains("Path=/"));
    assert!(!attrs.contains("Secure"));
}

#[tokio::test]
async fn fresh_session_not_reissued() {
    let (_provider, hub) = session_setup().await;
    let token = mint_test_session("fresh-noreissue-sub", "user@posit.co");

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(
        TestHub::set_auth_cookie(&resp).is_none(),
        "tokens younger than 1h must not be re-issued (Set-Cookie churn)"
    );
}

#[tokio::test]
async fn bearer_response_never_reissues_cookie() {
    let (provider, hub) = session_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("bearer-noreissue-sub")
            .to_value(),
    );

    let resp = hub.get_health().bearer_auth(&google).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    assert!(
        TestHub::set_auth_cookie(&resp).is_none(),
        "a Bearer/MCP response must never set a session cookie"
    );
}

#[tokio::test]
async fn failed_auth_never_reissues_cookie() {
    let (_provider, hub) = session_setup().await;
    let lt = SessionLifetimes::default();
    let token = mint_session(
        &test_keys(),
        lt,
        &identity("tamper-noreissue-sub", "user@posit.co"),
        epoch_now() - 2 * 3600,
    )
    .unwrap();
    // Tamper so validation fails while the token still "looks" old
    // enough to trigger re-issue.
    let mut parts: Vec<String> = token.split('.').map(String::from).collect();
    let mut payload = parts[1].clone().into_bytes();
    let i = payload.len() / 2;
    payload[i] = if payload[i] == b'A' { b'B' } else { b'A' };
    parts[1] = String::from_utf8(payload).unwrap();

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&parts.join(".")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    assert!(TestHub::set_auth_cookie(&resp).is_none());
}

#[tokio::test]
async fn non_allowlisted_old_session_not_reissued() {
    let (_provider, hub) = session_allowlist_setup().await;
    let lt = SessionLifetimes::default();
    // Old enough to qualify for re-issue, but the user fails the
    // allowlist re-check — condition (b) must block the extension.
    let token = mint_session(
        &test_keys(),
        lt,
        &identity("removed-noreissue-sub", "user@gmail.com"),
        epoch_now() - 2 * 3600,
    )
    .unwrap();

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    assert!(TestHub::set_auth_cookie(&resp).is_none());
}

#[tokio::test]
async fn reissue_preserves_secure_flag_on_secure_hub() {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    let (_provider, hub) = SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .secure()
                .session_secret(TEST_SESSION_SECRET)
                .start(&provider)
                .await;
            (provider, hub)
        })
        .await;

    let lt = SessionLifetimes::default();
    let token = mint_session(
        &test_keys(),
        lt,
        &identity("secure-reissue-sub", "user@posit.co"),
        epoch_now() - 2 * 3600,
    )
    .unwrap();

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let (_, attrs) = TestHub::set_auth_cookie(&resp).expect("re-issued cookie");
    assert!(attrs.contains("Secure"), "Secure preserved on re-issue");
}

#[tokio::test]
async fn logout_clear_cookie_not_overridden_by_reissue() {
    let (_provider, hub) = session_setup().await;
    let lt = SessionLifetimes::default();
    // Old enough that the re-issue layer would fire — logout's clearing
    // Set-Cookie must win.
    let token = mint_session(
        &test_keys(),
        lt,
        &identity("logout-reissue-sub", "user@posit.co"),
        epoch_now() - 2 * 3600,
    )
    .unwrap();

    let resp = hub
        .post_auth_logout()
        .header("cookie", cookie_header(&token))
        .header("x-requested-with", "XMLHttpRequest")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let (value, attrs) = TestHub::set_auth_cookie(&resp).expect("clearing cookie");
    assert_eq!(value, "", "logout must clear, not re-issue");
    assert!(attrs.contains("Max-Age=0"));
}

#[tokio::test]
async fn ws_upgrade_never_reissues_cookie() {
    let (_provider, hub) = session_setup().await;
    let lt = SessionLifetimes::default();
    let token = mint_session(
        &test_keys(),
        lt,
        &identity("ws-noreissue-sub", "user@posit.co"),
        epoch_now() - 2 * 3600,
    )
    .unwrap();

    let resp = hub
        .ws_upgrade()
        .header("origin", &hub.base_url)
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 101);
    assert!(
        TestHub::set_auth_cookie(&resp).is_none(),
        "Set-Cookie on a 101 is unreliable; WS must never slide the window"
    );
}

// ── C5: revocation-event store ────────────────────────────────────

#[tokio::test]
async fn logout_everywhere_kills_prior_tokens_and_relogin_works() {
    let (provider, hub) = session_setup().await;
    let lt = SessionLifetimes::default();
    let sub = "logout-everywhere-sub";

    // Three members of the same user's token family: the presenting
    // device (A), a re-issued/parallel sibling (B), and a token minted
    // in the *same second* as the revocation (C) — the second-
    // granularity edge the first e2e run caught: with a strict `<`
    // against `now`, same-second logins survived logout-everywhere.
    let token_a = mint_session(
        &test_keys(),
        lt,
        &identity(sub, "user@posit.co"),
        epoch_now() - 2 * 3600,
    )
    .unwrap();
    let token_b = mint_session(
        &test_keys(),
        lt,
        &identity(sub, "user@posit.co"),
        epoch_now() - 3600,
    )
    .unwrap();
    let token_c = mint_session(
        &test_keys(),
        lt,
        &identity(sub, "user@posit.co"),
        epoch_now(),
    )
    .unwrap();

    let hub_json_before = std::fs::read_to_string(hub.data_dir.join("hub.json")).unwrap();

    // Self-service revocation from device A.
    let resp = hub
        .client
        .post(hub.url("/auth/logout-everywhere"))
        .header("cookie", cookie_header(&token_a))
        .header("x-requested-with", "XMLHttpRequest")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "body: {:?}", resp.text().await);
    let (value, attrs) = TestHub::set_auth_cookie(&resp).expect("clearing cookie");
    assert_eq!(value, "", "caller's cookie is cleared");
    assert!(attrs.contains("Max-Age=0"));

    // The whole family is dead — including the re-issued sibling and
    // the same-second mint — and dead tokens are never re-issued.
    for token in [&token_a, &token_b, &token_c] {
        let r = hub
            .get_health()
            .header("cookie", cookie_header(token))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401, "revoked family member must be rejected");
        assert!(TestHub::set_auth_cookie(&r).is_none());
    }

    // Immediate re-login works (auth_time >= not_before).
    let google = provider.sign(&ClaimsBuilder::from_provider(provider).sub(sub).to_value());
    let r = hub
        .client
        .post(hub.url("/auth/refresh"))
        .header("x-requested-with", "XMLHttpRequest")
        .json(&serde_json::json!({ "credential": google }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let (fresh, _) = TestHub::set_auth_cookie(&r).expect("fresh session");
    let r = hub
        .get_health()
        .header("cookie", cookie_header(&fresh))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "fresh post-revocation login authenticates");

    // Revocations persist in their own file and never touch hub.json
    // (which holds the signing secrets).
    assert!(hub.data_dir.join("revocations.json").exists());
    let hub_json_after = std::fs::read_to_string(hub.data_dir.join("hub.json")).unwrap();
    assert_eq!(
        hub_json_before, hub_json_after,
        "revocation writes must never touch hub.json"
    );
    let dir_has_tmp = std::fs::read_dir(&hub.data_dir)
        .unwrap()
        .any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp"));
    assert!(!dir_has_tmp, "atomic persist must not leave temp files");
}

#[tokio::test]
async fn ban_gates_verify_and_mint() {
    // The builder pre-writes revocations.json with the ban — this IS
    // the documented stopped-hub operator procedure, so this test also
    // covers "hand-added ban enforced after restart" (the load path).
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    let (provider, hub) = SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .session_secret(TEST_SESSION_SECRET)
                .banned_subs(&["banned-sub"])
                .start(&provider)
                .await;
            (provider, hub)
        })
        .await;

    // Verify path: a valid, unexpired session token for the banned sub
    // is refused.
    let token = mint_test_session("banned-sub", "banned@posit.co");
    let r = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "banned sub must be refused at verify");
    assert!(TestHub::set_auth_cookie(&r).is_none());

    // Mint path: a banned sub re-logging in via Google is refused —
    // otherwise a ban is just one OAuth round-trip away from useless.
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("banned-sub")
            .email("banned@posit.co")
            .to_value(),
    );
    let r = hub
        .client
        .post(hub.url("/auth/refresh"))
        .header("x-requested-with", "XMLHttpRequest")
        .json(&serde_json::json!({ "credential": google }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "banned sub must be refused at mint");
    assert!(TestHub::set_auth_cookie(&r).is_none());

    // Other users are unaffected.
    let ok = mint_test_session("not-banned-sub", "user@posit.co");
    let r = hub
        .get_health()
        .header("cookie", cookie_header(&ok))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn logout_everywhere_requires_csrf_and_cookie_kind() {
    let (provider, hub) = session_setup().await;
    let token = mint_test_session("logout-gates-sub", "user@posit.co");

    // Cookie without the CSRF header → 403.
    let r = hub
        .client
        .post(hub.url("/auth/logout-everywhere"))
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "CSRF header required");

    // Bearer-kind caller → rejected (revocation is a browser-session
    // self-service action).
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("logout-gates-sub")
            .to_value(),
    );
    let r = hub
        .client
        .post(hub.url("/auth/logout-everywhere"))
        .bearer_auth(&google)
        .header("x-requested-with", "XMLHttpRequest")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "Bearer-kind callers are rejected");

    // Dual credential → 400 wins.
    let r = hub
        .client
        .post(hub.url("/auth/logout-everywhere"))
        .bearer_auth(&google)
        .header("cookie", cookie_header(&token))
        .header("x-requested-with", "XMLHttpRequest")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    // No credential → 401.
    let r = hub
        .client
        .post(hub.url("/auth/logout-everywhere"))
        .header("x-requested-with", "XMLHttpRequest")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // The gate checks must not have revoked anything: the token still works.
    let r = hub
        .get_health()
        .header("cookie", cookie_header(&token))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
}

// ── C5b: secret rotation via kid overlap ──────────────────────────

/// The pre-rotation secret; the rotated hub signs under
/// `TEST_SESSION_SECRET` and verifies both during the overlap window.
const OLD_SESSION_SECRET: [u8; 32] = [0x24; 32];

/// Hub mid-graceful-rotation: current = TEST_SESSION_SECRET,
/// previous = OLD_SESSION_SECRET, rotated_at = hub start.
async fn rotated_session_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .session_secret(TEST_SESSION_SECRET)
                .previous_session_secret(OLD_SESSION_SECRET)
                .start(&provider)
                .await;
            (provider, hub)
        })
        .await
}

#[tokio::test]
async fn graceful_rotation_old_cookie_verifies_and_is_reminted_under_new_kid() {
    let (_provider, hub) = rotated_session_setup().await;
    let lt = SessionLifetimes::default();
    // A cookie minted under the OLD secret, fresh (age < 1 h): the
    // non-current kid alone must trigger prompt re-issue (§2c).
    let old_keys = SessionKeys::new(OLD_SESSION_SECRET);
    let old_token = mint_session(
        &old_keys,
        lt,
        &identity("rotation-sub", "user@posit.co"),
        epoch_now(),
    )
    .unwrap();
    let original = verify_session(&old_keys, lt, &old_token, epoch_now())
        .unwrap()
        .claims;

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&old_token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "old-kid cookie verifies during overlap");

    let (value, _) = TestHub::set_auth_cookie(&resp).expect("prompt re-mint under the new kid");
    let header = jsonwebtoken::decode_header(&value).unwrap();
    assert_eq!(
        header.kid.as_deref(),
        Some(SessionKeys::new(TEST_SESSION_SECRET).current().kid()),
        "re-issued cookie carries the new kid"
    );
    // Session continuity across the rotation: same family, same anchor.
    let re = verify_session(&test_keys(), lt, &value, epoch_now())
        .unwrap()
        .claims;
    assert_eq!(re.auth_time, original.auth_time);
    assert_eq!(re.sid, original.sid);
}

#[tokio::test]
async fn new_logins_on_rotated_hub_carry_new_kid() {
    let (provider, hub) = rotated_session_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("rotation-login-sub")
            .to_value(),
    );

    let resp = hub
        .client
        .post(hub.url("/auth/refresh"))
        .header("x-requested-with", "XMLHttpRequest")
        .json(&serde_json::json!({ "credential": google }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let (value, _) = TestHub::set_auth_cookie(&resp).unwrap();
    let header = jsonwebtoken::decode_header(&value).unwrap();
    assert_eq!(
        header.kid.as_deref(),
        Some(SessionKeys::new(TEST_SESSION_SECRET).current().kid())
    );
}

#[tokio::test]
async fn emergency_rotation_rejects_old_cookies_immediately() {
    // The regular session hub has NO previous secret — exactly the
    // emergency-rotation keyring (and the post-overlap one): tokens
    // under any other secret fail closed, logged as kid mismatch.
    let (_provider, hub) = session_setup().await;
    let old_keys = SessionKeys::new(OLD_SESSION_SECRET);
    let old_token = mint_session(
        &old_keys,
        SessionLifetimes::default(),
        &identity("emergency-sub", "user@posit.co"),
        epoch_now(),
    )
    .unwrap();

    let resp = hub
        .get_health()
        .header("cookie", cookie_header(&old_token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    assert!(TestHub::set_auth_cookie(&resp).is_none());

    let events = snapshot_events();
    assert!(
        events.iter().any(|e| {
            e.fields.get("credential_kind").map(|s| s.as_str()) == Some("cookie")
                && e.fields
                    .get("detail")
                    .is_some_and(|d| d.contains("kid_mismatch"))
        }),
        "rejection must be observable as a kid mismatch, not a generic failure"
    );
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
