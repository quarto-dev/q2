//! Server-verified `nonce` in the GIS login (H2, `bd-uqjiac5a`).
//!
//! Without a nonce the hub validates an ID token's signature, `iss`,
//! `aud`, and `exp` — but has no way to tell whether *this* token belongs
//! to *this* login attempt. Any captured Google ID token could therefore
//! be replayed to a mint endpoint for its full (~1 h) validity. These
//! tests pin the binding: `GET /auth/nonce` seals a nonce into a cookie,
//! and `POST /auth/callback` requires the ID token to echo it.
//!
//! **Scope.** Enforcement covers the Google form-post callback only —
//! `/auth/session` (the Generic provider's JSON mint) stays replay-able
//! within the submitted token's validity. That is an accepted boundary,
//! not an oversight; see the plan.
//!
//! Every enforcement test runs on a **secure** hub:
//! `--allow-insecure-auth` deliberately skips the check, because the
//! `SameSite=None; Secure` cookie the flow needs cannot work over plain
//! HTTP. `insecure_mode_skips_enforcement_with_a_warning` covers that.

use quarto_hub::login_state::{LOGIN_STATE_TTL_SECS, seal_login_state};
use quarto_hub::session::{SessionIdentity, SessionKeys, SessionLifetimes, mint_session};

use crate::support::{
    AUTH_COOKIE_NAME_SECURE, ClaimsBuilder, LOGIN_STATE_COOKIE_LEGACY, LOGIN_STATE_COOKIE_SECURE,
    MockOidcProvider, TEST_SESSION_SECRET, TestHub, TestHubBuilder, install_tracing_once,
    snapshot_events,
};

// ── fixtures ──────────────────────────────────────────────────────

/// Secure Google-provider hub with the known session secret — the shape
/// that enforces the nonce.
async fn secure_google_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .secure()
                .google_provider()
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

fn no_redirect_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

/// Fetch `/auth/nonce`, returning `(nonce, sealed_blob)`.
async fn fetch_nonce(hub: &TestHub, cookie_name: &str) -> (String, String) {
    let resp = hub.client.get(hub.url("/auth/nonce")).send().await.unwrap();
    assert_eq!(resp.status(), 200, "nonce pre-flight must succeed");
    let (blob, _) =
        TestHub::find_set_cookie(&resp, cookie_name).expect("sealed login-state cookie set");
    let body: serde_json::Value = resp.json().await.unwrap();
    let nonce = body["nonce"].as_str().expect("nonce in body").to_string();
    (nonce, blob)
}

/// Drive `POST /auth/callback` with the Google double-submit CSRF pair,
/// optionally attaching a sealed login-state cookie.
async fn post_callback(
    hub: &TestHub,
    credential: &str,
    login_cookie: Option<(&str, &str)>,
) -> reqwest::Response {
    let mut cookies = vec!["g_csrf_token=csrf-h2".to_string()];
    if let Some((name, value)) = login_cookie {
        cookies.push(format!("{name}={value}"));
    }
    no_redirect_client()
        .post(hub.url("/auth/callback"))
        .header("cookie", cookies.join("; "))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("credential={credential}&g_csrf_token=csrf-h2"))
        .send()
        .await
        .unwrap()
}

fn assert_auth_error(resp: &reqwest::Response) {
    assert!(
        resp.status().is_redirection(),
        "expected a redirect, got {}",
        resp.status()
    );
    assert_eq!(
        resp.headers().get("location").unwrap(),
        "/?auth_error",
        "failed nonce verification must land on the auth-error route"
    );
    assert!(
        TestHub::find_set_cookie(resp, AUTH_COOKIE_NAME_SECURE).is_none(),
        "no session may be minted"
    );
}

// ── the pre-flight endpoint ───────────────────────────────────────

