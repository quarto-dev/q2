//! Auth command - manage authentication for hub access
//!
//! Provides login, logout, and status subcommands for Google OAuth2
//! authentication used when connecting to authenticated hub servers.

use anyhow::Result;

use crate::auth;

/// Execute the auth login command.
pub fn login() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let _token = auth::get_id_token().await?;
        println!("Authenticated successfully.");
        Ok(())
    })
}

/// Execute the auth logout command.
pub fn logout() -> Result<()> {
    auth::clear_tokens()?;
    println!("Logged out. Token cache cleared.");
    Ok(())
}

/// Execute the auth status command.
pub fn status() -> Result<()> {
    auth::status();
    Ok(())
}
