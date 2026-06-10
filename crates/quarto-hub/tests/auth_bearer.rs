//! Phase 2 — Hub middleware: Bearer extraction + audience allowlist
//! + dual-credential 400 + CSRF/Origin gating by credential kind.
//!
//! These tests spin up:
//!   * a `MockOidcProvider` (axum server on a random localhost port)
//!     serving `/.well-known/openid-configuration` + a JWKS endpoint;
//!   * a `TestHub` (axum server on a random localhost port) configured
//!     with two allowlisted audiences — the SPA's `client_id` and the
//!     hub-mcp `additional_audiences` entry — plus an issuer pointing
//!     at the mock OIDC provider.
//!
//! JWTs are minted in-process with a RS256 keypair held by the
//! provider. The corresponding public JWK is served at the JWKS URL,
//! and `RemoteJwksDecoder` fetches it once at hub startup. The hub
//! itself is built via `build_router_with_state` with auth-state
//! injected through a new `build_auth_state_from_parts` constructor
//! that skips OIDC discovery (the discovery path requires HTTPS).
//!
//! Plan: claude-notes/plans/2026-05-05-hub-mcp-device-flow-implementation.md
//! §Phase 2.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::{Json, Router, extract::State, response::IntoResponse, routing::get};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use parking_lot::Mutex;
use rsa::{
    RsaPrivateKey, RsaPublicKey, pkcs1::EncodeRsaPrivateKey, pkcs8::LineEnding,
    traits::PublicKeyParts,
};
use serde_json::{Value as JsonValue, json};
use tokio::net::TcpListener;

use quarto_hub::auth::{self, AuthConfig, AuthState};
use quarto_hub::context::{HubConfig, HubContext, SharedContext};
use quarto_hub::server::build_router_with_state;
use quarto_hub::storage::StorageManager;

// ── tracing capture: in-process subscriber that retains every event ──

#[derive(Debug, Clone, Default)]
struct CapturedEvent {
    /// `tracing::Event::metadata().target()` of the captured event,
    /// retained so future tests can filter on `"quarto_hub::audit"`.
    #[allow(dead_code)]
    target: String,
    fields: HashMap<String, String>,
}

struct CaptureLayer;

impl<S> tracing_subscriber::layer::Layer<S> for CaptureLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut fields = HashMap::new();
        let mut visitor = FieldVisitor(&mut fields);
        event.record(&mut visitor);
        let captured = CapturedEvent {
            target: event.metadata().target().to_string(),
            fields,
        };
        captured_events().lock().push(captured);
    }
}

struct FieldVisitor<'a>(&'a mut HashMap<String, String>);
impl tracing::field::Visit for FieldVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_string(), format!("{value:?}"));
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
}

fn captured_events() -> &'static Mutex<Vec<CapturedEvent>> {
    static EVENTS: OnceLock<Mutex<Vec<CapturedEvent>>> = OnceLock::new();
    EVENTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn snapshot_events() -> Vec<CapturedEvent> {
    captured_events().lock().clone()
}

fn install_tracing_once() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let _ = tracing_subscriber::registry().with(CaptureLayer).try_init();
    });
}

// ── mock OIDC provider ────────────────────────────────────────────

struct MockOidcProvider {
    issuer: String,
    jwks_url: String,
    encoding_key: EncodingKey,
    kid: String,
}

#[derive(Clone)]
struct MockOidcState {
    jwks_body: Arc<JsonValue>,
    discovery_body: Arc<JsonValue>,
}

impl MockOidcProvider {
    async fn start() -> Self {
        // RSA-2048 keypair, one-shot per process via the global init below.
        let (private_pem, jwk, kid) = build_test_keypair();

        let encoding_key =
            EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("valid PEM for jsonwebtoken");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let issuer = format!("http://{addr}");
        let jwks_url = format!("{issuer}/.well-known/jwks.json");

        let state = MockOidcState {
            jwks_body: Arc::new(json!({ "keys": [jwk] })),
            discovery_body: Arc::new(json!({
                "issuer": issuer.clone(),
                "jwks_uri": jwks_url.clone(),
            })),
        };

        let app = Router::new()
            .route("/.well-known/openid-configuration", get(serve_discovery))
            .route("/.well-known/jwks.json", get(serve_jwks))
            .with_state(state);

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            issuer,
            jwks_url,
            encoding_key,
            kid,
        }
    }

    fn header(&self) -> Header {
        let mut h = Header::new(Algorithm::RS256);
        h.kid = Some(self.kid.clone());
        h
    }

    fn sign(&self, claims: &JsonValue) -> String {
        jsonwebtoken::encode(&self.header(), claims, &self.encoding_key).expect("sign JWT")
    }
}

