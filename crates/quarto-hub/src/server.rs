//! HTTP server setup and routing

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{
        Form, FromRef, FromRequestParts, Path, State,
        ws::{WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, request::Parts},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use cookie::SameSite;
use samod::DocumentId;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, info};

use crate::auth;
use crate::context::{HubConfig, HubContext, SharedContext};
use crate::error::Result;
use crate::storage::StorageManager;
use crate::watch::{FileWatcher, WatchConfig, WatchEvent};

/// Extract peer_id and storage_id as clean display strings from a `PeerInfo`.
pub(crate) fn format_peer_info(info: &Option<samod::PeerInfo>) -> (String, String) {
    match info {
        Some(info) => (
            info.peer_id.to_string(),
            info.storage_id
                .as_ref()
                .map_or_else(|| "-".to_string(), |s| s.to_string()),
        ),
        None => ("-".to_string(), "-".to_string()),
    }
}

/// Health check response
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_root: Option<String>,
    qmd_file_count: usize,
    index_document_id: String,
}

/// List of discovered files (from filesystem)
#[derive(Serialize)]
struct FilesResponse {
    qmd_files: Vec<String>,
}

/// Document entry in the index
#[derive(Serialize)]
struct DocumentEntry {
    path: String,
    document_id: String,
}

/// List of documents (from index)
#[derive(Serialize)]
struct DocumentsResponse {
    documents: Vec<DocumentEntry>,
}

/// Single document response
#[derive(Serialize)]
struct DocumentResponse {
    document_id: String,
    path: Option<String>,
    // For now we just return metadata; actual content would require
    // serializing the automerge document which is a future task
}

/// Error response
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

/// Update document request
#[derive(Deserialize)]
struct UpdateDocumentRequest {
    // For testing: just a simple key-value pair to put in the document
    key: String,
    value: String,
}

/// Build a Content-Security-Policy header value from the auth configuration.
///
/// Defense-in-depth against XSS: even with HttpOnly cookies eliminating
/// credential theft, XSS can still make authenticated requests from the
/// victim's browser. CSP limits what injected scripts can do.
///
/// The CSP is constructed dynamically from the OIDC issuer origin and
/// configured image domains (for profile pictures).
///
/// The issuer URL and image domains are validated at [`auth::AuthConfig`]
/// construction time, so this function cannot fail from invalid config.
fn build_csp(config: &auth::AuthConfig) -> String {
    let issuer_origin = config.issuer_origin();

    let img_src = config
        .image_domains
        .iter()
        .map(|d| format!("https://{d}"))
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        "default-src 'self'; \
         script-src 'self' {issuer_origin}; \
         style-src 'self' 'unsafe-inline'; \
         font-src 'self'; \
         img-src 'self' data: {img_src}; \
         connect-src 'self' {issuer_origin}; \
         frame-src {issuer_origin}"
    )
}

/// Cookie name for the hub authentication token.
const AUTH_COOKIE_NAME: &str = "quarto_hub_token";

/// Cookie Max-Age in seconds (1 hour, matches typical OIDC ID token lifetime).
const AUTH_COOKIE_MAX_AGE: u32 = 3600;

/// JSON error body for auth failures, so clients can distinguish
/// 401 auth errors from other HTTP errors programmatically.
fn unauthorized() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "unauthorized"})),
    )
}

/// Extract the auth token from the `Cookie` header.
///
/// Uses the `cookie` crate parser for RFC 6265 compliance (handles
/// quoted values and other edge cases). Returns `None` if the cookie
/// is absent, the header is not valid UTF-8, or the value is empty.
fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let cookies = headers.get("cookie")?.to_str().ok()?;
    cookies
        .split(';')
        .filter_map(|s| cookie::Cookie::parse(s.trim()).ok())
        .find(|c| c.name() == AUTH_COOKIE_NAME)
        .map(|c| c.value().to_owned())
        .filter(|v| !v.is_empty())
}

