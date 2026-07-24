//! Shared integration-test fixtures: in-process tracing capture, a mock
//! OIDC provider serving discovery + JWKS on a random localhost port, a
//! JWT claims builder, and a configurable test hub.
//!
//! Extracted from `auth_bearer.rs` (device-flow Phase 2) so the
//! sliding-session tests (epic `bd-ey6jg70f`) can reuse the same
//! provider/hub scaffolding with a **known session secret**.

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

use quarto_hub::auth::{self, AuthConfig};
use quarto_hub::context::{HubConfig, HubContext, SharedContext};
use quarto_hub::server::build_router_with_state;
use quarto_hub::storage::StorageManager;

// ── tracing capture: in-process subscriber that retains every event ──

#[derive(Debug, Clone, Default)]
pub struct CapturedEvent {
    /// `tracing::Event::metadata().target()` of the captured event,
    /// retained so tests can filter on `"quarto_hub::audit"`.
    #[allow(dead_code)]
    pub target: String,
    pub fields: HashMap<String, String>,
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

pub fn snapshot_events() -> Vec<CapturedEvent> {
    captured_events().lock().clone()
}

pub fn install_tracing_once() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let _ = tracing_subscriber::registry().with(CaptureLayer).try_init();
    });
}

// ── mock OIDC provider ────────────────────────────────────────────

pub struct MockOidcProvider {
    pub issuer: String,
    pub jwks_url: String,
    encoding_key: EncodingKey,
    kid: String,
}

#[derive(Clone)]
struct MockOidcState {
    jwks_body: Arc<JsonValue>,
    discovery_body: Arc<JsonValue>,
}

