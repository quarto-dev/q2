//! `provide-hub` — connect to a hub session as a code-execution provider.
//!
//! Joins an existing hub project's automerge session (authenticating via the
//! Node auth bridge) and — for Phase 3 — lists the project's files, proving
//! the authenticated sync path. Execution-on-request lands in Phase 4.
//!
//! See `claude-notes/plans/2026-06-29-remote-execution-provider.md` (bd-sfet3264).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use quarto_hub_provider::{JoinConfig, NodeBridge, join_and_list_files};

/// Arguments for `q2 provide-hub`.
pub struct ProvideHubArgs {
    /// A quarto-hub share URL (`https://quarto-hub.com/#/share/<id>?…`) or a
    /// bare index-document id of the project to join.
    pub project: String,
    /// Hub websocket URL. Defaults to `$QUARTO_HUB_SERVER`, else the canonical
    /// hub.
    pub server: Option<String>,
}

const DEFAULT_SERVER_WS: &str = "wss://quarto-hub.com/ws";

pub fn execute(args: ProvideHubArgs) -> Result<()> {
    // A full multi-threaded runtime: the auth bridge's stdout reader and the
    // samod sync both run as background tasks.
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run(args))
}

/// Extract the index-document id from a quarto-hub share URL, or return the
/// input unchanged when it is already a bare id.
fn parse_index_doc_id(project: &str) -> String {
    if let Some(rest) = project.split("#/share/").nth(1) {
        // The id is up to the first query/separator character.
        let id = rest.split(['?', '&', '/']).next().unwrap_or(rest);
        return id.to_string();
    }
    project.to_string()
}

/// Resolve the hub websocket URL: explicit `--server`, else `$QUARTO_HUB_SERVER`,
/// else the canonical hub.
fn resolve_server_ws(arg: Option<String>) -> String {
    arg.or_else(|| std::env::var("QUARTO_HUB_SERVER").ok())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SERVER_WS.to_string())
}

async fn run(args: ProvideHubArgs) -> Result<()> {
    let index_doc_id = parse_index_doc_id(&args.project);
    let server_ws = resolve_server_ws(args.server);
    let server_ws_url = url::Url::parse(&server_ws)
        .with_context(|| format!("invalid hub server URL: {server_ws}"))?;

    eprintln!("Authenticating with the hub…");
    let bridge = NodeBridge::spawn().context("starting the auth bridge")?;

    eprintln!("Connecting to project {index_doc_id} at {server_ws_url}…");
    let files = join_and_list_files(
        JoinConfig {
            server_ws_url,
            index_doc_id,
            connect_timeout: Duration::from_secs(30),
        },
        Arc::new(bridge),
    )
    .await
    .context("joining the hub session")?;

    println!("Connected. {} file(s) in the project:", files.len());
    for file in &files {
        println!("  {file}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_share_url() {
        assert_eq!(
            parse_index_doc_id("https://quarto-hub.com/#/share/abc123?file=x.qmd&name=y"),
            "abc123"
        );
        assert_eq!(
            parse_index_doc_id("https://quarto-hub.com/#/share/abc123"),
            "abc123"
        );
    }

    #[test]
    fn passes_a_bare_id_through() {
        assert_eq!(parse_index_doc_id("abc123"), "abc123");
    }

    #[test]
    fn server_resolution_prefers_the_explicit_arg() {
        assert_eq!(
            resolve_server_ws(Some("wss://example.test/ws".into())),
            "wss://example.test/ws"
        );
        // Blank explicit arg falls through to the default.
        assert_eq!(resolve_server_ws(Some("   ".into())), DEFAULT_SERVER_WS);
    }
}
