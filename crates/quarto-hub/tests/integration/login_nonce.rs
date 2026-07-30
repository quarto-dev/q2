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

/// As [`secure_google_setup`], plus a domain allowlist — the standard
/// admission gate, and the only shape that can produce the 403 from
/// `authenticate_claims`.
async fn allowlisted_google_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .secure()
                .google_provider()
                .allowed_domains(&["posit.co"])
                .session_secret(TEST_SESSION_SECRET)
                .start(&provider)
                .await;
            (provider, hub)
        })
        .await
}

/// As [`secure_google_setup`], with one `sub` banned — the other cause
/// that qualifies as a genuine denial.
async fn banned_google_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHubBuilder::new()
                .secure()
                .google_provider()
                .banned_subs(&["reason-banned-sub"])
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

/// Assert a failed callback: a redirect to `/?auth_error=<reason>`, no
/// session minted, and the sealed login-state cookie cleared.
///
/// `reason` is the coarse, user-facing class — deliberately many-to-one
/// over the dozen causes, because it lands in a URL the user sees. The
/// precise cause is asserted separately, against the audit log.
fn assert_auth_error(resp: &reqwest::Response, reason: &str) {
    assert!(
        resp.status().is_redirection(),
        "expected a redirect, got {}",
        resp.status()
    );
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("a Location header");
    assert_eq!(
        location,
        format!("/?auth_error={reason}"),
        "the redirect must name which kind of failure this was"
    );
    assert!(
        TestHub::find_set_cookie(resp, AUTH_COOKIE_NAME_SECURE).is_none(),
        "no session may be minted"
    );
    // Single use, on **every** exit path — one pre-flight can complete
    // at most one login, and adding reasons must not open a hole in that.
    let (value, attrs) = TestHub::find_set_cookie(resp, LOGIN_STATE_COOKIE_SECURE)
        .expect("the login-state cookie must be cleared on every failure path");
    assert_eq!(value, "", "cleared, not rewritten: {attrs}");
    assert!(attrs.contains("Max-Age=0"), "{attrs}");
}

/// The `detail` of the sole `auth_fail` audit event carrying `sub`.
///
/// Asserted **exactly**, not by substring: the whole point of the
/// discriminator is that an operator can tell one failure class from
/// another, and a substring match would accept a doubled prefix.
fn auth_fail_detail_for(sub: &str) -> String {
    let details: Vec<String> = snapshot_events()
        .iter()
        .filter(|e| {
            e.fields.get("action").map(String::as_str) == Some("auth_fail")
                && e.fields.get("sub").map(String::as_str) == Some(sub)
        })
        .filter_map(|e| e.fields.get("detail").cloned())
        .collect();
    assert_eq!(
        details.len(),
        1,
        "expected exactly one auth_fail for sub={sub}, got {details:?}"
    );
    details.into_iter().next().unwrap()
}

/// `detail`s of every `auth_fail` audit event carrying **no** `sub` —
/// the pre-identity failures, where there is no validated subject to
/// report.
fn subless_auth_fail_details() -> Vec<String> {
    snapshot_events()
        .iter()
        .filter(|e| {
            e.fields.get("action").map(String::as_str) == Some("auth_fail")
                && !e.fields.contains_key("sub")
        })
        .filter_map(|e| e.fields.get("detail").cloned())
        .collect()
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
    assert_auth_error(&resp, "restart");
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
    assert_auth_error(&resp, "restart");
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
    assert_auth_error(&resp, "restart");
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
    assert_auth_error(&resp, "restart");
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
    assert_auth_error(&resp, "restart");
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
    assert_auth_error(&resp, "restart");
}

// ── which cookie-absent reading? (E0) ─────────────────────────────

/// A cookie-absent callback has two readings, and they want opposite
/// remedies — reload the app, or fix cookie delivery. The token's own
/// `nonce` claim is what tells them apart, so the check must look at it
/// rather than returning a single blanket class.
///
/// Nonce-less: no current client can produce this. `GoogleAuthProvider`
/// renders nothing until it holds a nonce, so the token came from a
/// stale bundle or from something driving GIS outside the app.
#[tokio::test]
async fn a_nonceless_token_without_a_cookie_audits_as_stale_client() {
    let (provider, hub) = secure_google_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-stale-client-sub")
            .to_value(),
    );

    let resp = post_callback(hub, &google, None).await;
    assert_auth_error(&resp, "stale_client");
    assert_eq!(
        auth_fail_detail_for("nonce-stale-client-sub"),
        "login_state_stale_client"
    );
}

/// Nonce-bearing: a login attempt that really did do the pre-flight but
/// arrived without its cookie (`SameSite` / `Path` / proxy — fix the
/// configuration), *or* a captured token replayed from a browser that
/// never did one. Indistinguishable per event; correlation tells them
/// apart.
#[tokio::test]
async fn a_nonce_bearing_token_without_a_cookie_audits_as_login_state_missing() {
    let (provider, hub) = secure_google_setup().await;
    let (nonce, _blob) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-cookie-lost-sub")
            .nonce(&nonce)
            .to_value(),
    );

    let resp = post_callback(hub, &google, None).await;
    assert_auth_error(&resp, "restart");
    // Exactly this, undoubled: the emit site adds the `login_state_`
    // prefix, so the returned class must not carry one of its own.
    assert_eq!(
        auth_fail_detail_for("nonce-cookie-lost-sub"),
        "login_state_missing"
    );
}

// ── the callback's other failure paths, in the log (E2) ───────────