#[tokio::test]
async fn nonce_endpoint_returns_a_nonce_and_a_sealed_cookie() {
    let (_provider, hub) = secure_google_setup().await;
    let resp = hub.client.get(hub.url("/auth/nonce")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let (blob, attrs) = TestHub::find_set_cookie(&resp, LOGIN_STATE_COOKIE_SECURE)
        .expect("__Secure- prefixed login-state cookie");

    // `SameSite=None` is load-bearing, not laxity: Google delivers the
    // credential by *cross-site* form POST, and a Lax cookie is not
    // attached to that.
    assert!(attrs.contains("SameSite=None"), "{attrs}");
    assert!(attrs.contains("Secure"), "__Secure- requires it: {attrs}");
    assert!(attrs.contains("HttpOnly"), "{attrs}");
    // Scoped to the only two routes that use it, hence `__Secure-`
    // rather than `__Host-` (which would force Path=/).
    assert!(attrs.contains("Path=/auth"), "{attrs}");
    assert!(
        attrs.contains(&format!("Max-Age={LOGIN_STATE_TTL_SECS}")),
        "{attrs}"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    let nonce = body["nonce"].as_str().expect("nonce in body");
    assert_eq!(nonce.len(), 64, "256 bits, hex-encoded");

    // The cookie must seal the nonce that was handed to the client —
    // otherwise the callback could never match them.
    let opened =
        quarto_hub::login_state::open_login_state(&test_keys(), &blob, epoch_now()).unwrap();
    assert_eq!(opened.nonce, nonce);
}

#[tokio::test]
async fn nonce_endpoint_issues_a_distinct_nonce_per_call() {
    let (_provider, hub) = secure_google_setup().await;
    let (a, blob_a) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;
    let (b, blob_b) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;
    assert_ne!(a, b, "a reused nonce would not bind a single attempt");
    assert_ne!(blob_a, blob_b);
}

// ── happy path ────────────────────────────────────────────────────

#[tokio::test]
async fn callback_with_matching_nonce_mints_a_session() {
    let (provider, hub) = secure_google_setup().await;
    let (nonce, blob) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;

    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-happy-sub")
            .email("nonce@posit.co")
            .nonce(&nonce)
            .to_value(),
    );
    let resp = post_callback(hub, &google, Some((LOGIN_STATE_COOKIE_SECURE, &blob))).await;

    assert!(resp.status().is_redirection());
    assert_eq!(resp.headers().get("location").unwrap(), "/");
    let (token, _) = TestHub::find_set_cookie(&resp, AUTH_COOKIE_NAME_SECURE)
        .expect("session cookie minted on a nonce-bound login");
    let verified = quarto_hub::session::verify_session(
        &test_keys(),
        SessionLifetimes::default(),
        &token,
        epoch_now(),
    )
    .unwrap();
    assert_eq!(verified.claims.sub, "nonce-happy-sub");
}

#[tokio::test]
async fn callback_clears_the_login_cookie_after_use() {
    let (provider, hub) = secure_google_setup().await;
    let (nonce, blob) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-singleuse-sub")
            .nonce(&nonce)
            .to_value(),
    );

    let resp = post_callback(hub, &google, Some((LOGIN_STATE_COOKIE_SECURE, &blob))).await;
    assert!(resp.status().is_redirection());

    // Single use: the blob is cleared so the same login attempt cannot
    // be completed twice from the same jar.
    let (value, attrs) = TestHub::find_set_cookie(&resp, LOGIN_STATE_COOKIE_SECURE)
        .expect("login-state cookie cleared on success");
    assert_eq!(value, "");
    assert!(attrs.contains("Max-Age=0"), "{attrs}");
}

// ── the replay window this closes ─────────────────────────────────

#[tokio::test]
async fn callback_without_a_login_cookie_is_rejected() {
    // This is the captured-token replay case: a valid, unexpired Google
    // ID token, correct CSRF pair, no login attempt behind it.
    let (provider, hub) = secure_google_setup().await;
    let (nonce, _blob) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-nocookie-sub")
            .nonce(&nonce)
            .to_value(),
    );

    let resp = post_callback(hub, &google, None).await;
    assert_auth_error(&resp);
}

#[tokio::test]
async fn callback_with_a_nonce_from_another_login_is_rejected() {
    // Two concurrent pre-flights; the token from one presented with the
    // cookie from the other.
    let (provider, hub) = secure_google_setup().await;
    let (nonce_a, _blob_a) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;
    let (_nonce_b, blob_b) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;

    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-crossed-sub")
            .nonce(&nonce_a)
            .to_value(),
    );
    let resp = post_callback(hub, &google, Some((LOGIN_STATE_COOKIE_SECURE, &blob_b))).await;
    assert_auth_error(&resp);
}

#[tokio::test]
async fn callback_with_a_token_carrying_no_nonce_is_rejected() {
    // A pre-H2 client, or a token minted for some other relying party.
    let (provider, hub) = secure_google_setup().await;
    let (_nonce, blob) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-missing-claim-sub")
            .to_value(),
    );

    let resp = post_callback(hub, &google, Some((LOGIN_STATE_COOKIE_SECURE, &blob))).await;
    assert_auth_error(&resp);
}

#[tokio::test]
async fn callback_with_an_expired_login_blob_is_rejected() {
    let (provider, hub) = secure_google_setup().await;
    let nonce = "a".repeat(64);
    // Sealed far enough in the past that even the leeway is exhausted.
    let stale = seal_login_state(
        &test_keys(),
        &nonce,
        epoch_now() - LOGIN_STATE_TTL_SECS - 3600,
        LOGIN_STATE_TTL_SECS,
    )
    .unwrap();

    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-expired-sub")
            .nonce(&nonce)
            .to_value(),
    );
    let resp = post_callback(hub, &google, Some((LOGIN_STATE_COOKIE_SECURE, &stale))).await;
    assert_auth_error(&resp);
}

