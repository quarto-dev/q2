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
│  │  /health:   authenticated (same as REST)                         │   │
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
| REST (`/api/*`, `/health`) | `Authorization: Bearer <id_token>` | Standard HTTP auth header; extracted and decoded via `HubContext::authenticate()` |
| WebSocket (`/ws`) | `?id_token=<token>` query param | Browsers can't set custom headers on WebSocket upgrade |

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
async fn build_router(ctx: SharedContext) -> Result<Router> {
    if let Some(config) = ctx.auth_config() {
        let auth_state = auth::build_auth_state(&config.client_id)
            .await
            .map_err(|e| Error::Server(format!(
                "Failed to initialize Google JWKS decoder: {e}"
            )))?;
        ctx.set_auth_state(auth_state);
    }

    let api_routes = Router::new()
        .route("/api/files", get(list_files))
        .route("/api/documents", get(list_documents));

    Ok(Router::new()
        .route("/health", get(health))
        .route("/auth/callback", post(auth_callback))
        .route("/ws", get(ws_handler))
        .merge(api_routes)
        .layer(TraceLayer::new_for_http().make_span_with(RedactedMakeSpan))
        .with_state(ctx))
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

Handles credential ingestion from the OAuth redirect, token expiry, and
silent refresh via Google One Tap.

```typescript
// hub-client/src/hooks/useAuth.ts

import { useCallback, useEffect, useRef, useState } from 'react';
import { useGoogleOneTapLogin } from '@react-oauth/google';
import {
  type AuthState,
  getStoredAuth,
  storeAuth,
  clearAuth,
} from '../services/authService';

const REFRESH_BUFFER_MS = 5 * 60 * 1000; // 5 minutes before expiry

export function useAuth() {
  const [auth, setAuth] = useState<AuthState | null>(() => {
    // Check URL search params first (OAuth redirect callback), then localStorage.
    const params = new URLSearchParams(window.location.search);
    const credential = params.get('auth_credential');
    if (credential) {
      try {
        const authState = storeAuth(credential);
        const url = new URL(window.location.href);
        url.searchParams.delete('auth_credential');
        window.history.replaceState(null, '', url.pathname + url.search + url.hash);
        return authState;
      } catch { /* fall through */ }
    }
    return getStoredAuth();
  });

  const [refreshEnabled, setRefreshEnabled] = useState(false);
  const refreshTimer = useRef<ReturnType<typeof setTimeout>>(null);
  const expiryTimer = useRef<ReturnType<typeof setTimeout>>(null);

  // Silent refresh via Google One Tap. Enabled ~5 min before expiry.
  useGoogleOneTapLogin({
    onSuccess: (response) => {
      if (response.credential) {
        try { setAuth(storeAuth(response.credential)); } catch { /* noop */ }
      }
      setRefreshEnabled(false);
    },
    onError: () => setRefreshEnabled(false),
    auto_select: true,
    disabled: !refreshEnabled,
  });

  // Schedule silent refresh and hard expiry.
  useEffect(() => {
    if (refreshTimer.current) clearTimeout(refreshTimer.current);
    if (expiryTimer.current) clearTimeout(expiryTimer.current);
    if (!auth) return;

    const msUntilExpiry = auth.expiresAt - Date.now();
    if (msUntilExpiry <= 0) { clearAuth(); setAuth(null); return; }

    const msUntilRefresh = msUntilExpiry - REFRESH_BUFFER_MS;
    if (msUntilRefresh > 0) {
      refreshTimer.current = setTimeout(() => setRefreshEnabled(true), msUntilRefresh);
    }

    expiryTimer.current = setTimeout(() => { clearAuth(); setAuth(null); }, msUntilExpiry);

    return () => {
      if (refreshTimer.current) clearTimeout(refreshTimer.current);
      if (expiryTimer.current) clearTimeout(expiryTimer.current);
    };
  }, [auth]);

  const logout = useCallback(() => { clearAuth(); setAuth(null); }, []);

  return { auth, logout };
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

Google Identity Services' "Sign In With Google" button in redirect mode
(`ux_mode="redirect"`) keeps the login flow in the same browser window
instead of opening a popup. The credential is returned via a server-side
callback.

```tsx
// hub-client/src/components/auth/LoginButton.tsx

import { GoogleLogin } from '@react-oauth/google';

export function LoginButton() {
  return (
    <GoogleLogin
      ux_mode="redirect"
      login_uri={window.location.origin + '/auth/callback'}
      onSuccess={() => {}}
      onError={() => console.error('Google login failed')}
    />
  );
}
```

**Redirect flow:**
1. User clicks button → browser navigates to Google (same tab)
2. After auth → Google POSTs credential JWT to `/auth/callback`
3. Server-side handler (Vite middleware in dev, hub server in production)
   extracts credential, validates CSRF, redirects to `/?auth_credential=<jwt>`
4. `useAuth()` picks up credential from URL search params on mount

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

   **Web application** (for hub-client browser + server validation):
   - Authorized JavaScript origins: `http://localhost:5173` (dev), plus your production URL
   - Copy the **client ID** — this is `VITE_GOOGLE_CLIENT_ID` and `--google-client-id`
   - The client ID looks like `123456789-abcdef.apps.googleusercontent.com`

Both the server `--google-client-id` flag and the browser `VITE_GOOGLE_CLIENT_ID` use this client ID. The server validates that the JWT `aud` claim matches this ID.

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

---

## Security Review

*Reviewed 2026-02-25.*

### Hardening measures in place

1. **TLS required.** `--google-client-id` requires either `--behind-tls-proxy` (production: reverse proxy terminates TLS) or `--allow-insecure-auth` (local dev only, logged as a warning). The server itself stays HTTP-only; TLS is handled by the proxy layer.
2. **Stateless local validation.** ID tokens are validated by checking the JWT signature against Google's cached JWKS public keys. No outbound network call per connection. Keys auto-rotate via a background refresh task with cancellation token support for clean shutdown.
3. **Audience + issuer verification.** `jsonwebtoken::Validation` verifies the `aud` claim matches the configured client ID and that `iss` is `https://accounts.google.com`, preventing tokens issued for other applications from being accepted.
4. **Email verification check.** Unverified Google emails are rejected before allowlist checks.
5. **Domain/email allowlists.** Defense in depth beyond Google authentication. OR logic allows combining `--allowed-domains` with `--allowed-emails` for flexibility.
6. **CSRF validation on OAuth callback.** Both the Vite dev middleware and production `auth_callback` handler validate that the `g_csrf_token` cookie matches the form POST value before issuing a redirect.
7. **JWT validation on OAuth callback.** The production `auth_callback` handler validates the JWT (via `ctx.authenticate()`) before redirecting to the SPA. This prevents the redirect from injecting arbitrary data into the `?auth_credential=` URL parameter. (Defense-in-depth: subsequent WebSocket/REST calls validate again.)
8. **Log redaction.** `RedactedMakeSpan` ensures the `TraceLayer` logs only `uri.path()`, never the query string (which may contain `id_token` for WebSocket upgrades).
9. **Token in URL (WebSocket).** Encrypted by TLS in transit. Redacted from server logs (see above).
10. **Short-lived tokens.** Google ID tokens expire in ~1 hour. The browser client schedules an exact `setTimeout` based on the token's `exp` claim to clear auth state precisely at expiry, with no polling gap.
11. **Minimal client errors.** Invalid/missing tokens return 401; allowlist rejections return 403. Neither includes user-identifying detail. Specific reasons logged server-side only.
12. **Credential in redirect is URL-safe by construction.** JWTs are base64url-encoded segments separated by `.` — all unreserved URI characters per RFC 3986. Both `auth_callback` handlers document this invariant explicitly.
13. **Case-insensitive Bearer matching.** The `bearer_token()` extractor matches the `Authorization` header scheme case-insensitively per RFC 7235 §2.1.
14. **JWT structure validation (browser).** `decodeJwtPayload()` verifies the token has exactly 3 dot-separated segments before attempting base64 decode, preventing cryptic errors from malformed input.
15. **Graceful JWKS initialization failure.** `build_router` propagates JWKS decoder initialization errors via `Result` rather than panicking, so operators get a clean error message if Google's JWKS endpoint is unreachable at startup.
18. **Silent token refresh.** ~5 minutes before the ID token expires, the `useAuth` hook enables Google One Tap with `auto_select` via `useGoogleOneTapLogin` from `@react-oauth/google`. If the user has an active Google session (and the browser supports FedCM or third-party cookies), a fresh credential is returned silently — no UI, no redirect. If silent refresh fails, the hard expiry timer clears auth and the user sees the login screen. This keeps collaborative editing sessions alive across token boundaries in most environments.

### Deployment recommendations

- **Content-Security-Policy.** The reverse proxy that terminates TLS should set a `Content-Security-Policy` header on HTML responses to mitigate XSS (which could steal localStorage auth tokens). A reasonable baseline: `default-src 'self'; script-src 'self' https://accounts.google.com; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com https://accounts.google.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' data: https://lh3.googleusercontent.com; connect-src 'self' ws: wss: https://accounts.google.com; frame-src https://accounts.google.com`. This is a deployment concern (not application code) because different deployments have different CSP requirements depending on CDN origins, proxy setups, etc.
- **Reverse proxy query-string logging.** Configure the reverse proxy to not log query strings, which may contain `id_token` values on WebSocket upgrade requests.

### Residual risks (accepted)

1. **localStorage tokens (browser).** Accessible to XSS. Mitigated by short token lifetime (~1 hour), server-side validation, and the CSP deployment recommendation above.
2. **Token in WebSocket URL.** Could appear in browser dev tools or reverse proxy logs. Mitigated by TLS, server-side log redaction, and the proxy logging recommendation above. A future iteration could add a short-lived ticket exchange endpoint (`POST /auth/ticket` → one-time ticket for WebSocket URL).
3. **Credential in redirect URL.** The JWT appears briefly in the browser URL bar during the OAuth callback redirect. The `useAuth` hook clears it via `replaceState` on mount, but it may appear in browser history for a brief window. (Mitigated: the production `auth_callback` handler now validates the JWT before redirecting, so only valid Google-issued tokens reach the URL.)
4. **WebSocket validated once at upgrade.** After the initial `authenticate()` call, the WebSocket connection lives until the client disconnects. If a user is removed from the allowlist or their token expires, already-established connections are not terminated. Clients naturally reconnect (and re-authenticate) when the frontend detects token expiry.

---

## Known Limitations

1. **No user database.** Cannot track users, audit access history, or implement per-user settings. Add if/when needed.

2. **Silent refresh browser support.** The silent token refresh (hardening measure 18) depends on the browser supporting FedCM or third-party cookies. Browsers with strict tracking protection (e.g. Safari, Firefox with ETP) may block One Tap, in which case the user must manually re-authenticate when the token expires (~1 hour). This is a graceful degradation, not a failure.

---

## Implementation Progress

### Phase 1: Server Auth Module (Rust — `crates/quarto-hub`)

- [x] Add `axum-jwt-auth` and `jsonwebtoken` dependencies to Cargo.toml
- [x] Create `src/auth.rs`: `AuthConfig`, `GoogleClaims`, `AuthState`, `check_allowlists()`, `build_auth_state()`
- [x] Add `auth_config: Option<AuthConfig>` to `HubConfig`, `OnceLock<AuthState>` to `HubContext`
- [x] Add `HubContext::authenticate()` and `HubContext::auth_config()` methods
- [x] Add `HubContext::set_auth_state()` method
- [x] Update `server.rs`: `build_router` becomes async, initializes auth state, returns `Result<Router>`
- [x] REST handlers: extract Bearer token from header, call `ctx.authenticate()`
- [x] WebSocket handler: extract `id_token` from query param, call `ctx.authenticate()`; document single-validation-at-upgrade security property
- [x] `auth_callback`: validate JWT server-side before redirecting (defense-in-depth)
- [x] `build_router`: propagate JWKS initialization errors via `Result` (no panic)
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
- [x] Create `src/hooks/useAuth.ts` (auth state, expiry monitoring, silent refresh via `useGoogleOneTapLogin`)
- [x] Create `src/components/auth/LoginButton.tsx`
- [x] Wrap app in `GoogleOAuthProvider` (conditional on env var)
- [x] Add auth gate to `App.tsx`
- [x] Append ID token to WebSocket URL in `automergeSync.ts` connect()

### Phase 4: CLI Client Auth (Rust — `crates/quarto`) — REMOVED

CLI auth module (`auth.rs`, `auth_cmd.rs`, `yup-oauth2`, `dirs`) was removed as dead code — nothing consumed the tokens. The hub server only receives tokens from the browser-based Google Sign-In flow. CLI-to-hub auth can be re-implemented if/when a CLI client connect command is added.