/// Build a `Set-Cookie` header value for the auth token.
///
/// The cookie is `HttpOnly` (no JS access), `SameSite=Lax` (sent on
/// same-site requests and top-level navigations), scoped to `Path=/`,
/// and expires after `AUTH_COOKIE_MAX_AGE` seconds. The `Secure` flag
/// is included unless `allow_insecure` is true (HTTP dev mode).
///
/// Uses the `cookie` crate for correct value encoding, preventing
/// injection of extra attributes via malformed token values.
fn build_auth_cookie(token: &str, secure: bool) -> String {
    if token.len() > 3800 {
        tracing::warn!(
            token_len = token.len(),
            "JWT token exceeds 3800 bytes; browsers may silently drop the cookie \
             (4096 byte limit including cookie metadata). Consider server-side sessions \
             if your OIDC provider issues large tokens."
        );
    }
    let mut builder = cookie::Cookie::build((AUTH_COOKIE_NAME, token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(AUTH_COOKIE_MAX_AGE as i64));
    if secure {
        builder = builder.secure(true);
    }
    builder.build().to_string()
}

/// Build a `Set-Cookie` header value that clears the auth cookie.
fn build_clear_cookie() -> String {
    cookie::Cookie::build((AUTH_COOKIE_NAME, ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::ZERO)
        .build()
        .to_string()
}

/// Verify that a state-mutating request includes the CSRF protection header.
///
/// Requires `X-Requested-With: XMLHttpRequest`. Browsers don't allow
/// cross-origin custom headers without a CORS preflight, so this blocks
/// cross-site form POSTs that auto-attach cookies. Same mechanism as
/// Django and Rails.
fn check_csrf(headers: &HeaderMap) -> std::result::Result<(), StatusCode> {
    let ok = headers
        .get("x-requested-with")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("xmlhttprequest"));
    if ok {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Verify that the WebSocket upgrade `Origin` matches the request `Host`.
///
/// Browsers send cookies on WebSocket upgrades but don't enforce CORS
/// preflight, so a cross-origin page could open an authenticated
/// WebSocket. Comparing `Origin` against `Host` blocks this.
fn check_ws_origin(headers: &HeaderMap) -> std::result::Result<(), StatusCode> {
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::FORBIDDEN)?;

    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::FORBIDDEN)?;

    // Strip scheme from Origin to get host:port (e.g. "https://example.com:3000" → "example.com:3000")
    let origin_host = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .unwrap_or(origin);

    if origin_host == host {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Log request method and path only — never the query string.
/// Auth tokens are now in HttpOnly cookies (not query strings), but
/// redacting query strings is still good practice for defense-in-depth.
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

/// Axum extractor that validates the auth cookie before the handler runs.
///
/// If auth is disabled, extraction always succeeds. If auth is enabled,
/// the `quarto_hub_token` cookie must be present and contain a valid JWT.
/// Returns 401 with a JSON body on failure.
struct Authenticated;

impl<S> FromRequestParts<S> for Authenticated
where
    SharedContext: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let ctx = SharedContext::from_ref(state);
        let token = cookie_token(&parts.headers);
        ctx.authenticate(token.as_deref())
            .await
            .map_err(|_| unauthorized())?;
        Ok(Authenticated)
    }
}

/// Health check endpoint
async fn health(_auth: Authenticated, State(ctx): State<SharedContext>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        project_root: ctx
            .storage()
            .project_root()
            .map(|p| p.display().to_string()),
        qmd_file_count: ctx.project_files().map_or(0, |pf| pf.qmd_files.len()),
        index_document_id: ctx.index().document_id(),
    })
}

/// List discovered files (from filesystem)
async fn list_files(_auth: Authenticated, State(ctx): State<SharedContext>) -> Json<FilesResponse> {
    Json(FilesResponse {
        qmd_files: ctx
            .project_files()
            .map(|pf| {
                pf.qmd_files
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// List all documents from the index
async fn list_documents(
    _auth: Authenticated,
    State(ctx): State<SharedContext>,
) -> Json<DocumentsResponse> {
    let files = ctx.index().get_all_files();

    let documents: Vec<DocumentEntry> = files
        .into_iter()
        .map(|(path, document_id)| DocumentEntry { path, document_id })
        .collect();

    Json(DocumentsResponse { documents })
}

/// Get a single document by ID
async fn get_document(
    _auth: Authenticated,
    State(ctx): State<SharedContext>,
    Path(doc_id_str): Path<String>,
) -> impl IntoResponse {
    // Validate the document ID format
    let doc_id = match DocumentId::from_str(&doc_id_str) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid document ID format".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Try to find the document
    match ctx.repo().find(doc_id).await {
        Ok(Some(_handle)) => {
            // Find the path for this document ID (reverse lookup)
            let path = ctx
                .index()
                .get_all_files()
                .into_iter()
                .find(|(_, id)| id == &doc_id_str)
                .map(|(p, _)| p);

            Json(DocumentResponse {
                document_id: doc_id_str,
                path,
            })
            .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Document not found".to_string(),
            }),
        )
            .into_response(),
        Err(_stopped) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Repository is stopped".to_string(),
            }),
        )
            .into_response(),
    }
}