impl MockOidcProvider {
    pub async fn start() -> Self {
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

    pub fn sign(&self, claims: &JsonValue) -> String {
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

pub const SPA_CLIENT_ID: &str = "spa.apps.googleusercontent.com";
pub const MCP_CLIENT_ID: &str = "mcp.apps.googleusercontent.com";

#[derive(Clone)]
pub struct ClaimsBuilder {
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
    pub fn from_provider(provider: &MockOidcProvider) -> Self {
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

    // Builder setter mirroring the JWT claim name; not std::ops::Sub.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(mut self, sub: impl Into<String>) -> Self {
        self.sub = sub.into();
        self
    }
    pub fn aud(mut self, aud: JsonValue) -> Self {
        self.aud = aud;
        self
    }
    pub fn azp(mut self, azp: impl Into<String>) -> Self {
        self.azp = Some(azp.into());
        self
    }
    pub fn no_azp(mut self) -> Self {
        self.azp = None;
        self
    }
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = email.into();
        self
    }
    pub fn email_verified(mut self, v: bool) -> Self {
        self.email_verified = v;
        self
    }
    pub fn iss(mut self, iss: impl Into<String>) -> Self {
        self.iss = iss.into();
        self
    }
    pub fn iat(mut self, iat: i64) -> Self {
        self.iat = Some(iat);
        self
    }
    pub fn exp(mut self, exp: i64) -> Self {
        self.exp = exp;
        self
    }
    pub fn to_value(&self) -> JsonValue {
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

/// Known session-signing secret for fixture hubs — lets tests mint
/// session tokens out-of-band via `quarto_hub::session` and have the
/// hub accept them.
pub const TEST_SESSION_SECRET: [u8; 32] = [0x42; 32];

pub struct TestHub {
    pub base_url: String,
    pub client: reqwest::Client,
}

/// Configurable fixture hub. Defaults: standalone mode, two allowlisted
/// audiences (SPA + MCP), no email/domain allowlist,
/// `allow_insecure_auth = true`, no pinned session secret.
pub struct TestHubBuilder {
    allowed_domains: Option<Vec<String>>,
    allow_insecure_auth: bool,
    session_secret: Option<[u8; 32]>,
    google_provider: bool,
}

impl Default for TestHubBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TestHubBuilder {
    pub fn new() -> Self {
        Self {
            allowed_domains: None,
            allow_insecure_auth: true,
            session_secret: None,
            google_provider: false,
        }
    }

    pub fn allowed_domains(mut self, domains: &[&str]) -> Self {
        self.allowed_domains = Some(domains.iter().map(|d| d.to_string()).collect());
        self
    }

    /// `allow_insecure_auth = false` (enables e.g. the WS-Origin check).
    pub fn secure(mut self) -> Self {
        self.allow_insecure_auth = false;
        self
    }

    /// Pin the hub's session-signing secret by pre-writing `hub.json`
    /// into the data dir before `StorageManager` initializes.
    pub fn session_secret(mut self, secret: [u8; 32]) -> Self {
        self.session_secret = Some(secret);
        self
    }

    /// Mark the provider as Google so provider-specific surface (the
    /// `POST /auth/callback` form-post route, double-submit CSRF) is
    /// registered. The issuer still points at the mock provider.
    pub fn google_provider(mut self) -> Self {
        self.google_provider = true;
        self
    }

    pub async fn start(self, provider: &MockOidcProvider) -> TestHub {
        // Auth config carries TWO audiences: SPA primary + MCP additional.
        // Construct directly (bypassing AuthConfig::new) so we can use the
        // mock provider's http:// issuer URL.
        let auth_config = AuthConfig {
            client_id: SPA_CLIENT_ID.to_string(),
            additional_audiences: vec![MCP_CLIENT_ID.to_string()],
            issuer: provider.issuer.clone(),
            image_domains: vec!["lh3.googleusercontent.com".to_string()],
            allowed_emails: None,
            allowed_domains: self.allowed_domains,
            provider: if self.google_provider {
                auth::OidcProvider::Google
            } else {
                auth::OidcProvider::Generic
            },
        };

        // Standalone HubContext so we don't need a project on disk.
        let temp = tempfile::TempDir::new().unwrap();

        // Pre-write hub.json with the pinned session secret; the
        // StorageManager loads (and preserves) it at init.
        if let Some(secret) = self.session_secret {
            let config = serde_json::json!({
                "version": 1,
                "created_at": "0",
                "session_secret": hex::encode(secret),
            });
            std::fs::write(temp.path().join("hub.json"), config.to_string()).unwrap();
        }

        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        // Keep the TempDir alive for the hub's lifetime by leaking it —
        // tests are short-lived; the TempDir would otherwise drop before
        // the hub finishes.
        Box::leak(Box::new(temp));

        let config = HubConfig {
            port: 0,
            host: "127.0.0.1".to_string(),
            sync_interval_secs: None,
            watch_enabled: false,
            auth_config: Some(auth_config),
            allow_insecure_auth: self.allow_insecure_auth,
            register_root_ws: false,
            ..HubConfig::default()
        };

        let ctx = HubContext::new(storage, config).await.unwrap();
        let ctx: SharedContext = Arc::new(ctx);

        // Inject the auth state ourselves, bypassing OIDC discovery so the
        // mock provider's http:// URLs are usable in tests.
        let audiences = vec![SPA_CLIENT_ID.to_string(), MCP_CLIENT_ID.to_string()];
        let auth_state = auth::build_auth_state_from_parts(
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

        TestHub { base_url, client }
    }
}

impl TestHub {
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// `GET /health` — `Authenticated` extractor required.
    pub fn get_health(&self) -> reqwest::RequestBuilder {
        self.client.get(self.url("/health"))
    }

    pub fn get_auth_me(&self) -> reqwest::RequestBuilder {
        self.client.get(self.url("/auth/me"))
    }

    pub fn post_auth_logout(&self) -> reqwest::RequestBuilder {
        self.client.post(self.url("/auth/logout"))
    }

    /// Extract the `quarto_hub_token` value from a response's
    /// `Set-Cookie` headers, plus the full attribute string.
    /// Returns `None` when no auth cookie was set.
    pub fn set_auth_cookie(resp: &reqwest::Response) -> Option<(String, String)> {
        resp.headers()
            .get_all(http::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|v| v.starts_with("quarto_hub_token="))
            .map(|v| {
                let value = v
                    .strip_prefix("quarto_hub_token=")
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
                    .to_string();
                (value, v.to_string())
            })
    }

    /// WS upgrade — we drive it through reqwest as a plain GET with the
    /// upgrade headers, since axum decides 101 vs error status from
    /// inside the handler before the actual ws library kicks in.
    pub fn ws_upgrade(&self) -> reqwest::RequestBuilder {
        self.client
            .get(self.url("/ws"))
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGVzdHNvY2tleS0xMjM0NTY3OA==")
    }
}
