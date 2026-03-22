//! OIDC authentication for quarto-hub.
//!
//! All auth code lives in this module. Authentication is optional — disabled
//! by default and enabled with `--oidc-client-id <ID>`.
//!
//! Uses OIDC ID tokens (JWTs) validated locally against the provider's cached
//! public keys via `axum-jwt-auth`. No per-connection HTTP call to the provider.
//!
//! The JWKS URL is discovered automatically from the issuer's
//! `/.well-known/openid-configuration` endpoint at startup.

use axum::http::StatusCode;
use axum_jwt_auth::RemoteJwksDecoder;
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, Validation};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Default image domain for Google profile pictures.
const DEFAULT_IMAGE_DOMAIN: &str = "lh3.googleusercontent.com";

/// Authentication configuration.
///
/// Construct via [`AuthConfig::new()`] which validates the issuer URL
/// and image domains at creation time.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub client_id: String,
    /// OIDC issuer URL, guaranteed to be a valid HTTPS URL.
    pub issuer: String,
    pub image_domains: Vec<String>,
    pub allowed_emails: Option<Vec<String>>,
    pub allowed_domains: Option<Vec<String>>,
}

impl AuthConfig {
    /// Create a new `AuthConfig`, validating the issuer URL and image domains.
    ///
    /// - `issuer` must be a well-formed HTTPS URL.
    /// - Each image domain must be a bare hostname (no scheme, no path).
    /// - If `image_domains` is empty, defaults to Google's profile picture CDN.
    pub fn new(
        client_id: String,
        issuer: String,
        image_domains: Vec<String>,
        allowed_emails: Option<Vec<String>>,
        allowed_domains: Option<Vec<String>>,
    ) -> Result<Self, String> {
        // Validate issuer is a well-formed HTTPS URL.
        let parsed = url::Url::parse(&issuer)
            .map_err(|e| format!("Malformed OIDC issuer URL '{issuer}': {e}"))?;
        if parsed.scheme() != "https" {
            return Err(format!(
                "OIDC issuer must use HTTPS, got '{}'",
                parsed.scheme()
            ));
        }

        // Apply default and validate image domains.
        let image_domains = if image_domains.is_empty() {
            vec![DEFAULT_IMAGE_DOMAIN.to_string()]
        } else {
            for domain in &image_domains {
                validate_image_domain(domain).map_err(|e| format!("Image domain: {e}"))?;
            }
            image_domains
        };

        Ok(Self {
            client_id,
            issuer,
            image_domains,
            allowed_emails,
            allowed_domains,
        })
    }

    /// Whether the configured issuer is Google (`https://accounts.google.com`).
    ///
    /// Used to gate Google-specific endpoints like `/auth/callback`.
    pub fn is_google_issuer(&self) -> bool {
        self.issuer.trim_end_matches('/') == "https://accounts.google.com"
    }

    /// Extract the CSP origin (`scheme://host[:port]`) from the validated issuer URL.
    ///
    /// Panics if the issuer is not a valid URL, which cannot happen if
    /// the config was constructed via [`AuthConfig::new()`].
    pub fn issuer_origin(&self) -> String {
        let url = url::Url::parse(&self.issuer).expect("issuer validated at construction");
        match url.port() {
            Some(port) => format!("https://{}:{}", url.host_str().unwrap_or(""), port),
            None => format!("https://{}", url.host_str().unwrap_or("")),
        }
    }
}