/// Update a document (for testing)
///
/// This is a simple endpoint that puts a key-value pair into the document.
/// In a real implementation, the document schema would be more structured.
async fn update_document(
    _auth: Authenticated,
    headers: HeaderMap,
    State(ctx): State<SharedContext>,
    Path(doc_id_str): Path<String>,
    Json(request): Json<UpdateDocumentRequest>,
) -> impl IntoResponse {
    use automerge::{ROOT, transaction::Transactable};

    if let Err(status) = check_csrf(&headers) {
        return (
            status,
            Json(ErrorResponse {
                error: "csrf check failed".to_string(),
            }),
        )
            .into_response();
    }

    // Validate the document ID format
    let doc_id = match DocumentId::from_str(&doc_id_str) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid document ID format".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Try to find the document
    match ctx.repo().find(doc_id).await {
        Ok(Some(handle)) => {
            // Update the document
            let result = handle.with_document(|doc| {
                doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                    tx.put(ROOT, &request.key, &request.value)?;
                    Ok(())
                })
            });

            match result {
                Ok(_) => Json(serde_json::json!({
                    "status": "updated",
                    "document_id": doc_id_str,
                    "key": request.key,
                    "value": request.value
                }))
                .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Failed to update document: {:?}", e),
                    }),
                )
                    .into_response(),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Document not found".to_string(),
            }),
        )
            .into_response(),
        Err(_stopped) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Repository is stopped".to_string(),
            }),
        )
            .into_response(),
    }
}

/// Google-frontend-specific OAuth2 redirect callback form data.
///
/// When `GoogleLogin` uses `ux_mode="redirect"`, Google POSTs the credential
/// JWT and a CSRF token to the `login_uri` after the user authenticates.
///
/// This form structure is specific to Google's Sign-In library. Non-Google
/// OIDC frontends should use `POST /auth/refresh` instead.
#[derive(Deserialize)]
struct AuthCallbackForm {
    credential: String,
    g_csrf_token: String,
}

/// Handle Google-frontend-specific OAuth2 redirect callback.
///
/// Receives the credential JWT from Google's POST, validates the CSRF token
/// and the JWT itself, then sets an HttpOnly cookie and redirects to `/`.
///
/// Validating the JWT here (not just in subsequent API calls) prevents
/// setting a cookie with a bogus credential.
///
/// **Google-specific**: This endpoint is tightly coupled to Google's Sign-In
/// library (which controls the POST body and `g_csrf_token` cookie). Non-Google
/// OIDC frontends should use `POST /auth/refresh` instead — it accepts a JWT
/// via JSON POST, validates through the full JWKS/issuer/allowlist pipeline,
/// and is protected by the standard `X-Requested-With` CSRF check.
///
/// **CSRF**: This endpoint is excluded from the `X-Requested-With` CSRF
/// check because it receives a cross-origin POST from Google's servers.
/// Google's own `g_csrf_token` cookie provides CSRF protection instead.
async fn auth_callback(
    State(ctx): State<SharedContext>,
    headers: HeaderMap,
    Form(form): Form<AuthCallbackForm>,
) -> impl IntoResponse {
    // Validate CSRF: g_csrf_token cookie must match the form value.
    // Google sets this cookie and includes the same value in the POST body.
    let cookie_csrf = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .filter_map(|s| cookie::Cookie::parse(s.trim()).ok())
                .find(|c| c.name() == "g_csrf_token")
                .map(|c| c.value().to_owned())
        });

    if form.g_csrf_token.is_empty() || cookie_csrf.as_deref() != Some(form.g_csrf_token.as_str()) {
        return Redirect::to("/?auth_error").into_response();
    }

    // Validate the JWT before setting the cookie.
    if let Err(_status) = ctx.authenticate(Some(&form.credential)).await {
        return Redirect::to("/?auth_error").into_response();
    }

    // Set HttpOnly cookie and redirect to clean `/`.
    let secure = !ctx.allow_insecure_auth();
    let cookie = build_auth_cookie(&form.credential, secure);
    let mut response = Redirect::to("/").into_response();
    response
        .headers_mut()
        .insert(http::header::SET_COOKIE, cookie.parse().unwrap());
    response
}

/// Response for GET /auth/me.
#[derive(Serialize)]
struct AuthMeResponse {
    email: String,
    name: Option<String>,
    picture: Option<String>,
    actor_id: String,
}

/// Request body for POST /auth/refresh.
#[derive(Deserialize)]
struct RefreshRequest {
    credential: String,
}

