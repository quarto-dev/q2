//! OAuth2 authentication for the Quarto CLI.
//!
//! Uses `yup-oauth2` for the installed application flow (opens browser,
//! receives callback). By requesting `openid` scopes, the token response
//! includes an `id_token` field which is what the hub server validates.

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

    // id_token() returns Result<Option<String>, Error>.
    // Requires "openid" in SCOPES for Google to include the ID token.
    auth.id_token(SCOPES)
        .await
        .context("Failed to get ID token")?
        .ok_or_else(|| {
            anyhow::anyhow!("No ID token in response. Ensure 'openid' scope is granted.")
        })
}

pub fn clear_tokens() -> Result<()> {
    let path = token_cache_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Show authentication status.
pub fn status() {
    let cache = token_cache_path();
    let secret = client_secret_path();

    if secret.exists() {
        println!("Client secret: {}", secret.display());
    } else {
        println!("Client secret: not found (expected at {})", secret.display());
    }

    if cache.exists() {
        println!("Token cache:   {} (cached)", cache.display());
    } else {
        println!("Token cache:   not logged in");
    }
}