/// OIDC ID token claims.
///
/// Uses standard OIDC claim names defined in the
/// [OIDC Core spec](https://openid.net/specs/openid-connect-core-1_0.html#StandardClaims).
/// `email_verified` defaults to `false` via `#[serde(default)]` so providers
/// that omit the claim (e.g. Azure AD) deserialize safely rather than failing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcClaims {
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
///
/// **Important**: the `email_verified` claim is trusted as reported by the
/// OIDC provider. Some providers set it to `true` without rigorous
/// verification. When using `--allowed-domains`, ensure your provider
/// actually verifies email ownership before issuing tokens.
pub fn check_allowlists(claims: &OidcClaims, config: &AuthConfig) -> Result<(), StatusCode> {
    if !claims.email_verified {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let has_email_list = config.allowed_emails.is_some();
    let has_domain_list = config.allowed_domains.is_some();

    // No allowlists configured — all verified emails pass.
    if !has_email_list && !has_domain_list {
        return Ok(());
    }

    // Case-insensitive comparison: most OIDC providers normalize emails to
    // lowercase in ID token claims, but the allowlist may have mixed case.
    // Using eq_ignore_ascii_case handles providers that don't normalize.
    let email_ok = config
        .allowed_emails
        .as_ref()
        .is_some_and(|list| list.iter().any(|e| e.eq_ignore_ascii_case(&claims.email)));

    let domain_ok = config.allowed_domains.as_ref().is_some_and(|list| {
        let domain = claims.email.split('@').next_back().unwrap_or("");
        list.iter().any(|d| d.eq_ignore_ascii_case(domain))
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
    /// Cancellation token to stop the JWKS refresh task.
    _cancellation_token: CancellationToken,
}

impl std::fmt::Debug for AuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthState")
            .field("decoder", &"<RemoteJwksDecoder>")
            .finish()
    }
}

/// OIDC Discovery document (subset of fields we need).
#[derive(Deserialize)]
struct OidcDiscoveryDocument {
    issuer: String,
    jwks_uri: String,
}

/// Discover the JWKS URL from the issuer's `/.well-known/openid-configuration`.
///
/// The `issuer` must be a validated HTTPS URL (guaranteed by [`AuthConfig::new()`]).
///
/// Validates:
/// - The discovery document's `issuer` field matches the configured issuer
/// - The `jwks_uri` is an HTTPS URL
///
/// Returns the `jwks_uri` from the discovery document.
pub async fn discover_jwks_url(
    client: &reqwest::Client,
    issuer: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );

    let response = client.get(&discovery_url).send().await.map_err(|e| {
        format!("Failed to fetch OIDC discovery document from {discovery_url}: {e}")
    })?;

    if !response.status().is_success() {
        return Err(format!(
            "OIDC discovery endpoint returned HTTP {}: {discovery_url}",
            response.status()
        )
        .into());
    }

    let doc: OidcDiscoveryDocument = response.json().await.map_err(|e| {
        format!("Failed to parse OIDC discovery document from {discovery_url}: {e}")
    })?;

    validate_discovery_document(&doc, issuer, &discovery_url)
}

/// Validate an OIDC discovery document against the configured issuer.
///
/// - The document's `issuer` field must match the configured issuer (prevents spoofing).
/// - The `jwks_uri` must be a well-formed HTTPS URL.
///
/// Returns the `jwks_uri` on success.
fn validate_discovery_document(
    doc: &OidcDiscoveryDocument,
    configured_issuer: &str,
    discovery_url: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if doc.issuer.trim_end_matches('/') != configured_issuer.trim_end_matches('/') {
        return Err(format!(
            "OIDC issuer mismatch: configured '{}' but discovery document reports '{}'",
            configured_issuer, doc.issuer
        )
        .into());
    }

    let jwks_url = url::Url::parse(&doc.jwks_uri)
        .map_err(|e| format!("Malformed JWKS URI '{}': {e}", doc.jwks_uri))?;
    if jwks_url.scheme() != "https" {
        return Err(format!(
            "JWKS URI must use HTTPS, got '{}' from {}",
            jwks_url.scheme(),
            discovery_url
        )
        .into());
    }

    Ok(doc.jwks_uri.clone())
}

/// Convert a JWK key algorithm to a JWT signing algorithm.
///
/// Returns `None` for key encryption algorithms (RSA1_5, RSA-OAEP, etc.)
/// and unknown algorithms, which are not used for OIDC token signing.
fn signing_algorithm(ka: &jsonwebtoken::jwk::KeyAlgorithm) -> Option<Algorithm> {
    use jsonwebtoken::jwk::KeyAlgorithm;
    match ka {
        KeyAlgorithm::RS256 => Some(Algorithm::RS256),
        KeyAlgorithm::RS384 => Some(Algorithm::RS384),
        KeyAlgorithm::RS512 => Some(Algorithm::RS512),
        KeyAlgorithm::ES256 => Some(Algorithm::ES256),
        KeyAlgorithm::ES384 => Some(Algorithm::ES384),
        KeyAlgorithm::PS256 => Some(Algorithm::PS256),
        KeyAlgorithm::PS384 => Some(Algorithm::PS384),
        KeyAlgorithm::PS512 => Some(Algorithm::PS512),
        KeyAlgorithm::EdDSA => Some(Algorithm::EdDSA),
        _ => None,
    }
}

