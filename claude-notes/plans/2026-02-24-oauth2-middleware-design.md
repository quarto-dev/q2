# OAuth2 Middleware Design for quarto-hub

*2026-02-24*

Google OAuth2 authentication for quarto-hub, enforced at the middleware layer. The sync protocol (samod/automerge) is completely unaware of authentication.

## Design Principles

1. **Auth at the transport layer.** Unauthenticated requests are rejected before any sync protocol processing begins. samod is never modified.
2. **Stateless server.** No database, no server-issued tokens. Google ID tokens (JWTs) are validated locally using Google's cached public keys — no per-connection HTTP call to Google.
3. **Minimal moving parts.** Auth is a single module inside `quarto-hub`, using `axum-jwt-auth` for JWKS management and JWT validation. No separate auth crate. No upstream fork.
4. **Optional.** Auth is disabled by default. Enable with `--google-client-id <ID>`.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         quarto-hub Server (Axum)                        │
│                                                                         │
│  Incoming request                                                       │
│       │                                                                 │
│       ▼                                                                 │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                    Auth Layer (axum extractor)                    │   │
│  │                                                                  │   │
│  │  REST:      Authorization: Bearer <id_token> → authenticate() → 401│  │
│  │  WebSocket: ?id_token=<token> → authenticate() → 401             │  │
│  │  /health:   no auth required                                     │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│       │                                                                 │
│       ▼ (authenticated)                                                 │
│  ┌──────────────────┐    ┌──────────────────────────────────────────┐   │
│  │  REST handlers   │    │  samod (unmodified)                      │   │
│  │  /api/files      │    │  accept_axum(socket) → document sync     │   │
│  │  /api/documents  │    │  (no knowledge of auth)                  │   │
│  └──────────────────┘    └──────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
        │
        │  Token validation (local)
        │  JWT signature checked against Google's public keys
        │  Keys fetched once and cached (auto-refresh on rotation)
        │
        ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  Google JWKS endpoint: googleapis.com/oauth2/v3/certs                   │