/// CSRF was the callback's one genuinely silent rejection: a deployment
/// whose reverse proxy drops the `g_csrf_token` cookie failed every
/// login with nothing whatsoever in `journalctl -u hub`.
#[tokio::test]
async fn a_bad_csrf_pair_is_audited_as_callback_csrf() {
    let (provider, hub) = secure_google_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-csrf-sub")
            .to_value(),
    );

    // The double-submit pair disagrees — cookie says one thing, form
    // field another.
    let resp = no_redirect_client()
        .post(hub.url("/auth/callback"))
        .header("cookie", "g_csrf_token=from-the-cookie")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("credential={google}&g_csrf_token=from-the-form"))
        .send()
        .await
        .unwrap();
    assert_auth_error(&resp, "restart");

    let events = snapshot_events();
    let mut matching = events
        .iter()
        .filter(|e| e.fields.get("detail").map(String::as_str) == Some("callback_csrf"));
    let event = matching.next().expect("a callback_csrf audit event");
    assert!(matching.next().is_none(), "exactly one callback_csrf event");

    assert_eq!(event.target, "quarto_hub::audit");
    assert_eq!(
        event.fields.get("action").map(String::as_str),
        Some("auth_fail")
    );
    assert_eq!(
        event.fields.get("outcome").map(String::as_str),
        Some("deny")
    );
    assert_eq!(
        event.fields.get("credential_kind").map(String::as_str),
        Some("cookie")
    );
    // The check runs before the token is parsed, so no `sub` has been
    // validated. An unvalidated one in the audit log is worse than none.
    assert!(
        !event.fields.contains_key("sub"),
        "no sub is available here: {:?}",
        event.fields
    );
}

/// The credential paths already carry details finer than a blanket
/// `credential_invalid`, but nothing pinned them **from the callback**.
/// These two tests are what stops a later blanket emit from burying the
/// specific discriminator an operator needs.
#[tokio::test]
async fn a_non_allowlisted_credential_is_audited_as_user_not_allowlisted() {
    let (provider, hub) = allowlisted_google_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("nonce-allowlist-sub")
            .email("outsider@example.com")
            .to_value(),
    );

    // `authenticate_claims` runs before the nonce check, so this never
    // reaches the login-state cookie — hence no `login_state_` prefix.
    let resp = post_callback(hub, &google, None).await;
    assert_auth_error(&resp, "denied");
    assert_eq!(
        auth_fail_detail_for("nonce-allowlist-sub"),
        "user_not_allowlisted"
    );
}

#[tokio::test]
async fn an_undecodable_credential_is_audited_as_a_jwt_decode_failure() {
    let (_provider, hub) = secure_google_setup().await;

    let resp = post_callback(hub, "not-a-jwt", None).await;
    assert_auth_error(&resp, "restart");

    let details = subless_auth_fail_details();
    assert_eq!(
        details.len(),
        1,
        "one event, not a specific one plus a blanket one: {details:?}"
    );
    assert!(
        details[0].starts_with("jwt_decode:"),
        "expected a jwt_decode: detail, got {details:?}"
    );
}

// ── the 403/401 split (E1) ────────────────────────────────────────

/// `denied` means an identity was established and then refused, and the
/// allowlist miss is one of exactly two causes that qualify: signature,
/// `aud`, `azp` and `iat` all passed and the email is verified, so this
/// user was refused on policy and will be refused again. Mapping it to
/// `restart` would put a permanently-refused user in a retry loop.
///
/// This is the mapping whose failure mode is both silent and
/// user-visible, which is why it gets a test of its own: the callback
/// used to discard the status that draws the line.
#[tokio::test]
async fn an_allowlist_miss_redirects_with_reason_denied() {
    let (provider, hub) = allowlisted_google_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("reason-denied-sub")
            .email("stranger@example.com")
            .to_value(),
    );

    let resp = post_callback(hub, &google, None).await;
    assert_auth_error(&resp, "denied");
}

/// The 401 family is `restart`, never `denied` — no identity was
/// established, so "your account is not authorized" would simply be
/// false. A wrong `aud` is the case that matters most: on client-ID
/// drift (the hub's configured audience diverging from the SPA's) every
/// user in the deployment fails here at once, and `denied` would tell
/// all of them their account was refused — indistinguishable from a mass
/// de-allowlisting.
#[tokio::test]
async fn a_wrong_audience_credential_redirects_with_reason_restart() {
    let (provider, hub) = secure_google_setup().await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("reason-restart-sub")
            .aud(serde_json::json!(
                "some-other-client.apps.googleusercontent.com"
            ))
            .to_value(),
    );

    let resp = post_callback(hub, &google, None).await;
    assert_auth_error(&resp, "restart");
}

/// The other cause that qualifies as `denied`. It is gated *after* the
/// nonce check, so reaching it takes an otherwise flawless nonce-bound
/// login — which is the point of testing it: a ban must refuse a login
/// that is perfect in every other respect.
///
/// The pre-existing ban coverage (`session_auth.rs::ban_gates_verify_and_mint`)
/// goes through `/auth/session`, which answers 403 rather than
/// redirecting, so it says nothing about the reason a browser is shown.
#[tokio::test]
async fn a_banned_user_redirects_with_reason_denied() {
    let (provider, hub) = banned_google_setup().await;
    let (nonce, blob) = fetch_nonce(hub, LOGIN_STATE_COOKIE_SECURE).await;
    let google = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("reason-banned-sub")
            .nonce(&nonce)
            .to_value(),
    );

    let resp = post_callback(hub, &google, Some((LOGIN_STATE_COOKIE_SECURE, &blob))).await;
    assert_auth_error(&resp, "denied");
    assert_eq!(auth_fail_detail_for("reason-banned-sub"), "user_banned");
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
    assert_auth_error(&resp, "restart");
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
