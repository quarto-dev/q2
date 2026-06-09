//! HTTP server setup and routing

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{
        Form, FromRef, FromRequestParts, Path, Query, State,
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

/// Source the credential was attached on. Cookie-authenticated
/// requests still require CSRF + WS-Origin checks (browsers attach
/// cookies automatically); Bearer-authenticated requests come from
/// non-browser clients (hub-mcp) which are not subject to either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    Cookie,
    Bearer,
}

impl CredentialKind {
    fn label(self) -> &'static str {
        match self {
            CredentialKind::Cookie => "cookie",
            CredentialKind::Bearer => "bearer",
        }
    }
}

/// A request's auth credential, normalized to the JWT it carries.
#[derive(Debug, Clone)]
pub enum Credential {
    Cookie(String),
    Bearer(String),
}

impl Credential {
    pub fn token(&self) -> &str {
        match self {
            Credential::Cookie(t) | Credential::Bearer(t) => t,
        }
    }
    pub fn kind(&self) -> CredentialKind {
        match self {
            Credential::Cookie(_) => CredentialKind::Cookie,
            Credential::Bearer(_) => CredentialKind::Bearer,
        }
    }
}

/// Failure mode for [`extract_credential`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialError {
    /// Both `Cookie` and `Authorization: Bearer` were attached. We
    /// reject with HTTP 400 + body `{"error":"conflicting_credentials"}`
    /// rather than picking one — the auth-confusion CVE shape this
    /// rule blocks is what drove Phase 2 of the device-flow plan.
    Conflicting,
    /// `Authorization` header present but the scheme is not `Bearer`
    /// (we explicitly reject `Basic`, `Token`, etc.). 401, never 400.
    UnsupportedScheme,
}

/// Extract a Cookie/Bearer credential from request headers.
///
/// Returns:
///   * `Ok(Some(Credential::Cookie))` — cookie present, no Authorization.
///   * `Ok(Some(Credential::Bearer))` — Authorization: Bearer present,
///     no cookie.
///   * `Ok(None)` — neither attached (anonymous request).
///   * `Err(Conflicting)` — both attached. Caller maps to 400.
///   * `Err(UnsupportedScheme)` — non-Bearer Authorization. Caller maps
///     to 401.
///
/// The dual-credential 400 rule MUST run before CSRF / WS-Origin
/// checks: a request that smuggles a stolen Bearer with a same-origin
/// cookie must not be silently routed through one credential path or
/// the other.
pub fn extract_credential(
    headers: &HeaderMap,
) -> std::result::Result<Option<Credential>, CredentialError> {
    let cookie = cookie_token(headers);
    let auth_header = headers.get(http::header::AUTHORIZATION);

    let bearer = match auth_header {
        Some(value) => {
            let raw = value
                .to_str()
                .map_err(|_| CredentialError::UnsupportedScheme)?;
            let trimmed = raw.trim();
            // `Bearer <token>` (case-insensitive scheme per RFC 6750 §2.1).
            if let Some(token) = trimmed
                .strip_prefix("Bearer ")
                .or_else(|| trimmed.strip_prefix("bearer "))
            {
                let token = token.trim();
                if token.is_empty() {
                    return Err(CredentialError::UnsupportedScheme);
                }
                Some(token.to_string())
            } else {
                return Err(CredentialError::UnsupportedScheme);
            }
        }
        None => None,
    };

    match (cookie, bearer) {
        (Some(_), Some(_)) => Err(CredentialError::Conflicting),
        (Some(c), None) => Ok(Some(Credential::Cookie(c))),
        (None, Some(b)) => Ok(Some(Credential::Bearer(b))),
        (None, None) => Ok(None),
    }
}