async fn serve_discovery(State(s): State<MockOidcState>) -> impl IntoResponse {
    Json((*s.discovery_body).clone())
}

async fn serve_jwks(State(s): State<MockOidcState>) -> impl IntoResponse {
    Json((*s.jwks_body).clone())
}

fn build_test_keypair() -> (String, JsonValue, String) {
    static KEYPAIR: OnceLock<(String, JsonValue, String)> = OnceLock::new();
    KEYPAIR
        .get_or_init(|| {
            // OsRng from rsa's re-export — matches the rand_core 0.6 version
            // the rsa crate compiles against (workspace's `rand = 0.9` uses
            // a newer, incompatible rand_core).
            let mut rng = rsa::rand_core::OsRng;
            let private = RsaPrivateKey::new(&mut rng, 2048).expect("RSA-2048 generation");
            let public = RsaPublicKey::from(&private);

            let pem = private
                .to_pkcs1_pem(LineEnding::LF)
                .expect("PKCS#1 PEM encode")
                .to_string();

            let n_bytes = public.n().to_bytes_be();
            let e_bytes = public.e().to_bytes_be();
            let kid = "test-kid-1".to_string();
            let jwk = json!({
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": kid,
                "n": URL_SAFE_NO_PAD.encode(&n_bytes),
                "e": URL_SAFE_NO_PAD.encode(&e_bytes),
            });

            (pem, jwk, kid)
        })
        .clone()
}

// ── JWT claim helpers ─────────────────────────────────────────────

const SPA_CLIENT_ID: &str = "spa.apps.googleusercontent.com";
const MCP_CLIENT_ID: &str = "mcp.apps.googleusercontent.com";

#[derive(Clone)]
struct ClaimsBuilder {
    iss: String,
    sub: String,
    aud: JsonValue,
    azp: Option<String>,
    email: String,
    email_verified: bool,
    name: Option<String>,
    picture: Option<String>,
    iat: Option<i64>,
    nbf: Option<i64>,
    exp: i64,
}

impl ClaimsBuilder {
    fn from_provider(provider: &MockOidcProvider) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            iss: provider.issuer.clone(),
            sub: "bearer-test-sub".to_string(),
            aud: JsonValue::String(SPA_CLIENT_ID.to_string()),
            azp: None,
            email: "user@posit.co".to_string(),
            email_verified: true,
            name: Some("Test User".to_string()),
            picture: None,
            iat: Some(now - 5),
            nbf: None,
            exp: now + 600,
        }
    }

    fn sub(mut self, sub: impl Into<String>) -> Self {
        self.sub = sub.into();
        self
    }
    fn aud(mut self, aud: JsonValue) -> Self {
        self.aud = aud;
        self
    }
    fn azp(mut self, azp: impl Into<String>) -> Self {
        self.azp = Some(azp.into());
        self
    }
    fn no_azp(mut self) -> Self {
        self.azp = None;
        self
    }
    fn email(mut self, email: impl Into<String>) -> Self {
        self.email = email.into();
        self
    }
    fn email_verified(mut self, v: bool) -> Self {
        self.email_verified = v;
        self
    }
    fn iss(mut self, iss: impl Into<String>) -> Self {
        self.iss = iss.into();
        self
    }
    fn iat(mut self, iat: i64) -> Self {
        self.iat = Some(iat);
        self
    }
    fn exp(mut self, exp: i64) -> Self {
        self.exp = exp;
        self
    }
    fn to_value(&self) -> JsonValue {
        let mut v = json!({
            "iss": self.iss,
            "sub": self.sub,
            "aud": self.aud,
            "email": self.email,
            "email_verified": self.email_verified,
            "exp": self.exp,
        });
        let m = v.as_object_mut().unwrap();
        if let Some(azp) = &self.azp {
            m.insert("azp".into(), JsonValue::String(azp.clone()));
        }
        if let Some(name) = &self.name {
            m.insert("name".into(), JsonValue::String(name.clone()));
        }
        if let Some(picture) = &self.picture {
            m.insert("picture".into(), JsonValue::String(picture.clone()));
        }
        if let Some(iat) = self.iat {
            m.insert("iat".into(), JsonValue::Number(iat.into()));
        }
        if let Some(nbf) = self.nbf {
            m.insert("nbf".into(), JsonValue::Number(nbf.into()));
        }
        v
    }
}