/// Discover allowed JWT signing algorithms from a JWKS endpoint.
///
/// Fetches the JWKS and extracts the `alg` field from each key.
/// If the resulting set is empty (all keys omit `alg`), falls back to
/// `[RS256]` — the most common OIDC signing algorithm.
async fn discover_algorithms(
    client: &reqwest::Client,
    jwks_url: &str,
) -> Result<Vec<Algorithm>, Box<dyn std::error::Error>> {
    let jwks: jsonwebtoken::jwk::JwkSet = client
        .get(jwks_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch JWKS from {jwks_url}: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse JWKS from {jwks_url}: {e}"))?;

    let algorithms = extract_algorithms_from_jwks(&jwks, jwks_url);
    Ok(algorithms)
}

/// Extract signing algorithms from a JWKS key set.
///
/// Returns a deduplicated list of signing algorithms found in the keys' `alg` fields.
/// Falls back to `[RS256]` if no keys declare a signing algorithm.
fn extract_algorithms_from_jwks(
    jwks: &jsonwebtoken::jwk::JwkSet,
    jwks_url: &str,
) -> Vec<Algorithm> {
    let mut algorithms = Vec::new();
    for jwk in &jwks.keys {
        if let Some(ref ka) = jwk.common.key_algorithm {
            if let Some(algo) = signing_algorithm(ka) {
                if !algorithms.contains(&algo) {
                    algorithms.push(algo);
                }
            }
        }
    }

    if algorithms.is_empty() {
        tracing::warn!("No 'alg' field found in any JWK from {jwks_url}; falling back to RS256");
        algorithms.push(Algorithm::RS256);
    } else {
        tracing::info!(
            algorithms = ?algorithms,
            "Discovered JWT signing algorithms from JWKS"
        );
    }

    algorithms
}

/// Build the JWKS decoder for OIDC ID token validation.
/// Returns an `AuthState` that owns both the decoder and the
/// background JWKS refresh task handle.
///
/// Discovers the JWKS URL and signing algorithms from the provider's
/// OIDC discovery endpoint, then initializes the decoder with provider-specific
/// validation settings.
pub async fn build_auth_state(
    config: &AuthConfig,
) -> std::result::Result<AuthState, Box<dyn std::error::Error>> {
    // Shared HTTP client for OIDC discovery requests.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    // Discover JWKS URL and fetch initial keys.
    let jwks_url = discover_jwks_url(&client, &config.issuer).await?;
    tracing::info!(jwks_url = %jwks_url, "Discovered JWKS URL from OIDC issuer");

    // Discover algorithms from the JWKS endpoint.
    let algorithms = discover_algorithms(&client, &jwks_url).await?;

    let mut validation = Validation::default();
    validation.algorithms = algorithms;
    validation.set_audience(&[&config.client_id]);
    validation.set_issuer(&[&config.issuer]);
    validation.validate_nbf = true;
    // leeway defaults to 60 seconds in jsonwebtoken, which is fine

    let decoder = RemoteJwksDecoder::builder()
        .jwks_url(jwks_url)
        .validation(validation)
        .build()?;

    // Fetch the initial JWKS keys before accepting requests.
    decoder.initialize().await?;

    // Spawn the periodic JWKS key refresh as a background task.
    // RemoteJwksDecoder is Clone — the spawned copy shares the
    // internal key cache with our copy.
    let cancellation_token = CancellationToken::new();
    let refresh_decoder = decoder.clone();
    let token = cancellation_token.clone();
    let refresh_handle = tokio::spawn(async move {
        refresh_decoder.refresh_keys_periodically(token).await;
    });

    Ok(AuthState {
        decoder,
        _refresh_handle: refresh_handle,
        _cancellation_token: cancellation_token,
    })
}

/// Validate that TLS is accounted for when auth is enabled.
/// Called once at startup before the server accepts requests.
///
/// Returns an error if auth is enabled without TLS protection.
/// Logs a warning if `--allow-insecure-auth` is used (local dev).
pub fn validate_tls_config(
    oidc_client_id: Option<&str>,
    behind_tls_proxy: bool,
    allow_insecure_auth: bool,
) -> std::result::Result<(), String> {
    if oidc_client_id.is_some() && !behind_tls_proxy && !allow_insecure_auth {
        return Err(
            "--oidc-client-id requires TLS to protect tokens in transit.\n\
             Use --behind-tls-proxy if a reverse proxy terminates TLS,\n\
             or --allow-insecure-auth for local development (never in production)."
                .to_string(),
        );
    }
    if allow_insecure_auth && oidc_client_id.is_some() {
        tracing::warn!(
            "Auth enabled WITHOUT TLS (--allow-insecure-auth). \
             Tokens will transit in plaintext. Do not use in production."
        );
    }
    Ok(())
}

/// Validate that an image domain is safe for CSP inclusion.
///
/// Accepts bare hostnames only (e.g. `lh3.googleusercontent.com`).
/// Rejects domains containing characters that could allow CSP injection.
pub fn validate_image_domain(domain: &str) -> Result<(), String> {
    if domain.is_empty() {
        return Err("Image domain must not be empty".to_string());
    }
    // Must be a bare hostname: alphanumeric, dots, hyphens only.
    // No scheme, no path, no whitespace, no semicolons.
    let valid = domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if !valid {
        return Err(format!(
            "Invalid image domain '{domain}': must contain only alphanumeric characters, dots, and hyphens"
        ));
    }
    Ok(())
}

/// Derive a per-project Automerge actor ID using HMAC-SHA256.
///
/// Uses `HMAC-SHA256(key=server_secret, message="{sub}\0{project_id}")`.
///
/// Properties:
/// - **Per-project isolation**: Same user gets a different actor ID in every
///   project. Cross-project correlation via actor_id is impossible.
/// - **Server-secret binding**: The actor_id cannot be computed outside the
///   server even if an attacker knows both `sub` and `project_id`.
/// - **Per-session consistency**: Within a single project, the same user gets
///   the same actor_id across sessions/devices/reconnections.
///
/// The null byte separator (`\0`) cannot appear in JWT `sub` claims (which are
/// JSON strings) or Automerge IDs (`automerge:<bs58>`), preventing separator
/// injection attacks.
pub fn sub_to_actor_id_for_project(server_secret: &[u8], sub: &str, project_id: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(server_secret).expect("HMAC accepts keys of any length");
    mac.update(sub.as_bytes());
    mac.update(b"\0");
    mac.update(project_id.as_bytes());
    let result = mac.finalize();
    format!("{:x}", result.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_claims(email: &str, verified: bool) -> OidcClaims {
        OidcClaims {
            sub: "123".to_string(),
            email: email.to_string(),
            email_verified: verified,
            name: Some("Test User".to_string()),
            picture: None,
        }
    }

    fn make_config(emails: Option<Vec<&str>>, domains: Option<Vec<&str>>) -> AuthConfig {
        AuthConfig::new(
            "test-client-id".to_string(),
            "https://accounts.google.com".to_string(),
            vec!["lh3.googleusercontent.com".to_string()],
            emails.map(|v| v.into_iter().map(String::from).collect()),
            domains.map(|v| v.into_iter().map(String::from).collect()),
        )
        .unwrap()
    }

    #[test]
    fn unverified_email_returns_unauthorized() {
        let claims = make_claims("user@example.com", false);
        let config = make_config(None, None);
        assert_eq!(
            check_allowlists(&claims, &config),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn no_allowlists_allows_all_verified() {
        let claims = make_claims("user@example.com", true);
        let config = make_config(None, None);
        assert_eq!(check_allowlists(&claims, &config), Ok(()));
    }

    #[test]
    fn email_allowlist_match() {
        let claims = make_claims("admin@example.com", true);
        let config = make_config(Some(vec!["admin@example.com"]), None);
        assert_eq!(check_allowlists(&claims, &config), Ok(()));
    }

    #[test]
    fn email_allowlist_no_match() {
        let claims = make_claims("other@example.com", true);
        let config = make_config(Some(vec!["admin@example.com"]), None);
        assert_eq!(
            check_allowlists(&claims, &config),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn domain_allowlist_match() {
        let claims = make_claims("user@company.com", true);
        let config = make_config(None, Some(vec!["company.com"]));
        assert_eq!(check_allowlists(&claims, &config), Ok(()));
    }

    #[test]
    fn domain_allowlist_no_match() {
        let claims = make_claims("user@other.com", true);
        let config = make_config(None, Some(vec!["company.com"]));
        assert_eq!(
            check_allowlists(&claims, &config),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn combined_lists_or_logic_email_match() {
        let claims = make_claims("contractor@gmail.com", true);
        let config = make_config(
            Some(vec!["contractor@gmail.com"]),
            Some(vec!["company.com"]),
        );
        assert_eq!(check_allowlists(&claims, &config), Ok(()));
    }

    #[test]
    fn combined_lists_or_logic_domain_match() {
        let claims = make_claims("employee@company.com", true);
        let config = make_config(
            Some(vec!["contractor@gmail.com"]),
            Some(vec!["company.com"]),
        );
        assert_eq!(check_allowlists(&claims, &config), Ok(()));
    }

    #[test]
    fn combined_lists_or_logic_no_match() {
        let claims = make_claims("random@other.com", true);
        let config = make_config(
            Some(vec!["contractor@gmail.com"]),
            Some(vec!["company.com"]),
        );
        assert_eq!(
            check_allowlists(&claims, &config),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn email_allowlist_case_insensitive() {
        let claims = make_claims("Admin@Example.COM", true);
        let config = make_config(Some(vec!["admin@example.com"]), None);
        assert_eq!(check_allowlists(&claims, &config), Ok(()));
    }

    #[test]
    fn domain_allowlist_case_insensitive() {
        let claims = make_claims("user@Company.COM", true);
        let config = make_config(None, Some(vec!["company.com"]));
        assert_eq!(check_allowlists(&claims, &config), Ok(()));
    }

    // ── validate_tls_config ──────────────────────────────────────

    #[test]
    fn tls_required_when_auth_enabled() {
        let result = validate_tls_config(Some("client-id"), false, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("--behind-tls-proxy"));
    }

    #[test]
    fn tls_satisfied_by_proxy() {
        assert!(validate_tls_config(Some("client-id"), true, false).is_ok());
    }

    #[test]
    fn tls_satisfied_by_insecure_flag() {
        assert!(validate_tls_config(Some("client-id"), false, true).is_ok());
    }

    #[test]
    fn tls_both_flags_ok() {
        assert!(validate_tls_config(Some("client-id"), true, true).is_ok());
    }

    #[test]
    fn tls_not_required_when_auth_disabled() {
        assert!(validate_tls_config(None, false, false).is_ok());
    }

    // ── OidcClaims deserialization ─────────────────────────────

    #[test]
    fn oidc_claims_google_style() {
        let json = r#"{
            "sub": "1234567890",
            "email": "user@gmail.com",
            "email_verified": true,
            "name": "Test User",
            "picture": "https://lh3.googleusercontent.com/photo.jpg"
        }"#;
        let claims: OidcClaims = serde_json::from_str(json).unwrap();
        assert_eq!(claims.sub, "1234567890");
        assert_eq!(claims.email, "user@gmail.com");
        assert!(claims.email_verified);
        assert_eq!(claims.name.as_deref(), Some("Test User"));
        assert!(claims.picture.is_some());
    }

    #[test]
    fn oidc_claims_azure_style_no_picture_no_email_verified() {
        let json = r#"{
            "sub": "AAAAABBBBBcccccc",
            "email": "user@contoso.com",
            "name": "Contoso User"
        }"#;
        let claims: OidcClaims = serde_json::from_str(json).unwrap();
        assert_eq!(claims.sub, "AAAAABBBBBcccccc");
        assert_eq!(claims.email, "user@contoso.com");
        // email_verified defaults to false when absent
        assert!(!claims.email_verified);
        assert_eq!(claims.name.as_deref(), Some("Contoso User"));
        assert!(claims.picture.is_none());
    }

    #[test]
    fn oidc_claims_missing_email_verified_defaults_false_and_rejected() {
        let json = r#"{
            "sub": "xyz",
            "email": "user@example.com",
            "name": "User"
        }"#;
        let claims: OidcClaims = serde_json::from_str(json).unwrap();
        assert!(!claims.email_verified);

        // Should be rejected by check_allowlists
        let config = make_config(None, None);
        assert_eq!(
            check_allowlists(&claims, &config),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    // ── validate_image_domain ──────────────────────────────────

    #[test]
    fn image_domain_valid() {
        assert!(validate_image_domain("lh3.googleusercontent.com").is_ok());
        assert!(validate_image_domain("cdn.example.co.uk").is_ok());
        assert!(validate_image_domain("avatars.githubusercontent.com").is_ok());
    }

    #[test]
    fn image_domain_rejects_csp_injection() {
        assert!(validate_image_domain("evil.com; script-src 'unsafe-inline'").is_err());
    }

    #[test]
    fn image_domain_rejects_whitespace() {
        assert!(validate_image_domain("evil.com evil2.com").is_err());
    }

    #[test]
    fn image_domain_rejects_scheme_prefix() {
        assert!(validate_image_domain("https://example.com").is_err());
    }

    #[test]
    fn image_domain_rejects_empty() {
        assert!(validate_image_domain("").is_err());
    }

    // ── AuthConfig::new() validation ───────────────────────────

    #[test]
    fn auth_config_rejects_http_issuer() {
        let result = AuthConfig::new(
            "client-id".to_string(),
            "http://accounts.google.com".to_string(),
            vec![],
            None,
            None,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("OIDC issuer must use HTTPS"));
    }

    #[test]
    fn auth_config_rejects_malformed_issuer() {
        let result = AuthConfig::new(
            "client-id".to_string(),
            "not a url at all".to_string(),
            vec![],
            None,
            None,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Malformed"));
    }

    #[test]
    fn auth_config_defaults_image_domain_when_empty() {
        let config = AuthConfig::new(
            "client-id".to_string(),
            "https://accounts.google.com".to_string(),
            vec![],
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.image_domains, vec!["lh3.googleusercontent.com"]);
    }

    #[test]
    fn auth_config_rejects_invalid_image_domain() {
        let result = AuthConfig::new(
            "client-id".to_string(),
            "https://accounts.google.com".to_string(),
            vec!["evil.com; script-src 'unsafe-inline'".to_string()],
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn auth_config_issuer_origin() {
        let config = AuthConfig::new(
            "client-id".to_string(),
            "https://login.microsoftonline.com/tenant/v2.0".to_string(),
            vec![],
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.issuer_origin(), "https://login.microsoftonline.com");
    }

    #[test]
    fn auth_config_issuer_origin_with_port() {
        let config = AuthConfig::new(
            "client-id".to_string(),
            "https://auth.example.com:8443/realm".to_string(),
            vec![],
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.issuer_origin(), "https://auth.example.com:8443");
    }

    // ── is_google_issuer ──────────────────────────────────────────

    #[test]
    fn is_google_issuer_true() {
        let config = make_config(None, None);
        assert!(config.is_google_issuer());
    }

    #[test]
    fn is_google_issuer_with_trailing_slash() {
        let config = AuthConfig::new(
            "client-id".to_string(),
            "https://accounts.google.com/".to_string(),
            vec![],
            None,
            None,
        )
        .unwrap();
        assert!(config.is_google_issuer());
    }

    #[test]
    fn is_google_issuer_false_for_azure() {
        let config = AuthConfig::new(
            "client-id".to_string(),
            "https://login.microsoftonline.com/tenant/v2.0".to_string(),
            vec![],
            None,
            None,
        )
        .unwrap();
        assert!(!config.is_google_issuer());
    }

    // ── signing_algorithm ─────────────────────────────────────────

    #[test]
    fn signing_algorithm_maps_common_algorithms() {
        use jsonwebtoken::jwk::KeyAlgorithm;
        assert_eq!(
            signing_algorithm(&KeyAlgorithm::RS256),
            Some(Algorithm::RS256)
        );
        assert_eq!(
            signing_algorithm(&KeyAlgorithm::ES256),
            Some(Algorithm::ES256)
        );
        assert_eq!(
            signing_algorithm(&KeyAlgorithm::EdDSA),
            Some(Algorithm::EdDSA)
        );
    }

    #[test]
    fn signing_algorithm_rejects_encryption_algorithms() {
        use jsonwebtoken::jwk::KeyAlgorithm;
        // RSA1_5 and RSA-OAEP are key encryption algorithms, not signing.
        assert_eq!(signing_algorithm(&KeyAlgorithm::RSA1_5), None);
        assert_eq!(signing_algorithm(&KeyAlgorithm::RSA_OAEP), None);
    }

    // ── validate_discovery_document ───────────────────────────────

    fn make_discovery_doc(issuer: &str, jwks_uri: &str) -> OidcDiscoveryDocument {
        OidcDiscoveryDocument {
            issuer: issuer.to_string(),
            jwks_uri: jwks_uri.to_string(),
        }
    }

    #[test]
    fn discovery_doc_valid() {
        let doc = make_discovery_doc(
            "https://accounts.google.com",
            "https://www.googleapis.com/oauth2/v3/certs",
        );
        let result =
            validate_discovery_document(&doc, "https://accounts.google.com", "https://ignored");
        assert_eq!(
            result.unwrap(),
            "https://www.googleapis.com/oauth2/v3/certs"
        );
    }

    #[test]
    fn discovery_doc_issuer_mismatch() {
        let doc = make_discovery_doc("https://evil.com", "https://evil.com/.well-known/jwks.json");
        let result =
            validate_discovery_document(&doc, "https://accounts.google.com", "https://ignored");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("issuer mismatch"), "got: {err}");
        assert!(err.contains("evil.com"));
    }

    #[test]
    fn discovery_doc_issuer_trailing_slash_normalization() {
        let doc = make_discovery_doc(
            "https://accounts.google.com/",
            "https://www.googleapis.com/oauth2/v3/certs",
        );
        // Configured without trailing slash, doc has trailing slash — should still match.
        let result =
            validate_discovery_document(&doc, "https://accounts.google.com", "https://ignored");
        assert!(result.is_ok());
    }

    #[test]
    fn discovery_doc_rejects_http_jwks_uri() {
        let doc = make_discovery_doc(
            "https://accounts.google.com",
            "http://www.googleapis.com/oauth2/v3/certs",
        );
        let result =
            validate_discovery_document(&doc, "https://accounts.google.com", "https://ignored");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("JWKS URI must use HTTPS"), "got: {err}");
    }

    #[test]
    fn discovery_doc_rejects_malformed_jwks_uri() {
        let doc = make_discovery_doc("https://accounts.google.com", "not a url");
        let result =
            validate_discovery_document(&doc, "https://accounts.google.com", "https://ignored");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Malformed JWKS URI"), "got: {err}");
    }

    // ── extract_algorithms_from_jwks ──────────────────────────────

    #[test]
    fn extract_algorithms_finds_rs256() {
        let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_value(serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "alg": "RS256",
                "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
                "e": "AQAB",
                "use": "sig"
            }]
        }))
        .unwrap();
        let algos = extract_algorithms_from_jwks(&jwks, "https://example.com/jwks");
        assert_eq!(algos, vec![Algorithm::RS256]);
    }

    #[test]
    fn extract_algorithms_deduplicates() {
        let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_value(serde_json::json!({
            "keys": [
                {
                    "kty": "RSA",
                    "alg": "RS256",
                    "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
                    "e": "AQAB",
                    "use": "sig",
                    "kid": "key1"
                },
                {
                    "kty": "RSA",
                    "alg": "RS256",
                    "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
                    "e": "AQAB",
                    "use": "sig",
                    "kid": "key2"
                }
            ]
        }))
        .unwrap();
        let algos = extract_algorithms_from_jwks(&jwks, "https://example.com/jwks");
        assert_eq!(algos, vec![Algorithm::RS256]);
    }

    #[test]
    fn extract_algorithms_multiple_different() {
        let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_value(serde_json::json!({
            "keys": [
                {
                    "kty": "RSA",
                    "alg": "RS256",
                    "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
                    "e": "AQAB",
                    "use": "sig",
                    "kid": "rsa-key"
                },
                {
                    "kty": "EC",
                    "alg": "ES256",
                    "crv": "P-256",
                    "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
                    "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0",
                    "use": "sig",
                    "kid": "ec-key"
                }
            ]
        }))
        .unwrap();
        let algos = extract_algorithms_from_jwks(&jwks, "https://example.com/jwks");
        assert_eq!(algos, vec![Algorithm::RS256, Algorithm::ES256]);
    }

    #[test]
    fn extract_algorithms_falls_back_to_rs256_when_no_alg() {
        let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_value(serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
                "e": "AQAB",
                "use": "sig"
            }]
        }))
        .unwrap();
        let algos = extract_algorithms_from_jwks(&jwks, "https://example.com/jwks");
        assert_eq!(algos, vec![Algorithm::RS256]);
    }

    #[test]
    fn extract_algorithms_empty_keyset_falls_back_to_rs256() {
        let jwks: jsonwebtoken::jwk::JwkSet =
            serde_json::from_value(serde_json::json!({ "keys": [] })).unwrap();
        let algos = extract_algorithms_from_jwks(&jwks, "https://example.com/jwks");
        assert_eq!(algos, vec![Algorithm::RS256]);
    }

    // ── sub_to_actor_id_for_project ──────────────────────────────

    fn make_secret() -> [u8; 32] {
        [1u8; 32]
    }

    #[test]
    fn actor_id_for_project_is_64_hex_chars() {
        let id = sub_to_actor_id_for_project(&make_secret(), "user123", "automerge:abc");
        assert_eq!(id.len(), 64, "expected 64 hex chars, got {}", id.len());
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "non-hex char in id"
        );
    }

    #[test]
    fn actor_id_for_project_deterministic() {
        let secret = make_secret();
        let id1 = sub_to_actor_id_for_project(&secret, "user123", "automerge:abc");
        let id2 = sub_to_actor_id_for_project(&secret, "user123", "automerge:abc");
        assert_eq!(id1, id2);
    }

    #[test]
    fn actor_id_for_project_differs_across_projects() {
        let secret = make_secret();
        let id1 = sub_to_actor_id_for_project(&secret, "user123", "automerge:project1");
        let id2 = sub_to_actor_id_for_project(&secret, "user123", "automerge:project2");
        assert_ne!(id1, id2);
    }

    #[test]
    fn actor_id_for_project_differs_across_subs() {
        let secret = make_secret();
        let id1 = sub_to_actor_id_for_project(&secret, "user-one", "automerge:abc");
        let id2 = sub_to_actor_id_for_project(&secret, "user-two", "automerge:abc");
        assert_ne!(id1, id2);
    }

    #[test]
    fn actor_id_for_project_differs_across_secrets() {
        let id1 = sub_to_actor_id_for_project(&[1u8; 32], "user123", "automerge:abc");
        let id2 = sub_to_actor_id_for_project(&[2u8; 32], "user123", "automerge:abc");
        assert_ne!(id1, id2);
    }

    #[test]
    fn actor_id_for_project_no_separator_injection() {
        // Without a separator, plain concatenation of (sub, project_id) would be
        // ambiguous: "userproject" + "abc" == "user" + "projectabc".
        // With the \0 separator, these yield distinct HMAC messages:
        //   "userproject\0abc" != "user\0projectabc"
        // Because \0 cannot appear in JWT sub claims or Automerge IDs, the mapping
        // (sub, project_id) → HMAC-message is injective for all valid inputs.
        let secret = make_secret();
        let id1 = sub_to_actor_id_for_project(&secret, "userproject", "abc");
        let id2 = sub_to_actor_id_for_project(&secret, "user", "projectabc");
        assert_ne!(
            id1, id2,
            "\\0 separator must prevent ambiguity between \
             (sub='userproject', project='abc') and (sub='user', project='projectabc')"
        );
    }
}