│  (fetched once by axum-jwt-auth, cached, auto-refreshed hourly)         │
└─────────────────────────────────────────────────────────────────────────┘
```

### Why ID Tokens Instead of Access Tokens

| Aspect | Access tokens | ID tokens |
|--------|--------------|-----------|
| Format | Opaque string | JWT (signed by Google, RS256) |
| Validation | HTTP call to Google tokeninfo API per connection | Local signature check against cached public keys |
| Latency | 100-300ms per validation (network round-trip) | Microseconds (CPU only) |
| Resilience | Fails if Google API is unreachable | Works offline after initial key fetch |
| User info | Requires separate userinfo API call | Email, name, picture embedded in JWT claims |
| Lifetime | ~1 hour | ~1 hour |

### Token Transport

| Endpoint | Token location | Rationale |
|----------|---------------|-----------|
| REST (`/api/*`) | `Authorization: Bearer <id_token>` | Standard HTTP auth header; extracted and decoded via `HubContext::authenticate()` |
| WebSocket (`/ws`) | `?id_token=<token>` query param | Browsers can't set custom headers on WebSocket upgrade |
| Health (`/health`) | None | Always open for monitoring |

The ID token in the WebSocket URL is encrypted in transit by a TLS-terminating reverse proxy (`--behind-tls-proxy`). The `RedactedMakeSpan` trace layer ensures tokens are never logged server-side.

---

## Server-Side Implementation (Rust)

### Dependencies

Add to `crates/quarto-hub/Cargo.toml`:

```toml
[dependencies]
axum-jwt-auth = "0.6"
jsonwebtoken = "10"
```

`axum-jwt-auth` handles JWKS fetching, caching, auto-refresh, and JWT
validation. `jsonwebtoken` is a transitive dependency re-exported for
`Validation` configuration.

### Auth Module

All auth code lives in a single file: `crates/quarto-hub/src/auth.rs`.

```rust
// crates/quarto-hub/src/auth.rs

use axum::http::StatusCode;
use axum_jwt_auth::RemoteJwksDecoder;
use jsonwebtoken::{Algorithm, Validation};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

/// Authentication configuration.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub client_id: String,
    pub allowed_emails: Option<Vec<String>>,
    pub allowed_domains: Option<Vec<String>>,
}

/// Google ID token claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleClaims {
    pub sub: String,
    pub email: String,
    #[serde(default)]
    pub email_verified: bool,
    pub name: Option<String>,
    pub picture: Option<String>,
}

/// Check email/domain allowlists. Returns 401 for unverified emails,
/// 403 for verified emails that don't match any allowlist.
///
/// Logic: email must be verified. If no allowlists are configured, all
/// verified emails pass. If one or both allowlists are configured, the
/// user passes if they match ANY list (OR, not AND). This allows
/// combining `--allowed-domains=company.com` with
/// `--allowed-emails=contractor@gmail.com`.
pub fn check_allowlists(
    claims: &GoogleClaims,
    config: &AuthConfig,
) -> Result<(), StatusCode> {
    if !claims.email_verified {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let has_email_list = config.allowed_emails.is_some();
    let has_domain_list = config.allowed_domains.is_some();

    // No allowlists configured — all verified emails pass.
    if !has_email_list && !has_domain_list {
        return Ok(());
    }

    let email_ok = config.allowed_emails.as_ref()
        .is_some_and(|list| list.contains(&claims.email));

    let domain_ok = config.allowed_domains.as_ref()
        .is_some_and(|list| {
            let domain = claims.email.split('@').last().unwrap_or("");
            list.iter().any(|d| d == domain)
        });

    if email_ok || domain_ok {
        Ok(())
    } else {
        // 403, not 401: the user authenticated successfully but is
        // not permitted. Helps operators distinguish "bad credentials"
        // from "good credentials, wrong user" in server logs.
        Err(StatusCode::FORBIDDEN)
    }
}

/// Active auth state: decoder for JWT validation + background refresh task.
pub struct AuthState {
    pub decoder: RemoteJwksDecoder,
    /// Background task that periodically refreshes JWKS keys.
    /// Aborting this handle stops automatic key rotation.
    /// Must live as long as the server.
    _refresh_handle: JoinHandle<()>,
}

/// Build the JWKS decoder for Google ID token validation.
/// Returns an `AuthState` that owns both the decoder and the
/// background JWKS refresh task handle.
pub async fn build_auth_state(
    client_id: &str,
) -> Result<AuthState, Box<dyn std::error::Error>> {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id]);
    validation.set_issuer(&["https://accounts.google.com"]);

    let decoder = RemoteJwksDecoder::builder()
        .jwks_url("https://www.googleapis.com/oauth2/v3/certs")
        .validation(validation)
        .build()?;

    // Spawn the periodic JWKS key refresh as a background task.
    // RemoteJwksDecoder is Clone — the spawned copy shares the
    // internal key cache with our copy.
    let refresh_decoder = decoder.clone();
    let refresh_handle = tokio::spawn(async move {
        refresh_decoder.refresh_keys_periodically().await;
    });

    Ok(AuthState { decoder, _refresh_handle: refresh_handle })
}
```

### Integration with Axum Router

Both REST and WebSocket handlers use the same `HubContext::authenticate()`
helper, which decodes the JWT and checks allowlists. No `Claims<T>`
extractor needed — this avoids the problem where the extractor would fail
when auth is disabled (no decoder in state).

```rust
// crates/quarto-hub/src/server.rs

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    Router,
};
use serde::Deserialize;
use tower_http::trace::TraceLayer;
use crate::auth::GoogleClaims;

/// JSON error body for auth failures, so clients can distinguish
/// 401 auth errors from other HTTP errors programmatically.
fn unauthorized() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"})))
}

/// REST handler: extract Bearer token from Authorization header.
async fn list_files(
    headers: HeaderMap,
    State(ctx): State<SharedContext>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    ctx.authenticate(bearer_token(&headers))
        .await
        .map_err(|_| unauthorized())?;
    // ... handler logic
}

/// Extract Bearer token from Authorization header. Returns None if
/// no header is present or the header is not a valid Bearer token.
/// Never fails — the authenticate() method decides whether a missing
/// token is an error based on whether auth is enabled.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

#[derive(Deserialize)]
struct WsParams {
    id_token: Option<String>,
}

/// WebSocket: extract token from query param.
async fn ws_handler(
    State(ctx): State<SharedContext>,
    Query(params): Query<WsParams>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, StatusCode> {
    ctx.authenticate(params.id_token.as_deref()).await?;

    Ok(ws.on_upgrade(|socket| handle_websocket(socket, ctx)))
}

/// samod knows nothing about authentication.
async fn handle_websocket(socket: WebSocket, ctx: SharedContext) {
    let connection = match ctx.repo().accept_axum(socket) {
        Ok(conn) => conn,
        Err(samod::Stopped) => return,
    };

    let reason = connection.finished().await;
    tracing::info!(reason = ?reason, "WebSocket client disconnected");
}

/// Log request method and path only — never the query string, which
/// may contain id_token for WebSocket upgrades.
#[derive(Clone)]
struct RedactedMakeSpan;

impl<B> tower_http::trace::MakeSpan<B> for RedactedMakeSpan {
    fn make_span(&mut self, request: &http::Request<B>) -> tracing::Span {
        tracing::info_span!(
            "request",
            method = %request.method(),
            path = request.uri().path(),
        )
    }
}

/// Validate that TLS is accounted for when auth is enabled.
/// Called once at startup before the server accepts requests.
fn validate_tls_config(args: &HubArgs) {
    if args.google_client_id.is_some()
        && !args.behind_tls_proxy
        && !args.allow_insecure_auth
    {
        eprintln!(
            "error: --google-client-id requires TLS to protect tokens in transit.\n\
             Use --behind-tls-proxy if a reverse proxy terminates TLS,\n\
             or --allow-insecure-auth for local development (never in production)."
        );
        std::process::exit(1);
    }
    if args.allow_insecure_auth && args.google_client_id.is_some() {
        tracing::warn!(
            "Auth enabled WITHOUT TLS (--allow-insecure-auth). \
             Tokens will transit in plaintext. Do not use in production."
        );
    }
}

/// Build the router. Auth state (decoder + JWKS refresh handle) is
/// initialized here and owned by HubContext for the server's lifetime.
async fn build_router(ctx: SharedContext) -> Router {
    if let Some(config) = ctx.auth_config() {
        let auth_state = auth::build_auth_state(&config.client_id)
            .await
            .expect("Failed to initialize Google JWKS decoder");
        ctx.set_auth_state(auth_state);
    }

    let api_routes = Router::new()
        .route("/api/files", get(list_files))
        .route("/api/documents", get(list_documents));

    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .merge(api_routes)
        .layer(TraceLayer::new_for_http().make_span_with(RedactedMakeSpan))
        .with_state(ctx)
}
```

### HubConfig and HubContext Changes

```rust
// crates/quarto-hub/src/context.rs (additions)

use crate::auth::{AuthConfig, AuthState};
use axum_jwt_auth::JwtDecoder;
use std::sync::OnceLock;

pub struct HubConfig {
    // ... existing fields ...

    /// OAuth2 auth configuration. None = auth disabled.
    pub auth_config: Option<AuthConfig>,
}

pub struct HubContext {
    // ... existing fields ...

    /// Auth state: JWT decoder + JWKS refresh handle. Initialized once
    /// at server startup when auth is configured. Using OnceLock because
    /// it's set after construction but before the server accepts requests.
    auth_state: OnceLock<AuthState>,
}

impl HubContext {
    /// Store the auth state (decoder + refresh task handle).
    /// Called once during server startup in `build_router`.
    pub fn set_auth_state(&self, state: AuthState) {
        self.auth_state.set(state).expect("auth_state already initialized");
    }

    /// Authenticate a request. If auth is disabled, always succeeds.
    /// If auth is enabled, token must be present and valid.
    /// Used by both REST and WebSocket handlers.
    pub async fn authenticate(
        &self,
        token: Option<&str>,
    ) -> Result<(), StatusCode> {
        let Some(auth_config) = self.auth_config() else {
            return Ok(()); // Auth disabled — allow all.
        };

        let token = token.ok_or(StatusCode::UNAUTHORIZED)?;
        let auth_state = self.auth_state.get()
            .expect("auth_state is always present when auth is configured");

        // JwtDecoder<T>::decode returns TokenData<T>. The T parameter
        // lives on the trait, so we use a type annotation (not turbofish)
        // to select GoogleClaims.
        let token_data: jsonwebtoken::TokenData<GoogleClaims> = auth_state
            .decoder
            .decode(token)
            .await
            .map_err(|err| {
                tracing::warn!(%err, "Auth failed");
                StatusCode::UNAUTHORIZED
            })?;

        auth::check_allowlists(&token_data.claims, auth_config)?;
        tracing::info!(email = %token_data.claims.email, "Authenticated");
        Ok(())
    }
}
```

No `http_client` or `google_client` field needed — `axum-jwt-auth` manages
its own HTTP client and key cache internally. No separate `AppState` struct
needed — `SharedContext` remains the sole state type for all handlers.

---

## Client-Side Implementation

### Browser (hub-client) — TypeScript/React

#### Dependencies

```bash
npm install @react-oauth/google
```

No `jwt-decode` package needed — a JWT payload is decoded in three lines.

#### Auth Service

```typescript
// hub-client/src/services/authService.ts

import { googleLogout } from '@react-oauth/google';

export interface AuthState {
  idToken: string;
  email: string;
  name: string | null;
  picture: string | null;
  expiresAt: number;
}

const AUTH_STORAGE_KEY = 'quarto-hub-auth';

/** Decode JWT payload without verification (server validates). */
function decodeJwtPayload(jwt: string): Record<string, unknown> {
  const base64 = jwt.split('.')[1].replace(/-/g, '+').replace(/_/g, '/');
  return JSON.parse(atob(base64));
}