// ── test hub ──────────────────────────────────────────────────────

struct TestHub {
    base_url: String,
    client: reqwest::Client,
}

impl TestHub {
    async fn start(provider: &MockOidcProvider) -> Self {
        // Auth config carries TWO audiences: SPA primary + MCP additional.
        // Construct directly (bypassing AuthConfig::new) so we can use the
        // mock provider's http:// issuer URL.
        let auth_config = AuthConfig {
            client_id: SPA_CLIENT_ID.to_string(),
            additional_audiences: vec![MCP_CLIENT_ID.to_string()],
            issuer: provider.issuer.clone(),
            image_domains: vec!["lh3.googleusercontent.com".to_string()],
            allowed_emails: None,
            allowed_domains: None,
            provider: auth::OidcProvider::Generic,
        };

        // Standalone HubContext so we don't need a project on disk.
        let temp = tempfile::TempDir::new().unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        // Keep the TempDir alive for the hub's lifetime by leaking it —
        // tests are short-lived; the TempDir would otherwise drop before
        // the hub finishes.
        Box::leak(Box::new(temp));

        let config = HubConfig {
            port: 0,
            host: "127.0.0.1".to_string(),
            peers: Vec::new(),
            sync_interval_secs: None,
            watch_enabled: false,
            watch_debounce_ms: 500,
            watch_filter: Default::default(),
            single_file: None,
            resource_files: Vec::new(),
            auth_config: Some(auth_config),
            allow_insecure_auth: true,
            register_root_ws: false,
        };

        let ctx = HubContext::new(storage, config).await.unwrap();
        let ctx: SharedContext = Arc::new(ctx);

        // Inject the auth state ourselves, bypassing OIDC discovery so the
        // mock provider's http:// URLs are usable in tests.
        let audiences = vec![SPA_CLIENT_ID.to_string(), MCP_CLIENT_ID.to_string()];
        let auth_state: AuthState = auth::build_auth_state_from_parts(
            provider.jwks_url.clone(),
            vec![Algorithm::RS256],
            audiences,
            provider.issuer.clone(),
        )
        .await
        .expect("build auth state");
        ctx.set_auth_state(auth_state).expect("set auth state");

        let router = build_router_with_state(ctx.clone()).await.unwrap();
        let router = router.with_state(ctx);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");

        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        Self { base_url, client }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// `GET /health` — `Authenticated` extractor required.
    fn get_health(&self) -> reqwest::RequestBuilder {
        self.client.get(self.url("/health"))
    }

    fn get_auth_me(&self) -> reqwest::RequestBuilder {
        self.client.get(self.url("/auth/me"))
    }

    fn post_auth_logout(&self) -> reqwest::RequestBuilder {
        self.client.post(self.url("/auth/logout"))
    }

    /// WS upgrade — we drive it through reqwest as a plain GET with the
    /// upgrade headers, since axum decides 101 vs error status from
    /// inside the handler before the actual ws library kicks in.
    fn ws_upgrade(&self) -> reqwest::RequestBuilder {
        self.client
            .get(self.url("/ws"))
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGVzdHNvY2tleS0xMjM0NTY3OA==")
    }
}

// ── shared test fixture ───────────────────────────────────────────

async fn shared_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;
            let hub = TestHub::start(&provider).await;
            (provider, hub)
        })
        .await
}