/// Return user info from a valid cookie. 401 if missing/expired.
///
/// The client calls this on mount to check if the user is authenticated
/// without needing to decode the JWT client-side.
async fn auth_me(
    headers: HeaderMap,
    State(ctx): State<SharedContext>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token = cookie_token(&headers);
    let claims = ctx
        .authenticate_claims(token.as_deref())
        .await
        .map_err(|_| unauthorized())?;
    let actor_id = crate::auth::sub_to_actor_id(&claims.sub);
    Ok(Json(AuthMeResponse {
        email: claims.email,
        name: claims.name,
        picture: claims.picture,
        actor_id,
    }))
}

/// Clear the auth cookie.
///
/// Sets `Max-Age=0` so the browser deletes the cookie immediately.
/// Requires `X-Requested-With: XMLHttpRequest` for CSRF protection.
async fn auth_logout(
    _auth: Authenticated,
    headers: HeaderMap,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    check_csrf(&headers)
        .map_err(|s| (s, Json(serde_json::json!({"error": "csrf check failed"}))))?;

    let cookie = build_clear_cookie();
    let mut response = Json(serde_json::json!({"status": "ok"})).into_response();
    response
        .headers_mut()
        .insert(http::header::SET_COOKIE, cookie.parse().unwrap());
    Ok(response)
}

/// Validate a fresh OIDC JWT and set a new cookie.
///
/// Called by the client after obtaining a new credential from the OIDC provider
/// (e.g. Google One Tap silent refresh). The new JWT goes through the full
/// `authenticate()` path (signature, audience, issuer, email allowlist)
/// before setting the cookie.
///
/// This is also the recommended credential submission endpoint for non-Google
/// OIDC frontends (instead of the Google-specific `/auth/callback`).
///
/// Requires `X-Requested-With: XMLHttpRequest` for CSRF protection.
async fn auth_refresh(
    headers: HeaderMap,
    State(ctx): State<SharedContext>,
    Json(body): Json<RefreshRequest>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    check_csrf(&headers)
        .map_err(|s| (s, Json(serde_json::json!({"error": "csrf check failed"}))))?;

    // Validate the NEW credential (not the existing cookie — it may be expired).
    ctx.authenticate(Some(&body.credential))
        .await
        .map_err(|_| unauthorized())?;

    let secure = !ctx.allow_insecure_auth();
    let cookie = build_auth_cookie(&body.credential, secure);
    let mut response = Json(serde_json::json!({"status": "ok"})).into_response();
    response
        .headers_mut()
        .insert(http::header::SET_COOKIE, cookie.parse().unwrap());
    Ok(response)
}

/// 404 handler
async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "Not found")
}