#[tokio::test]
async fn callback_with_a_tampered_login_blob_is_rejected() {
    let (provider, hub) = secure_google_setup().await;
    let (nonce, blob) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;

    // Flip a byte in the signature: the payload still says the right
    // nonce, so only the HMAC stands between this and acceptance.
    let mut parts: Vec<String> = blob.split('.').map(String::from).collect();
    let mut sig = parts[2].clone().into_bytes();
    let i = sig.len() / 2;
    sig[i] = if sig[i] == b'A' { b'B' } else { b'A' };
    parts[2] = String::from_utf8(sig).unwrap();

    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-tampered-sub")
            .nonce(&nonce)
            .to_value(),
    );
    let resp = post_callback(
        hub,
        &google,
        Some((LOGIN_STATE_COOKIE_SECURE, &parts.join("."))),
    )
    .await;
    assert_auth_error(&resp);
}

/// A forged blob claiming an attacker-chosen nonce, signed with the
/// wrong secret — the case that would break the whole scheme if the
/// signature were not checked.
#[tokio::test]
async fn callback_with_a_blob_sealed_under_a_foreign_secret_is_rejected() {
    let (provider, hub) = secure_google_setup().await;
    let nonce = "b".repeat(64);
    let forged = seal_login_state(
        &SessionKeys::new([0x99; 32]),
        &nonce,
        epoch_now(),
        LOGIN_STATE_TTL_SECS,
    )
    .unwrap();

    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-forged-sub")
            .nonce(&nonce)
            .to_value(),
    );
    let resp = post_callback(hub, &google, Some((LOGIN_STATE_COOKIE_SECURE, &forged))).await;
    assert_auth_error(&resp);
}

// ── domain separation across the HTTP surface ─────────────────────

/// The unit tests prove the two token types cannot open as each other;
/// this proves the *wiring* honours that — a sealed login blob handed to
/// the hub as a session cookie must not authenticate.
#[tokio::test]
async fn a_sealed_login_blob_is_not_a_session_cookie() {
    let (_provider, hub) = secure_google_setup().await;
    let (_nonce, blob) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;

    let resp = hub
        .get_health()
        .header("cookie", format!("{AUTH_COOKIE_NAME_SECURE}={blob}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "a login blob must not authenticate");
}

/// And the reverse: a real session token presented as the login-state
/// cookie must not satisfy the nonce check.
#[tokio::test]
async fn a_session_token_is_not_a_login_state_cookie() {
    let (provider, hub) = secure_google_setup().await;
    let session = mint_session(
        &test_keys(),
        SessionLifetimes::default(),
        &SessionIdentity {
            sub: "nonce-crosstype-sub".into(),
            email: "user@posit.co".into(),
            email_verified: true,
            name: None,
            picture: None,
        },
        epoch_now(),
    )
    .unwrap();

    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-crosstype-sub")
            .nonce("whatever")
            .to_value(),
    );
    let resp = post_callback(hub, &google, Some((LOGIN_STATE_COOKIE_SECURE, &session))).await;
    assert_auth_error(&resp);
}

// ── insecure mode ─────────────────────────────────────────────────

/// `--allow-insecure-auth` has no TLS, and the flow's
/// `SameSite=None; Secure` cookie cannot function over plain HTTP. The
/// callback therefore skips the check and says so — consistent with that
/// flag's existing "never in production" contract.
#[tokio::test]
async fn insecure_mode_skips_enforcement_with_a_warning() {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    let (provider, hub) = SETUP
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
        .await;

    // No nonce claim, no login cookie — would be rejected in secure mode.
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-insecure-sub")
            .to_value(),
    );
    let resp = post_callback(hub, &google, None).await;
    assert!(resp.status().is_redirection());
    assert_eq!(
        resp.headers().get("location").unwrap(),
        "/",
        "insecure mode still logs in"
    );

    let events = snapshot_events();
    assert!(
        events.iter().any(|e| {
            e.fields
                .get("message")
                .is_some_and(|m| m.contains("nonce verification skipped"))
        }),
        "the skip must be loud in the log"
    );

    // The pre-flight still works there, using the unprefixed cookie name
    // (`__Secure-` requires TLS), so the client flow is identical.
    let (_nonce, _blob) = fetch_nonce(hub, LOGIN_STATE_COOKIE_LEGACY).await;
}