export function getStoredAuth(): AuthState | null {
  const stored = localStorage.getItem(AUTH_STORAGE_KEY);
  if (!stored) return null;

  try {
    const state: AuthState = JSON.parse(stored);
    if (Date.now() > state.expiresAt) {
      clearAuth();
      return null;
    }
    return state;
  } catch {
    return null;
  }
}

/** Store an ID token received from Google Sign-In. */
export function storeAuth(idToken: string): AuthState {
  const payload = decodeJwtPayload(idToken);

  const state: AuthState = {
    idToken,
    email: payload.email as string,
    name: (payload.name as string) ?? null,
    picture: (payload.picture as string) ?? null,
    expiresAt: (payload.exp as number) * 1000, // JWT exp is seconds
  };

  localStorage.setItem(AUTH_STORAGE_KEY, JSON.stringify(state));
  return state;
}

export function clearAuth(): void {
  localStorage.removeItem(AUTH_STORAGE_KEY);
  googleLogout();
}

export function getIdToken(): string | null {
  return getStoredAuth()?.idToken ?? null;
}
```

#### Auth Hook

Token expiry monitoring is built in — no separate hook needed.

```typescript
// hub-client/src/hooks/useAuth.ts

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  type AuthState,
  getStoredAuth,
  storeAuth,
  clearAuth,
} from '../services/authService';