/// JSON error body for dual-credential rejection. Stable shape — the
/// `error` discriminator is consumed by hub-mcp's connection manager
/// to detect the auth-confusion case without parsing free-form text.
fn conflicting_credentials() -> (StatusCode, Json<serde_json::Value>) {
    tracing::event!(
        target: "quarto_hub::audit",
        tracing::Level::WARN,
        action = "auth_fail",
        outcome = "deny",
        credential_kind = "bearer",
        detail = "conflicting_credentials",
    );
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": "conflicting_credentials"})),
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

/// Axum extractor that validates the request's auth credential before
/// the handler runs.
///
/// Accepts either an `Authorization: Bearer <jwt>` header (used by
/// quarto-hub-mcp) or the `quarto_hub_token` HttpOnly cookie (used by
/// the SPA). A request that carries BOTH is rejected with HTTP 400 —
/// see [`extract_credential`]. The `credential_kind` field on this
/// extractor is what mutating handlers gate CSRF / WS-Origin checks
/// on: cookie auth still requires them; Bearer auth does not.
///
/// When auth is disabled (`auth_config: None`), the extractor still
/// returns successfully so handlers don't need separate code paths —
/// the credential kind defaults to `Cookie` (preserving the existing
/// CSRF-applies-always semantic for no-auth deployments).
pub struct Authenticated {
    pub credential_kind: CredentialKind,
}

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
        let credential = match extract_credential(&parts.headers) {
            Ok(c) => c,
            Err(CredentialError::Conflicting) => {
                return Err(conflicting_credentials());
            }
            Err(CredentialError::UnsupportedScheme) => {
                return Err(unauthorized());
            }
        };

        // Auth disabled — no credential to validate, default kind to Cookie
        // so CSRF still applies (no behavior change for no-auth setups).
        if ctx.auth_config().is_none() {
            return Ok(Authenticated {
                credential_kind: CredentialKind::Cookie,
            });
        }

        let credential = credential.ok_or_else(unauthorized)?;
        let kind = credential.kind();
        // Preserve the original status code: `authenticate_claims_for_kind`
        // returns 401 for invalid/missing credentials but 403 for valid
        // credentials whose user is not allowlisted. Collapsing both
        // to 401 loses the distinction that the plan's allowlist-parity
        // tests assert.
        ctx.authenticate_claims_for_kind(Some(credential.token()), kind.label())
            .await
            .map_err(|status| {
                let body = if status == StatusCode::FORBIDDEN {
                    serde_json::json!({"error": "forbidden"})
                } else {
                    serde_json::json!({"error": "unauthorized"})
                };
                (status, Json(body))
            })?;
        Ok(Authenticated {
            credential_kind: kind,
        })
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
    auth: Authenticated,
    headers: HeaderMap,
    State(ctx): State<SharedContext>,
    Path(doc_id_str): Path<String>,
    Json(request): Json<UpdateDocumentRequest>,
) -> impl IntoResponse {
    use automerge::{ROOT, transaction::Transactable};

    // CSRF protection applies to cookie-authenticated requests only.
    // Browsers attach cookies automatically across origins; Bearer
    // tokens are explicit, so cross-site form posts can't smuggle them.
    if auth.credential_kind == CredentialKind::Cookie {
        if let Err(status) = check_csrf(&headers) {
            return (
                status,
                Json(ErrorResponse {
                    error: "csrf check failed".to_string(),
                }),
            )
                .into_response();
        }
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

/// Form data for `POST /auth/callback`.
///
/// Providers that use `response_mode=form_post` (or equivalent) POST a
/// credential JWT plus a provider-specific CSRF field. Fields are optional at
/// the struct level; `validate_callback_csrf` enforces which ones are required
/// for the active provider.
#[derive(Deserialize)]
struct AuthCallbackForm {
    credential: String,
    /// Google double-submit CSRF token (GIS `ux_mode=redirect`). Present only
    /// for Google providers.
    g_csrf_token: Option<String>,
}

/// Validate the CSRF token for `POST /auth/callback`.
///
/// Dispatches to the provider-specific check determined by `mode`:
///
/// - `GoogleDoubleSubmit`: the `g_csrf_token` form value must equal the
///   `g_csrf_token` cookie set by GIS on the hub origin before navigation.
/// - `OidcState`: not yet implemented; returns `false` so the callback fails
///   safe until the pre-flight endpoint and signing key are in place.
fn validate_callback_csrf(
    mode: &auth::CallbackCsrfMode,
    form: &AuthCallbackForm,
    headers: &HeaderMap,
) -> bool {
    match mode {
        auth::CallbackCsrfMode::GoogleDoubleSubmit => {
            let Some(token) = form.g_csrf_token.as_deref().filter(|t| !t.is_empty()) else {
                return false;
            };
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
            cookie_csrf.as_deref() == Some(token)
        }
        auth::CallbackCsrfMode::OidcState { .. } => {
            // Stateful validation requires a hub-set signed cookie from a
            // pre-flight endpoint that does not yet exist. Fail safe.
            false
        }
    }
}

/// Handle `POST /auth/callback` — credential delivery via form POST.
///
/// Registered for providers where [`AuthConfig::uses_form_post_callback()`]
/// returns `true`. Validates the provider-specific CSRF token, then validates
/// the credential JWT and sets an HttpOnly cookie.
///
/// **CSRF**: excluded from the `X-Requested-With` check because the POST
/// originates from the IdP (cross-origin). Provider-specific CSRF is handled
/// by [`validate_callback_csrf`] instead.
async fn auth_callback(
    State(ctx): State<SharedContext>,
    headers: HeaderMap,
    Form(form): Form<AuthCallbackForm>,
) -> impl IntoResponse {
    let mode = ctx
        .auth_config()
        .map(|c| c.callback_csrf_mode())
        .unwrap_or(auth::CallbackCsrfMode::GoogleDoubleSubmit);

    if !validate_callback_csrf(&mode, &form, &headers) {
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
}

/// Query parameters for GET /auth/actor.
#[derive(Deserialize)]
struct AuthActorQuery {
    project: String,
}

/// Response for GET /auth/actor.
#[derive(Serialize)]
struct AuthActorResponse {
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
    Ok(Json(AuthMeResponse {
        email: claims.email,
        name: claims.name,
        picture: claims.picture,
    }))
}

/// Return a per-project actor ID for the authenticated user.
///
/// The actor ID is `HMAC-SHA256(server_secret, sub || "\0" || project_id)`,
/// so the same user gets a different actor ID in each project. Cross-project
/// correlation is impossible without the server secret.
///
/// - Returns 401 if the cookie is missing or invalid.
/// - Returns 400 if the `project` query parameter is missing (Axum extractor).
/// - No server-side project validation: an unknown `project_id` just yields an
///   actor ID that will never match any document content.
async fn auth_actor(
    headers: HeaderMap,
    State(ctx): State<SharedContext>,
    Query(query): Query<AuthActorQuery>,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token = cookie_token(&headers);
    let claims = ctx
        .authenticate_claims(token.as_deref())
        .await
        .map_err(|_| unauthorized())?;
    let actor_id = crate::auth::sub_to_actor_id_for_project(
        ctx.server_secret_bytes(),
        &claims.sub,
        &query.project,
    );
    Ok(Json(AuthActorResponse { actor_id }))
}

/// Clear the auth cookie.
///
/// Sets `Max-Age=0` so the browser deletes the cookie immediately.
/// Requires `X-Requested-With: XMLHttpRequest` for CSRF protection
/// when the caller authenticated via cookie. Bearer callers (hub-mcp)
/// are not subject to CSRF — the header is a no-op cookie-clear for
/// them, kept symmetric for tooling.
async fn auth_logout(
    auth: Authenticated,
    headers: HeaderMap,
) -> std::result::Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.credential_kind == CredentialKind::Cookie {
        check_csrf(&headers)
            .map_err(|s| (s, Json(serde_json::json!({"error": "csrf check failed"}))))?;
    }

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
    // Dual-credential 400 wins over CSRF — same precedence as the
    // `Authenticated` extractor. Without this, a request carrying both
    // a cookie and a Bearer would bypass the conflicting-credentials
    // rule on this endpoint (the cookie path runs without `Authenticated`).
    let bearer_present = match extract_credential(&headers) {
        Ok(Some(c)) => matches!(c.kind(), CredentialKind::Bearer),
        Ok(None) => false,
        Err(CredentialError::Conflicting) => return Err(conflicting_credentials()),
        // Non-Bearer Authorization scheme — let the request through so the
        // body's credential is what gets validated; CSRF still applies.
        Err(CredentialError::UnsupportedScheme) => false,
    };

    if !bearer_present {
        check_csrf(&headers)
            .map_err(|s| (s, Json(serde_json::json!({"error": "csrf check failed"}))))?;
    }

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
/// Clients connect here to sync documents in real-time. Auth is via
/// either the `quarto_hub_token` HttpOnly cookie (SPA) or
/// `Authorization: Bearer <jwt>` header (hub-mcp). The `Origin` header
/// is checked for the cookie path only — Bearer requests come from
/// non-browser clients that cannot be cross-origin-hijacked.
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
) -> axum::response::Response {
    let email = if ctx.auth_config().is_some() {
        let credential = match extract_credential(&headers) {
            Ok(c) => c,
            Err(CredentialError::Conflicting) => {
                return conflicting_credentials().into_response();
            }
            Err(CredentialError::UnsupportedScheme) => {
                return StatusCode::UNAUTHORIZED.into_response();
            }
        };

        let credential = match credential {
            Some(c) => c,
            None => return StatusCode::UNAUTHORIZED.into_response(),
        };

        // Cookie auth applies the Origin check to block cross-origin
        // WebSocket hijacking. In dev mode (allow_insecure_auth), the
        // SPA runs on a different port (Vite :5173) than the hub
        // (:3000) and the Vite dev server proxies /ws with the
        // original Origin — we skip the check there so dev works.
        // Bearer requests come from non-browser MCP clients and
        // cannot be hijacked through Origin, so we skip the check
        // unconditionally for them.
        if credential.kind() == CredentialKind::Cookie && !ctx.allow_insecure_auth() {
            if let Err(status) = check_ws_origin(&headers) {
                return status.into_response();
            }
        }

        match ctx
            .authenticate_claims_for_kind(Some(credential.token()), credential.kind().label())
            .await
        {
            Ok(claims) => Some(claims.email),
            Err(status) => return status.into_response(),
        }
    } else {
        None
    };

    ws.on_upgrade(move |socket| handle_websocket(socket, ctx, email))
        .into_response()
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

                        debug!(
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

                        debug!(
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

/// Build the hub server's axum router. Composable: callers (e.g.
/// `quarto-preview`) can chain `.fallback(...)` to add SPA serving on
/// top, as long as `HubConfig::register_root_ws` is `false` so `/` is
/// available.
///
/// Auth state (decoder + JWKS refresh handle) is initialized here and
/// owned by HubContext for the server's lifetime.
pub async fn build_router(ctx: SharedContext) -> Result<Router> {
    let router = build_router_with_state(ctx.clone()).await?;
    Ok(router.with_state(ctx))
}

/// Same as [`build_router`] but returns the router with its state
/// type still unbound (`Router<SharedContext>`). Callers can register
/// additional routes that extract `State<SharedContext>` before
/// calling `.with_state(ctx)` to finalize.
pub async fn build_router_with_state(ctx: SharedContext) -> Result<Router<SharedContext>> {
    // Skip OIDC discovery when the caller already injected an
    // `AuthState` directly. Integration tests use this seam so they
    // can drive the hub against an `http://localhost` mock provider —
    // production discovery enforces HTTPS in `validate_discovery_document`.
    if let Some(config) = ctx.auth_config() {
        if !ctx.auth_state_initialized() {
            let auth_state = auth::build_auth_state(config).await.map_err(|e| {
                crate::error::Error::Server(format!("Failed to initialize OIDC JWKS decoder: {e}"))
            })?;
            ctx.set_auth_state(auth_state)
                .map_err(|e| crate::error::Error::Server(e.to_string()))?;
        }
    }

    let register_root_ws = ctx.register_root_ws();

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
        .route("/auth/actor", get(auth_actor))
        .route("/auth/logout", post(auth_logout))
        .route("/auth/refresh", post(auth_refresh))
        // WebSocket endpoint for automerge sync at `/ws` (hub-client +
        // q2-preview SPA's canonical path).
        .route("/ws", get(ws_handler))
        .fallback(not_found)
        .layer(TraceLayer::new_for_http().make_span_with(RedactedMakeSpan));

    if register_root_ws {
        // Root path "/" is the additional standard location used by
        // sync.automerge.org. `quarto preview` opts out (its embedded
        // SPA owns `/`) by setting `register_root_ws: false`.
        router = router.route("/", get(ws_handler));
    }

    // Register the form-POST callback route for providers that use it.
    // Add new providers by returning true from AuthConfig::uses_form_post_callback().
    if ctx
        .auth_config()
        .is_some_and(|c| c.uses_form_post_callback())
    {
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

    Ok(router)
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
    run_server_with(storage, config, None::<NoopExtend>, None, None).await
}

/// Convenience alias so callers can pass `None` without spelling out a
/// concrete `FnOnce` type for the `extend_router` parameter.
type NoopExtend = fn(Router<SharedContext>) -> Router<SharedContext>;

/// Callback that fires once `HubContext::new` finishes (samod repo
/// initialized, index loaded, initial filesystem sync complete) and
/// *before* the HTTP listener binds. Receives a clone of the Arc so
/// the caller can stash it elsewhere or spawn background tasks
/// against it.
///
/// Used by `quarto-preview` to drive Phase C engine-capture recording
/// from the q2 preview server's startup path without coupling
/// `quarto-hub` to the engine layer.
pub type OnReadyCallback =
    Box<dyn FnOnce(std::sync::Arc<crate::context::HubContext>) + Send + 'static>;

/// Callback that fires once for every file change observed by the
/// in-process file watcher, *after* the file's bytes have synced
/// into samod (`HubContext::sync_file` succeeded). Receives a clone
/// of the context and the project-relative path that changed (the
/// same path keying the IndexDocument's `files` and `captures`
/// maps).
///
/// Used by `quarto-preview` (Phase C.2) to drive staleness
/// recomputation against the capture sidecar without coupling
/// `quarto-hub` to the engine layer. `Fn` (not `FnOnce`) because the
/// callback fires once per change event; `Send + Sync` because the
/// watcher task may spawn handlers on other threads.
pub type OnFileChangedCallback = std::sync::Arc<
    dyn Fn(std::sync::Arc<crate::context::HubContext>, std::path::PathBuf) + Send + Sync + 'static,
>;

/// Run the hub server, optionally extending its router before serving.
///
/// Same lifecycle as [`run_server`] (signal handling, periodic sync,
/// file watcher, graceful shutdown), with the option for the caller to
/// transform the built router *after* `build_router` and *before*
/// `axum::serve`. This is the seam `quarto-preview` uses to layer its
/// SPA fallback (`router.fallback(spa_handler)`) on top of the hub's
/// API + ws routes.
///
/// `on_ready`, when provided, is invoked once with an `Arc<HubContext>`
/// after the context is constructed and its initial filesystem sync
/// has completed, but before the listener binds. The callback runs
/// synchronously on the calling task; if it needs to do work that
/// shouldn't block server startup (engine execution, large I/O), it
/// should `tokio::spawn` an internal task. Errors inside the callback
/// are the callback's responsibility — `run_server_with` does not
/// observe its return value.
///
/// The caller must set `HubConfig::register_root_ws` to `false` when
/// the extension claims `/`; otherwise axum will panic on the
/// duplicate route.
pub async fn run_server_with<F>(
    storage: StorageManager,
    config: HubConfig,
    extend_router: Option<F>,
    on_ready: Option<OnReadyCallback>,
    on_file_changed: Option<OnFileChangedCallback>,
) -> Result<()>
where
    F: FnOnce(Router<SharedContext>) -> Router<SharedContext> + Send,
{
    let addr = format!("{}:{}", config.host, config.port);
    let sync_interval = config.sync_interval_secs;
    let watch_enabled = config.watch_enabled;
    let watch_debounce_ms = config.watch_debounce_ms;
    let watch_filter = config.watch_filter;
    let watch_single_file = config.single_file.clone();
    let project_root = storage.project_root().map(|p| p.to_path_buf());
    let has_project = project_root.is_some();

    // HubContext::new is now async (initializes samod repo and performs initial sync)
    let ctx = Arc::new(HubContext::new(storage, config).await?);

    // Fire the on-ready callback (if any) after initial sync, before
    // binding the listener. Callback can clone the Arc and spawn
    // background tasks against it; we don't observe its return.
    if let Some(callback) = on_ready {
        callback(ctx.clone());
    }

    let ctx_for_sync = ctx.clone();
    let ctx_for_watch = ctx.clone();
    let ctx_for_shutdown = ctx.clone();

    // Build the hub router *before* state binding so extensions can
    // register routes that consume `State<SharedContext>`. After
    // extensions land we bind state and the router becomes
    // `Router<()>`, ready for axum::serve.
    let mut router = build_router_with_state(ctx.clone()).await?;
    if let Some(extend) = extend_router {
        router = extend(router);
    }
    let router = router.with_state(ctx);

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
        // bd-tnm3k: when single-file mode is set, the watcher needs
        // an absolute target path. The project_root is the file's
        // parent directory, so `project_root.join(rel)` is the file.
        let single_file_abs = watch_single_file.as_ref().map(|rel| project_root.join(rel));
        let watch_config = WatchConfig {
            debounce_ms: watch_debounce_ms,
            filter: watch_filter,
            single_file: single_file_abs,
        };
        match FileWatcher::new(&project_root, watch_config) {
            Ok(watcher) => {
                info!("Starting filesystem watcher");
                let on_change = on_file_changed.clone();
                Some(tokio::spawn(async move {
                    run_file_watcher(ctx_for_watch, watcher, shutdown_rx, on_change).await;
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
                if result.has_changes() || result.has_errors() {
                    debug!(
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
    on_file_changed: Option<OnFileChangedCallback>,
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
                                // Phase C.2 hook: fire post-sync callback so
                                // engine-aware consumers (quarto-preview) can
                                // recompute capture staleness. The callback
                                // takes the absolute path on disk; it owns
                                // the conversion to the project-relative key.
                                if let Some(callback) = on_file_changed.as_ref() {
                                    callback(ctx.clone(), path.clone());
                                }
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
            Vec::new(),
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
            Vec::new(),
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
            Vec::new(),
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
            Vec::new(),
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
    fn auth_callback_form_google_deserializes() {
        let form: AuthCallbackForm = serde_json::from_value(serde_json::json!({
            "credential": "eyJhbGciOiJSUzI1NiJ9.test",
            "g_csrf_token": "abc123"
        }))
        .unwrap();
        assert_eq!(form.credential, "eyJhbGciOiJSUzI1NiJ9.test");
        assert_eq!(form.g_csrf_token.as_deref(), Some("abc123"));
    }

    #[test]
    fn auth_callback_form_oidc_deserializes() {
        // `state` is an extra field serde ignores — the struct only holds fields
        // it actually uses. Verify credential and absent g_csrf_token.
        let form: AuthCallbackForm = serde_json::from_value(serde_json::json!({
            "credential": "eyJhbGciOiJSUzI1NiJ9.test",
            "state": "random-state-value"
        }))
        .unwrap();
        assert_eq!(form.credential, "eyJhbGciOiJSUzI1NiJ9.test");
        assert!(form.g_csrf_token.is_none());
    }

    // ── validate_callback_csrf ────────────────────────────────────

    fn google_csrf_form(token: &str) -> AuthCallbackForm {
        AuthCallbackForm {
            credential: "cred".to_string(),
            g_csrf_token: Some(token.to_string()),
        }
    }

    fn oidc_state_form(_state: &str) -> AuthCallbackForm {
        AuthCallbackForm {
            credential: "cred".to_string(),
            g_csrf_token: None,
        }
    }

    #[test]
    fn google_double_submit_accepts_matching_token() {
        let form = google_csrf_form("tok123");
        let headers = headers_with(&[("cookie", "g_csrf_token=tok123")]);
        assert!(validate_callback_csrf(
            &auth::CallbackCsrfMode::GoogleDoubleSubmit,
            &form,
            &headers
        ));
    }

    #[test]
    fn google_double_submit_rejects_mismatched_token() {
        let form = google_csrf_form("tok123");
        let headers = headers_with(&[("cookie", "g_csrf_token=other")]);
        assert!(!validate_callback_csrf(
            &auth::CallbackCsrfMode::GoogleDoubleSubmit,
            &form,
            &headers
        ));
    }

    #[test]
    fn google_double_submit_rejects_missing_form_token() {
        let form = AuthCallbackForm {
            credential: "cred".to_string(),
            g_csrf_token: None,
        };
        let headers = headers_with(&[("cookie", "g_csrf_token=tok123")]);
        assert!(!validate_callback_csrf(
            &auth::CallbackCsrfMode::GoogleDoubleSubmit,
            &form,
            &headers
        ));
    }

    #[test]
    fn google_double_submit_rejects_empty_form_token() {
        let form = google_csrf_form("");
        let headers = headers_with(&[("cookie", "g_csrf_token=tok123")]);
        assert!(!validate_callback_csrf(
            &auth::CallbackCsrfMode::GoogleDoubleSubmit,
            &form,
            &headers
        ));
    }

    #[test]
    fn google_double_submit_rejects_missing_cookie() {
        let form = google_csrf_form("tok123");
        let headers = HeaderMap::new();
        assert!(!validate_callback_csrf(
            &auth::CallbackCsrfMode::GoogleDoubleSubmit,
            &form,
            &headers
        ));
    }

    #[test]
    fn oidc_state_fails_safe_until_implemented() {
        // OidcState is a placeholder: the pre-flight endpoint and signed cookie
        // that would make this check stateful do not exist yet. Validate that
        // the callback always rejects rather than silently passing.
        let form = oidc_state_form("any-state");
        let headers = headers_with(&[("cookie", "oidc_state=any-state")]);
        assert!(!validate_callback_csrf(
            &auth::CallbackCsrfMode::OidcState {
                cookie_name: "oidc_state"
            },
            &form,
            &headers
        ));
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