/// WebSocket upgrade handler for automerge sync.
///
/// Clients connect here to sync documents in real-time. Auth is via the
/// `quarto_hub_token` HttpOnly cookie (sent automatically by the browser).
/// The `Origin` header is checked to prevent cross-origin WebSocket hijacking.
///
/// **Security note**: the token is validated once at upgrade time. After
/// that, the connection lives until the client disconnects. If a user is
/// removed from the allowlist or their token expires, already-established
/// connections are **not** terminated. This is a deliberate trade-off:
/// re-validating on every message would add latency to every sync
/// operation. Clients naturally reconnect (and re-authenticate) when the
/// frontend detects token expiry.
async fn ws_handler(
    State(ctx): State<SharedContext>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> std::result::Result<impl IntoResponse, StatusCode> {
    let email = if ctx.auth_config().is_some() {
        // In dev mode (allow_insecure_auth), the SPA runs on a different
        // port (Vite :5173) than the hub (:3000). The Vite dev server
        // proxies /ws to the hub so cookies are forwarded, but the Origin
        // header still shows the Vite origin. Skip only the Origin check;
        // cookie auth is still enforced.
        if !ctx.allow_insecure_auth() {
            check_ws_origin(&headers)?;
        }
        let claims = ctx
            .authenticate_claims(cookie_token(&headers).as_deref())
            .await?;
        Some(claims.email)
    } else {
        None
    };

    Ok(ws.on_upgrade(move |socket| handle_websocket(socket, ctx, email)))
}

/// Handle an upgraded WebSocket connection.
async fn handle_websocket(socket: WebSocket, ctx: SharedContext, email: Option<String>) {
    use futures::StreamExt;
    use samod::AcceptorEvent;

    // accept_axum returns immediately; the connection runs in the background
    match ctx.acceptor().accept_axum(socket) {
        Ok(connection) => {
            let mut events = connection.events();
            let mut connected_peer_id = None;

            // Wait for the handshake to complete (ClientConnected) or connection
            // to drop (ClientDisconnected / stream end).
            while let Some(event) = events.next().await {
                match event {
                    AcceptorEvent::ClientConnected {
                        peer_info,
                        connection_id: _,
                    } => {
                        let (peer_id_str, storage_id) = format_peer_info(&Some(peer_info.clone()));

                        // Store peer→email mapping for audit logging
                        if let Some(ref email) = email {
                            ctx.peer_emails()
                                .lock()
                                .unwrap()
                                .insert(peer_info.peer_id.clone(), email.clone());
                        }
                        connected_peer_id = Some(peer_info.peer_id);

                        info!(
                            peer_id = peer_id_str,
                            storage_id,
                            email = email.as_deref().unwrap_or("-"),
                            "WebSocket client connected"
                        );
                    }
                    AcceptorEvent::ClientDisconnected {
                        connection_id: _,
                        reason,
                    } => {
                        // Clean up mapping
                        if let Some(ref peer_id) = connected_peer_id {
                            ctx.peer_emails().lock().unwrap().remove(peer_id);
                        }

                        info!(
                            email = email.as_deref().unwrap_or("-"),
                            reason = ?reason,
                            "WebSocket client disconnected"
                        );
                        break;
                    }
                }
            }
        }
        Err(samod::Stopped) => {
            tracing::warn!("WebSocket rejected: repo is stopped");
        }
    }
}

/// Build the axum router. Auth state (decoder + JWKS refresh handle) is
/// initialized here and owned by HubContext for the server's lifetime.
async fn build_router(ctx: SharedContext) -> Result<Router> {
    if let Some(config) = ctx.auth_config() {
        let auth_state = auth::build_auth_state(config).await.map_err(|e| {
            crate::error::Error::Server(format!("Failed to initialize OIDC JWKS decoder: {e}"))
        })?;
        ctx.set_auth_state(auth_state)
            .map_err(|e| crate::error::Error::Server(e.to_string()))?;
    }

    let mut router = Router::new()
        .route("/health", get(health))
        .route("/api/files", get(list_files))
        .route("/api/documents", get(list_documents))
        .route(
            "/api/documents/{id}",
            get(get_document).put(update_document),
        )
        // Auth endpoints
        .route("/auth/me", get(auth_me))
        .route("/auth/logout", post(auth_logout))
        .route("/auth/refresh", post(auth_refresh))
        // WebSocket endpoint for automerge sync
        // Root path "/" is the standard location used by sync.automerge.org
        // "/ws" is kept for backward compatibility
        .route("/", get(ws_handler))
        .route("/ws", get(ws_handler))
        .fallback(not_found)
        .layer(TraceLayer::new_for_http().make_span_with(RedactedMakeSpan));

    // Google-specific redirect callback: only registered when the issuer is Google.
    // Non-Google OIDC frontends should use POST /auth/refresh instead.
    if ctx.auth_config().is_some_and(|c| c.is_google_issuer()) {
        router = router.route("/auth/callback", post(auth_callback));
    }

    // Add Content-Security-Policy header when auth is enabled.
    // Without auth there are no OIDC provider scripts to allow.
    if let Some(config) = ctx.auth_config() {
        let csp = build_csp(config);
        router = router.layer(SetResponseHeaderLayer::if_not_present(
            http::header::HeaderName::from_static("content-security-policy"),
            http::header::HeaderValue::from_str(&csp)
                .map_err(|e| crate::error::Error::Server(format!("Invalid CSP header: {e}")))?,
        ));
    }

    Ok(router.with_state(ctx))
}

/// Run the hub server.
///
/// This function blocks until the server is shut down.
/// On shutdown (SIGTERM, SIGINT, or Ctrl-C), it performs a final filesystem sync
/// to ensure all automerge changes are written to disk.
///
/// If `sync_interval_secs` is configured, a background task will periodically
/// sync all documents to the filesystem for crash resilience.
pub async fn run_server(storage: StorageManager, config: HubConfig) -> Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    let sync_interval = config.sync_interval_secs;
    let watch_enabled = config.watch_enabled;
    let watch_debounce_ms = config.watch_debounce_ms;
    let project_root = storage.project_root().map(|p| p.to_path_buf());
    let has_project = project_root.is_some();

    // HubContext::new is now async (initializes samod repo and performs initial sync)
    let ctx = Arc::new(HubContext::new(storage, config).await?);
    let ctx_for_sync = ctx.clone();
    let ctx_for_watch = ctx.clone();
    let ctx_for_shutdown = ctx.clone();

    let router = build_router(ctx).await?;

    let listener = TcpListener::bind(&addr).await?;
    if has_project {
        info!(%addr, "Hub server listening (project mode)");
    } else {
        info!(%addr, "Hub server listening (standalone sync mode)");
    }

    // Create shutdown signal channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Spawn task to listen for OS signals and trigger shutdown
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    // Spawn periodic sync task if interval is configured and we have a project
    let periodic_sync_handle = if has_project {
        if let Some(interval_secs) = sync_interval {
            let shutdown_rx = shutdown_rx.clone();
            info!(interval_secs = interval_secs, "Starting periodic sync task");
            Some(tokio::spawn(async move {
                run_periodic_sync(ctx_for_sync, interval_secs, shutdown_rx).await;
            }))
        } else {
            debug!("Periodic sync disabled");
            None
        }
    } else {
        debug!("Standalone mode: periodic sync not needed");
        None
    };

    // Spawn file watcher task if enabled and we have a project
    let watcher_handle = if has_project && watch_enabled {
        let project_root = project_root.expect("has_project is true");
        let shutdown_rx = shutdown_rx.clone();
        let watch_config = WatchConfig {
            debounce_ms: watch_debounce_ms,
        };
        match FileWatcher::new(&project_root, watch_config) {
            Ok(watcher) => {
                info!("Starting filesystem watcher");
                Some(tokio::spawn(async move {
                    run_file_watcher(ctx_for_watch, watcher, shutdown_rx).await;
                }))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to start filesystem watcher, continuing without it");
                None
            }
        }
    } else if has_project {
        debug!("Filesystem watcher disabled");
        None
    } else {
        debug!("Standalone mode: filesystem watcher not needed");
        None
    };

    // Run server with graceful shutdown
    let mut shutdown_rx_server = shutdown_rx.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            // Wait until shutdown is signaled
            let _ = shutdown_rx_server.wait_for(|&v| v).await;
            info!("Server shutting down...");
        })
        .await
        .map_err(|e| crate::error::Error::Server(e.to_string()))?;

    // Wait for background tasks to finish
    if let Some(handle) = periodic_sync_handle {
        debug!("Waiting for periodic sync task to finish...");
        let _ = handle.await;
    }
    if let Some(handle) = watcher_handle {
        debug!("Waiting for file watcher task to finish...");
        let _ = handle.await;
    }

    // Perform final sync on shutdown (no-op in standalone mode)
    if has_project {
        info!("Performing final filesystem sync before shutdown...");
        let sync_result = ctx_for_shutdown.sync_all().await;
        info!(
            synced = sync_result.total_synced(),
            errors = sync_result.errors.len(),
            "Final filesystem sync complete"
        );
    }

    Ok(())
}