export function useAuth() {
  const [auth, setAuth] = useState<AuthState | null>(getStoredAuth);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const expiryTimer = useRef<ReturnType<typeof setInterval>>(null);

  // Start expiry monitor on mount
  useEffect(() => {
    setIsLoading(false);

    expiryTimer.current = setInterval(() => {
      // getStoredAuth() returns null for expired tokens (and clears storage).
      // Sync React state if the stored auth has been cleared.
      if (!getStoredAuth()) setAuth(null);
    }, 60_000);

    return () => {
      if (expiryTimer.current) clearInterval(expiryTimer.current);
    };
  }, []);

  const handleCredentialResponse = useCallback((credential: string) => {
    try {
      setAuth(storeAuth(credential));
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Authentication failed');
    }
  }, []);

  const logout = useCallback(() => {
    clearAuth();
    setAuth(null);
  }, []);

  return { auth, isLoading, error, handleCredentialResponse, logout };
}
```

Consumers check `auth !== null` for authentication status and destructure
fields as needed (e.g., `auth.email`, `auth.picture`).

#### OAuth Provider Setup

`@react-oauth/google` requires a `GoogleOAuthProvider` ancestor in the React
tree. Wrap the app (or the auth-gated subtree) at the top level. The client
ID comes from a build-time environment variable.

```tsx
// hub-client/src/main.tsx (or App.tsx)

import { GoogleOAuthProvider } from '@react-oauth/google';

const GOOGLE_CLIENT_ID = import.meta.env.VITE_GOOGLE_CLIENT_ID;

