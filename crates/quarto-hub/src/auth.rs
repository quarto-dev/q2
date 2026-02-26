//! Google OAuth2 authentication for quarto-hub.
//!
//! All auth code lives in this module. Authentication is optional — disabled
//! by default and enabled with `--google-client-id <ID>`.
//!
//! Uses Google ID tokens (JWTs) validated locally against Google's cached
//! public keys via `axum-jwt-auth`. No per-connection HTTP call to Google.

use axum::http::StatusCode;
use axum_jwt_auth::RemoteJwksDecoder;
use jsonwebtoken::{Algorithm, Validation};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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
pub fn check_allowlists(claims: &GoogleClaims, config: &AuthConfig) -> Result<(), StatusCode> {
    if !claims.email_verified {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let has_email_list = config.allowed_emails.is_some();
    let has_domain_list = config.allowed_domains.is_some();

    // No allowlists configured — all verified emails pass.
    if !has_email_list && !has_domain_list {
        return Ok(());
    }

    // Case-insensitive comparison: Google normalizes emails to lowercase
    // in ID token claims, but the allowlist may have mixed case. Using
    // eq_ignore_ascii_case is also forward-compatible with non-Google
    // identity providers that may not normalize.
    let email_ok = config
        .allowed_emails
        .as_ref()
        .is_some_and(|list| list.iter().any(|e| e.eq_ignore_ascii_case(&claims.email)));

    let domain_ok = config.allowed_domains.as_ref().is_some_and(|list| {
        let domain = claims.email.split('@').last().unwrap_or("");
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

/// Build the JWKS decoder for Google ID token validation.
/// Returns an `AuthState` that owns both the decoder and the
/// background JWKS refresh task handle.
pub async fn build_auth_state(
    client_id: &str,
) -> std::result::Result<AuthState, Box<dyn std::error::Error>> {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id]);
    validation.set_issuer(&["https://accounts.google.com"]);

    let decoder = RemoteJwksDecoder::builder()
        .jwks_url("https://www.googleapis.com/oauth2/v3/certs".to_string())
        .validation(validation)
        .build()?;

    // Fetch the initial JWKS keys from Google before accepting requests.
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
    google_client_id: Option<&str>,
    behind_tls_proxy: bool,
    allow_insecure_auth: bool,
) -> std::result::Result<(), String> {
    if google_client_id.is_some() && !behind_tls_proxy && !allow_insecure_auth {
        return Err(
            "--google-client-id requires TLS to protect tokens in transit.\n\
             Use --behind-tls-proxy if a reverse proxy terminates TLS,\n\
             or --allow-insecure-auth for local development (never in production)."
                .to_string(),
        );
    }
    if allow_insecure_auth && google_client_id.is_some() {
        tracing::warn!(
            "Auth enabled WITHOUT TLS (--allow-insecure-auth). \
             Tokens will transit in plaintext. Do not use in production."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_claims(email: &str, verified: bool) -> GoogleClaims {
        GoogleClaims {
            sub: "123".to_string(),
            email: email.to_string(),
            email_verified: verified,
            name: Some("Test User".to_string()),
            picture: None,
        }
    }

    fn make_config(emails: Option<Vec<&str>>, domains: Option<Vec<&str>>) -> AuthConfig {
        AuthConfig {
            client_id: "test-client-id".to_string(),
            allowed_emails: emails.map(|v| v.into_iter().map(String::from).collect()),
            allowed_domains: domains.map(|v| v.into_iter().map(String::from).collect()),
        }
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
}