/// Run periodic filesystem sync in a background task.
///
/// This task runs until the shutdown signal is received, syncing all documents
/// to the filesystem at the specified interval.
async fn run_periodic_sync(
    ctx: Arc<HubContext>,
    interval_secs: u64,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

    // First tick completes immediately; we don't want to sync right after startup
    // since we just did an initial sync, so skip it
    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                debug!("Running periodic filesystem sync...");
                let result = ctx.sync_all().await;
                if result.total_synced() > 0 || result.has_errors() {
                    info!(
                        synced = result.total_synced(),
                        no_changes = result.no_changes,
                        automerge_changed = result.automerge_changed,
                        filesystem_changed = result.filesystem_changed,
                        both_changed = result.both_changed,
                        errors = result.errors.len(),
                        "Periodic sync complete"
                    );
                } else {
                    debug!("Periodic sync: no changes");
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    debug!("Periodic sync task shutting down");
                    break;
                }
            }
        }
    }
}

/// Run the filesystem watcher in a background task.
///
/// This task receives events from the file watcher and syncs changed files
/// until the shutdown signal is received.
async fn run_file_watcher(
    ctx: Arc<HubContext>,
    mut watcher: FileWatcher,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            event = watcher.recv() => {
                match event {
                    Some(WatchEvent::Modified(path)) => {
                        debug!(path = %path.display(), "File change detected, syncing...");
                        match ctx.sync_file(&path).await {
                            Ok(Some(result)) => {
                                debug!(
                                    path = %path.display(),
                                    result = ?result,
                                    "File synced successfully"
                                );
                            }
                            Ok(None) => {
                                debug!(path = %path.display(), "File not in index, skipping");
                            }
                            Err(e) => {
                                tracing::warn!(
                                    path = %path.display(),
                                    error = %e,
                                    "Failed to sync file"
                                );
                            }
                        }
                    }
                    None => {
                        // Watcher stopped
                        debug!("File watcher stopped");
                        break;
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    debug!("File watcher task shutting down");
                    break;
                }
            }
        }
    }
}