function App() {
  return (
    <GoogleOAuthProvider clientId={GOOGLE_CLIENT_ID}>
      {/* ... rest of the app ... */}
    </GoogleOAuthProvider>
  );
}
```

When `VITE_GOOGLE_CLIENT_ID` is not set (local dev without auth), the
provider can be conditionally omitted or the login UI hidden.

#### Login Component

Google Identity Services' "Sign In With Google" button returns an ID token
directly — no separate userinfo API call needed.

```tsx
// hub-client/src/components/auth/LoginButton.tsx

import { GoogleLogin } from '@react-oauth/google';

export function LoginButton({
  onCredential,
}: {
  onCredential: (credential: string) => void;
}) {
  return (
    <GoogleLogin
      onSuccess={(response) => {
        if (response.credential) onCredential(response.credential);
      }}
      onError={() => console.error('Google login failed')}
    />
  );
}
```

The component is now a pure UI element. The parent calls `useAuth()` and
passes `handleCredentialResponse` as the `onCredential` prop.

#### WebSocket URL Construction

Append the ID token to the WebSocket URL before connecting.
The sync client and samod are completely unaware of auth.

```typescript
// hub-client/src/services/automergeSync.ts (modifications)

import { getIdToken } from './authService';

export async function connect(
  syncServerUrl: string,
  indexDocId: string,
): Promise<FileEntry[]> {
  await initWasm();
  vfsClear();

  // Append ID token to WebSocket URL if available
  const token = getIdToken();
  const url = token
    ? `${syncServerUrl}?id_token=${encodeURIComponent(token)}`
    : syncServerUrl;

  return ensureClient().connect(url, indexDocId);
}
```

No changes to `quarto-sync-client` are needed. The token is in the URL, which
the standard `BrowserWebSocketClientAdapter` passes through unchanged.

---

### CLI Client (Rust)

The CLI uses `yup-oauth2` for the installed application flow (opens browser,
receives callback). By requesting `openid` scopes, the token response includes
an `id_token` field which is what the server validates.

#### Dependencies

Add to `crates/quarto/Cargo.toml`:

```toml
[dependencies]
yup-oauth2 = "11"
dirs = "6"
```

#### CLI Auth Module

```rust
// crates/quarto/src/auth.rs

use anyhow::{Context, Result};
use std::path::PathBuf;
use yup_oauth2::{InstalledFlowAuthenticator, InstalledFlowReturnMethod};

/// Request openid scopes so the token response includes an id_token.
const SCOPES: &[&str] = &[
    "openid",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
];

fn token_cache_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("quarto")
        .join("oauth2_tokens.json")
}

fn client_secret_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("quarto")
        .join("client_secret.json")
}

/// Get a Google ID token for hub authentication.
/// Opens browser on first use, uses cached/refreshed tokens subsequently.
pub async fn get_id_token() -> Result<String> {
    let secret_path = client_secret_path();
    if !secret_path.exists() {
        anyhow::bail!(
            "OAuth2 client secret not found at: {}\n\
             Download client_secret.json from Google Cloud Console.",
            secret_path.display()
        );
    }

    let secret = yup_oauth2::read_application_secret(&secret_path)
        .await
        .context("Failed to read client secret")?;

    let cache = token_cache_path();
    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let auth = InstalledFlowAuthenticator::builder(
        secret,
        InstalledFlowReturnMethod::HTTPRedirect,
    )
    .persist_tokens_to_disk(&cache)
    .build()
    .await
    .context("Failed to create authenticator")?;

    // id_token() is a method on Authenticator (not on Token).
    // It returns Result<Option<String>, Error>.
    // Requires "openid" in SCOPES for Google to include the ID token.
    auth.id_token(SCOPES)
        .await
        .context("Failed to get ID token")?
        .ok_or_else(|| anyhow::anyhow!(
            "No ID token in response. Ensure 'openid' scope is granted."
        ))
}

pub fn clear_tokens() -> Result<()> {
    let path = token_cache_path();
    if path.exists() { std::fs::remove_file(&path)?; }
    Ok(())
}

pub fn has_cached_tokens() -> bool {
    token_cache_path().exists()
}
```

#### CLI Commands and Hub Server Flags

```rust
// crates/quarto/src/commands/auth.rs

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Authenticate with Google for hub access.
    Login,
    /// Clear cached tokens.
    Logout,
    /// Show authentication status.
    Status,
}
```

```rust
// crates/quarto/src/commands/hub.rs (additions)

#[derive(Parser)]
pub struct HubArgs {
    // ... existing fields ...