async fn allowlist_setup() -> &'static (MockOidcProvider, TestHub) {
    // Separate hub with allowed_domains = ["posit.co"]; lets us assert
    // 403 vs 401 distinction on the allowlist path.
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;

            let auth_config = AuthConfig {
                client_id: SPA_CLIENT_ID.to_string(),
                additional_audiences: vec![MCP_CLIENT_ID.to_string()],
                issuer: provider.issuer.clone(),
                image_domains: vec!["lh3.googleusercontent.com".to_string()],
                allowed_emails: None,
                allowed_domains: Some(vec!["posit.co".to_string()]),
                provider: auth::OidcProvider::Generic,
            };

            let temp = tempfile::TempDir::new().unwrap();
            let storage = StorageManager::new_standalone(temp.path()).unwrap();
            Box::leak(Box::new(temp));

            let config = HubConfig {
                port: 0,
                host: "127.0.0.1".to_string(),
                peers: Vec::new(),
                sync_interval_secs: None,
                watch_enabled: false,
                watch_debounce_ms: 500,
                watch_filter: Default::default(),
                single_file: None,
                resource_files: Vec::new(),
                auth_config: Some(auth_config),
                allow_insecure_auth: true,
                register_root_ws: false,
            };

            let ctx = HubContext::new(storage, config).await.unwrap();
            let ctx: SharedContext = Arc::new(ctx);

            let audiences = vec![SPA_CLIENT_ID.to_string(), MCP_CLIENT_ID.to_string()];
            let auth_state = auth::build_auth_state_from_parts(
                provider.jwks_url.clone(),
                vec![Algorithm::RS256],
                audiences,
                provider.issuer.clone(),
            )
            .await
            .unwrap();
            ctx.set_auth_state(auth_state).unwrap();

            let router = build_router_with_state(ctx.clone()).await.unwrap();
            let router = router.with_state(ctx);

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let base_url = format!("http://{addr}");

            tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });

            let hub = TestHub {
                base_url,
                client: reqwest::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap(),
            };

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
    let (provider, hub) = shared_setup().await;
    // Hub started with allow_insecure_auth=true skips the Origin check in
    // the existing code path. To exercise the regression we need a hub
    // with secure cookies; we build a one-off here.
    let _ = (provider, hub);
    let (provider2, hub2) = secure_setup().await;
    let token = provider2.sign(
        &ClaimsBuilder::from_provider(provider2)
            .sub("ws-cookie-bad-origin")
            .to_value(),
    );
    let resp = hub2
        .ws_upgrade()
        .header("origin", "https://attacker.example.com")
        .header("cookie", format!("quarto_hub_token={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "cookie-authenticated WS upgrade with bad Origin must 403"
    );
}

// secure (allow_insecure_auth=false) hub for the WS-Origin regression
async fn secure_setup() -> &'static (MockOidcProvider, TestHub) {
    static SETUP: tokio::sync::OnceCell<(MockOidcProvider, TestHub)> =
        tokio::sync::OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            install_tracing_once();
            let provider = MockOidcProvider::start().await;

            let auth_config = AuthConfig {
                client_id: SPA_CLIENT_ID.to_string(),
                additional_audiences: vec![MCP_CLIENT_ID.to_string()],
                issuer: provider.issuer.clone(),
                image_domains: vec!["lh3.googleusercontent.com".to_string()],
                allowed_emails: None,
                allowed_domains: None,
                provider: auth::OidcProvider::Generic,
            };

            let temp = tempfile::TempDir::new().unwrap();
            let storage = StorageManager::new_standalone(temp.path()).unwrap();
            Box::leak(Box::new(temp));

            let config = HubConfig {
                port: 0,
                host: "127.0.0.1".to_string(),
                peers: Vec::new(),
                sync_interval_secs: None,
                watch_enabled: false,
                watch_debounce_ms: 500,
                watch_filter: Default::default(),
                single_file: None,
                resource_files: Vec::new(),
                auth_config: Some(auth_config),
                allow_insecure_auth: false, // <-- the key difference
                register_root_ws: false,
            };

            let ctx = HubContext::new(storage, config).await.unwrap();
            let ctx: SharedContext = Arc::new(ctx);

            let audiences = vec![SPA_CLIENT_ID.to_string(), MCP_CLIENT_ID.to_string()];
            let auth_state = auth::build_auth_state_from_parts(
                provider.jwks_url.clone(),
                vec![Algorithm::RS256],
                audiences,
                provider.issuer.clone(),
            )
            .await
            .unwrap();
            ctx.set_auth_state(auth_state).unwrap();

            let router = build_router_with_state(ctx.clone()).await.unwrap();
            let router = router.with_state(ctx);

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let base_url = format!("http://{addr}");

            tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });

            let hub = TestHub {
                base_url,
                client: reqwest::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap(),
            };
            (provider, hub)
        })
        .await
}

// ── Cookie still works (regression) ──────────────────────────────

#[tokio::test]
async fn cookie_still_authenticates() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("cookie-regression")
            .to_value(),
    );

    let resp = hub
        .get_auth_me()
        .header("cookie", format!("quarto_hub_token={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

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
                && e.fields.get("detail").is_some()
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

#[tokio::test]
async fn mutating_endpoint_with_cookie_still_requires_csrf() {
    let (provider, hub) = shared_setup().await;
    let token = provider.sign(
        &ClaimsBuilder::from_provider(provider)
            .sub("csrf-cookie-required")
            .to_value(),
    );

    let resp = hub
        .post_auth_logout()
        .header("cookie", format!("quarto_hub_token={token}"))
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