/// Wait for shutdown signals (Ctrl-C, SIGTERM, SIGINT).
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl-C, initiating graceful shutdown...");
        }
        _ = terminate => {
            info!("Received SIGTERM, initiating graceful shutdown...");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers_with(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                http::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    // ── cookie_token ──────────────────────────────────────────────

    #[test]
    fn cookie_token_extracts_value() {
        let h = headers_with(&[("cookie", "quarto_hub_token=abc123")]);
        assert_eq!(cookie_token(&h).as_deref(), Some("abc123"));
    }

    #[test]
    fn cookie_token_among_multiple_cookies() {
        let h = headers_with(&[(
            "cookie",
            "other=x; quarto_hub_token=jwt.value.here; third=y",
        )]);
        assert_eq!(cookie_token(&h).as_deref(), Some("jwt.value.here"));
    }

    #[test]
    fn cookie_token_missing() {
        let h = headers_with(&[("cookie", "other=x; another=y")]);
        assert_eq!(cookie_token(&h), None);
    }

    #[test]
    fn cookie_token_no_cookie_header() {
        let h = HeaderMap::new();
        assert_eq!(cookie_token(&h), None);
    }

    #[test]
    fn cookie_token_empty_value() {
        let h = headers_with(&[("cookie", "quarto_hub_token=")]);
        assert_eq!(cookie_token(&h), None);
    }

    #[test]
    fn cookie_token_prefix_mismatch() {
        // "quarto_hub_token_v2" should NOT match "quarto_hub_token"
        let h = headers_with(&[("cookie", "quarto_hub_token_v2=abc")]);
        assert_eq!(cookie_token(&h), None);
    }

    // ── build_auth_cookie ─────────────────────────────────────────

    #[test]
    fn build_auth_cookie_secure() {
        let cookie = build_auth_cookie("mytoken", true);
        assert!(cookie.starts_with("quarto_hub_token=mytoken;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=3600"));
    }

    #[test]
    fn build_auth_cookie_insecure() {
        let cookie = build_auth_cookie("mytoken", false);
        assert!(cookie.starts_with("quarto_hub_token=mytoken;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(!cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Lax"));
    }

    #[test]
    fn build_clear_cookie_has_zero_max_age() {
        let cookie = build_clear_cookie();
        assert!(cookie.contains("Max-Age=0"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.starts_with("quarto_hub_token=;"));
    }

    // ── check_csrf ────────────────────────────────────────────────

    #[test]
    fn csrf_accepts_xmlhttprequest() {
        let h = headers_with(&[("x-requested-with", "XMLHttpRequest")]);
        assert!(check_csrf(&h).is_ok());
    }

    #[test]
    fn csrf_accepts_case_insensitive() {
        let h = headers_with(&[("x-requested-with", "xmlhttprequest")]);
        assert!(check_csrf(&h).is_ok());
    }

    #[test]
    fn csrf_rejects_missing_header() {
        let h = HeaderMap::new();
        assert_eq!(check_csrf(&h), Err(StatusCode::FORBIDDEN));
    }

    #[test]
    fn csrf_rejects_wrong_value() {
        let h = headers_with(&[("x-requested-with", "fetch")]);
        assert_eq!(check_csrf(&h), Err(StatusCode::FORBIDDEN));
    }

    // ── check_ws_origin ───────────────────────────────────────────

    #[test]
    fn ws_origin_accepts_matching_https() {
        let h = headers_with(&[
            ("origin", "https://hub.example.com"),
            ("host", "hub.example.com"),
        ]);
        assert!(check_ws_origin(&h).is_ok());
    }

    #[test]
    fn ws_origin_accepts_matching_http() {
        let h = headers_with(&[
            ("origin", "http://localhost:3000"),
            ("host", "localhost:3000"),
        ]);
        assert!(check_ws_origin(&h).is_ok());
    }

    #[test]
    fn ws_origin_rejects_mismatch() {
        let h = headers_with(&[("origin", "https://evil.com"), ("host", "hub.example.com")]);
        assert_eq!(check_ws_origin(&h), Err(StatusCode::FORBIDDEN));
    }

    #[test]
    fn ws_origin_rejects_missing_origin() {
        let h = headers_with(&[("host", "hub.example.com")]);
        assert_eq!(check_ws_origin(&h), Err(StatusCode::FORBIDDEN));
    }

    #[test]
    fn ws_origin_rejects_missing_host() {
        let h = headers_with(&[("origin", "https://hub.example.com")]);
        assert_eq!(check_ws_origin(&h), Err(StatusCode::FORBIDDEN));
    }

    // ── CSP ───────────────────────────────────────────────────────

    fn google_auth_config() -> auth::AuthConfig {
        auth::AuthConfig::new(
            "test-client-id".to_string(),
            "https://accounts.google.com".to_string(),
            vec!["lh3.googleusercontent.com".to_string()],
            None,
            None,
        )
        .unwrap()
    }

    #[test]
    fn csp_google_issuer() {
        let config = google_auth_config();
        let csp = build_csp(&config);
        assert!(csp.contains("https://accounts.google.com"));
        assert!(csp.contains("https://lh3.googleusercontent.com"));
    }

    #[test]
    fn csp_custom_issuer() {
        let config = auth::AuthConfig::new(
            "test".to_string(),
            "https://login.microsoftonline.com/tenant-id/v2.0".to_string(),
            vec!["graph.microsoft.com".to_string()],
            None,
            None,
        )
        .unwrap();
        let csp = build_csp(&config);
        assert!(csp.contains("https://login.microsoftonline.com"));
        assert!(csp.contains("https://graph.microsoft.com"));
        assert!(!csp.contains("accounts.google.com"));
    }

    #[test]
    fn csp_custom_image_domains() {
        let config = auth::AuthConfig::new(
            "test".to_string(),
            "https://accounts.google.com".to_string(),
            vec![
                "avatars.example.com".to_string(),
                "cdn.example.com".to_string(),
            ],
            None,
            None,
        )
        .unwrap();
        let csp = build_csp(&config);
        assert!(csp.contains("https://avatars.example.com"));
        assert!(csp.contains("https://cdn.example.com"));
    }

    #[test]
    fn csp_default_image_domain_when_empty() {
        let config = auth::AuthConfig::new(
            "test".to_string(),
            "https://accounts.google.com".to_string(),
            vec![],
            None,
            None,
        )
        .unwrap();
        let csp = build_csp(&config);
        assert!(csp.contains("https://lh3.googleusercontent.com"));
    }

    #[test]
    fn csp_disallows_arbitrary_websocket() {
        let config = google_auth_config();
        let csp = build_csp(&config);
        let connect_src = csp.split(';').find(|d| d.contains("connect-src")).unwrap();
        let has_bare_ws = connect_src
            .split_whitespace()
            .any(|tok| tok == "ws:" || tok == "wss:");
        assert!(
            !has_bare_ws,
            "connect-src must not allow arbitrary WebSocket hosts"
        );
    }

    #[test]
    fn csp_blocks_inline_scripts() {
        let config = google_auth_config();
        let csp = build_csp(&config);
        let script_src = csp.split(';').find(|d| d.contains("script-src")).unwrap();
        assert!(!script_src.contains("unsafe-inline"));
    }

    #[test]
    fn csp_has_default_self() {
        let config = google_auth_config();
        let csp = build_csp(&config);
        assert!(csp.contains("default-src 'self'"));
    }

    // ── AuthCallbackForm ──────────────────────────────────────────

    #[test]
    fn auth_callback_form_deserializes() {
        // AuthCallbackForm is used by axum's Form extractor which parses
        // URL-encoded POST bodies. Verify it has the expected fields by
        // deserializing from JSON (same serde derive).
        let form: AuthCallbackForm = serde_json::from_value(serde_json::json!({
            "credential": "eyJhbGciOiJSUzI1NiJ9.test",
            "g_csrf_token": "abc123"
        }))
        .unwrap();
        assert_eq!(form.credential, "eyJhbGciOiJSUzI1NiJ9.test");
        assert_eq!(form.g_csrf_token, "abc123");
    }

    // ── format_peer_info ──────────────────────────────────────────

    #[test]
    fn format_peer_info_with_both_ids() {
        let info = Some(samod::PeerInfo {
            peer_id: samod::PeerId::from("peer-abc123"),
            storage_id: Some(samod::StorageId::from("store-xyz")),
        });
        assert_eq!(
            format_peer_info(&info),
            ("peer-abc123".to_string(), "store-xyz".to_string())
        );
    }

    #[test]
    fn format_peer_info_without_storage_id() {
        let info = Some(samod::PeerInfo {
            peer_id: samod::PeerId::from("peer-abc123"),
            storage_id: None,
        });
        assert_eq!(
            format_peer_info(&info),
            ("peer-abc123".to_string(), "-".to_string())
        );
    }

    #[test]
    fn format_peer_info_none() {
        assert_eq!(format_peer_info(&None), ("-".to_string(), "-".to_string()));
    }
}