    /// Google OAuth2 client ID. Presence enables auth.
    /// Requires --behind-tls-proxy (or --allow-insecure-auth for local dev).
    #[arg(long)]
    pub google_client_id: Option<String>,

    /// Acknowledge that a TLS-terminating reverse proxy (nginx, Caddy,
    /// cloud LB) sits in front of the hub. Required when auth is enabled.
    #[arg(long)]
    pub behind_tls_proxy: bool,

    /// Allow auth without TLS (local development only). Tokens will
    /// transit in plaintext — never use this in production.
    #[arg(long)]
    pub allow_insecure_auth: bool,

    /// Allowed email addresses (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub allowed_emails: Option<Vec<String>>,

    /// Allowed email domains (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub allowed_domains: Option<Vec<String>>,
}
```

#### CLI Client Connection

```rust
// crates/quarto/src/commands/hub.rs (client connection)

pub async fn connect_to_hub(url: &str, require_auth: bool) -> Result<()> {
    let ws_url = if require_auth {
        let token = crate::auth::get_id_token().await?;
        format!("{}?id_token={}", url, urlencoding::encode(&token))
    } else {
        url.to_string()
    };

    // Connect to hub with ws_url — samod sees a normal WebSocket
    // ...

    Ok(())
}
```

---

## Configuration

### Environment Variables

```bash
# Browser client (build-time)
VITE_GOOGLE_CLIENT_ID=your-id.apps.googleusercontent.com

# Server (runtime, via CLI flags or env)
QUARTO_HUB_GOOGLE_CLIENT_ID=your-id.apps.googleusercontent.com
QUARTO_HUB_ALLOWED_DOMAINS=mycompany.com,partner.org
QUARTO_HUB_ALLOWED_EMAILS=admin@example.com
```

### Google Cloud Console Setup

1. Go to https://console.cloud.google.com/ and create a project (or select an existing one).

2. Navigate to **APIs & Services > OAuth consent screen**:
   - Choose "External" user type (or "Internal" for Google Workspace orgs)
   - Fill in app name and support email
   - Add scopes: `openid`, `email`, `profile`
   - Add test users if the app is in "Testing" publish status

3. Navigate to **APIs & Services > Credentials > Create Credentials > OAuth client ID**.
   Create **two** credentials:

   **Web application** (for hub-client browser + server validation):
   - Authorized JavaScript origins: `http://localhost:5173` (dev), plus your production URL
   - Copy the **client ID** — this is `VITE_GOOGLE_CLIENT_ID` and `--google-client-id`
   - The client ID looks like `123456789-abcdef.apps.googleusercontent.com`

   **Desktop application** (for CLI `q2 auth login`):
   - Download the JSON credentials file
   - Save as `~/.config/quarto/client_secret.json`

Both the server `--google-client-id` flag and the browser `VITE_GOOGLE_CLIENT_ID` use the **web application** client ID. The server validates that the JWT `aud` claim matches this ID. The CLI uses the desktop credential to obtain tokens through the browser redirect flow.

### Usage

**Server** (local dev without TLS):
```bash
q2 hub --google-client-id YOUR_ID.apps.googleusercontent.com \
       --allow-insecure-auth
```

**Server** (production behind reverse proxy):
```bash
q2 hub --google-client-id YOUR_ID.apps.googleusercontent.com \
       --behind-tls-proxy \
       --allowed-domains mycompany.com \
       --allowed-emails contractor@gmail.com
```

**Browser client:**
```bash
VITE_GOOGLE_CLIENT_ID=YOUR_ID.apps.googleusercontent.com npm run dev
```

When `VITE_GOOGLE_CLIENT_ID` is not set, auth is completely disabled — no login screen, no token on WebSocket URLs.

**CLI client:**
```bash
q2 auth login    # Opens browser, gets Google ID token
q2 auth status   # Shows token cache and client secret paths
q2 auth logout   # Clears cached tokens
```

---

## Security Considerations

1. **TLS required.** `--google-client-id` requires either `--behind-tls-proxy` (production: reverse proxy terminates TLS) or `--allow-insecure-auth` (local dev only, logged as a warning). The server itself stays HTTP-only; TLS is handled by the proxy layer.
2. **Local validation.** ID tokens are validated by checking the JWT signature against Google's cached public keys. No outbound network call per connection.
3. **Token in URL (WebSocket).** Encrypted by TLS in transit. `RedactedMakeSpan` ensures the `TraceLayer` logs only `uri.path()`, never the query string containing the token.
4. **Short-lived tokens.** Google ID tokens expire in ~1 hour. Limits exposure window.
5. **Audience check.** The `jsonwebtoken::Validation` config verifies the `aud` claim matches the configured client ID, preventing tokens issued for other applications from being accepted.
6. **Domain/email allowlists.** Defense in depth beyond Google authentication.
7. **Minimal client errors.** Invalid/missing tokens return 401; allowlist rejections return 403. Neither includes user-identifying detail. Specific reasons logged server-side only.
8. **localStorage tokens (browser).** Accessible to XSS. Acceptable for v1; mitigate with Content-Security-Policy headers.

---

## Known Limitations

1. **No silent token refresh.** Google Identity Services' Sign In button does not provide refresh tokens. When the ID token expires (~1hr), the user must re-authenticate. The auth hook detects this proactively.

2. **Token in WebSocket URL.** Could appear in server access logs. Mitigated by TLS and log configuration. A future iteration could add a short-lived ticket exchange endpoint (`POST /auth/ticket` → one-time ticket for WebSocket URL).

3. **No user database.** Cannot track users, audit access history, or implement per-user settings. Add if/when needed.

4. **CLI ID token availability.** `yup-oauth2`'s `Authenticator::id_token()` method returns the ID token when the `openid` scope is requested. The ID token is stored alongside the access token in the token cache, so refreshed tokens also include it. However, the `id_token()` method is separate from `token()` (which only returns the access token).

---

## Implementation Progress

### Phase 1: Server Auth Module (Rust — `crates/quarto-hub`)

- [x] Add `axum-jwt-auth` and `jsonwebtoken` dependencies to Cargo.toml
- [x] Create `src/auth.rs`: `AuthConfig`, `GoogleClaims`, `AuthState`, `check_allowlists()`, `build_auth_state()`
- [x] Add `auth_config: Option<AuthConfig>` to `HubConfig`, `OnceLock<AuthState>` to `HubContext`
- [x] Add `HubContext::authenticate()` and `HubContext::auth_config()` methods
- [x] Add `HubContext::set_auth_state()` method
- [x] Update `server.rs`: `build_router` becomes async, initializes auth state
- [x] REST handlers: extract Bearer token from header, call `ctx.authenticate()`
- [x] WebSocket handler: extract `id_token` from query param, call `ctx.authenticate()`
- [x] Add `RedactedMakeSpan` to prevent token logging
- [x] Add `validate_tls_config()` check at startup
- [x] Add `unauthorized()` JSON error helper
- [x] Update `run_server()` to accept auth config and call validation
- [x] Add unit tests for `check_allowlists()` (9 tests)

### Phase 2: CLI Flags (Rust — `crates/quarto` + `crates/quarto-hub`)

- [x] Add `--google-client-id`, `--behind-tls-proxy`, `--allow-insecure-auth` flags to hub binary
- [x] Add `--allowed-emails`, `--allowed-domains` flags to hub binary
- [x] Add same flags to `quarto hub` subcommand in CLI
- [x] Wire flags through to `HubConfig` → `AuthConfig`

### Phase 3: Browser Client (TypeScript — `hub-client`)

- [x] Install `@react-oauth/google`
- [x] Add `VITE_GOOGLE_CLIENT_ID` env var type definition
- [x] Create `src/services/authService.ts` (store/get/clear auth, JWT decode)
- [x] Create `src/hooks/useAuth.ts` (auth state, expiry monitoring)
- [x] Create `src/components/auth/LoginButton.tsx`
- [x] Wrap app in `GoogleOAuthProvider` (conditional on env var)
- [x] Add auth gate to `App.tsx`
- [x] Append ID token to WebSocket URL in `automergeSync.ts` connect()

### Phase 4: CLI Client Auth (Rust — `crates/quarto`)

- [x] Add `yup-oauth2` and `dirs` dependencies
- [x] Create `crates/quarto/src/auth.rs` (get_id_token, clear_tokens, status)
- [x] Add `quarto auth login/logout/status` subcommands
- [ ] Append ID token to WebSocket URL when connecting as client (deferred: no client connect command exists yet)
