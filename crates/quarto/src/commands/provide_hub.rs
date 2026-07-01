//! `provide-hub` — connect to a hub session as a code-execution provider.
//!
//! Joins an existing hub project's automerge session (authenticating via the
//! Node auth bridge), lists the files, and — with `--allow-all` (Phase 4a) —
//! serves execution requests: it materializes the project, runs the engines
//! natively, and writes the results back as capture docs every collaborator's
//! editor consumes.
//!
//! Execution is **fail-closed** by default: without `--allow-all` the command
//! connects, lists files, and exits. The provider-only default (gate on the
//! provider's own actor id) lands in Phase 5.
//!
//! See `claude-notes/plans/2026-06-29-remote-execution-provider.md` (bd-sfet3264).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use quarto_hub_provider::{
    AuthzPolicy, JoinConfig, NodeBridge, Provider, StaticTokenSource, TokenSource, join,
};

/// Arguments for `q2 provide-hub`.
pub struct ProvideHubArgs {
    /// A quarto-hub share URL (`https://quarto-hub.com/#/share/<id>?…`) or a
    /// bare index-document id of the project to join.
    pub project: String,
    /// Hub websocket URL. Defaults to `$QUARTO_HUB_SERVER`, else the canonical
    /// hub.
    pub server: Option<String>,
    /// Serve execution requests from any collaborator. Without this the command
    /// is fail-closed (connect + list + exit).
    pub allow_all: bool,
    /// Dev/testing escape hatch: use this bearer token verbatim instead of
    /// running the interactive OAuth bridge. Intended for a **local, no-auth
    /// hub** (`q2 hub`), which ignores the bearer entirely. Never needed
    /// against quarto-hub.com.
    pub token: Option<String>,
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

    // A dev `--token` bypasses the OAuth bridge with a static bearer — for a
    // local no-auth hub, which ignores it. Otherwise spawn the Node auth
    // bridge and sign in interactively.
    let token_source: Arc<dyn TokenSource> = if let Some(token) = args.token {
        eprintln!("Using a static bearer token (dev mode; the hub must not require auth).");
        Arc::new(StaticTokenSource::new(token))
    } else {
        eprintln!("Authenticating with the hub…");
        Arc::new(NodeBridge::spawn().context("starting the auth bridge")?)
    };

    eprintln!("Connecting to project {index_doc_id} at {server_ws_url}…");
    let (repo, index) = join(
        JoinConfig {
            server_ws_url,
            index_doc_id,
            connect_timeout: Duration::from_secs(30),
        },
        token_source,
    )
    .await
    .context("joining the hub session")?;

    let mut files: Vec<String> = index.get_all_files().into_keys().collect();
    files.sort();
    println!("Connected. {} file(s) in the project:", files.len());
    for file in &files {
        println!("  {file}");
    }

    if !args.allow_all {
        eprintln!();
        eprintln!("Execution is DISABLED (fail-closed default).");
        eprintln!("Serving requests would run this project's code on THIS machine.");
        eprintln!("Re-run with --allow-all to let collaborators execute this project's");
        eprintln!("code here. (A safer provider-only default is coming in a later release.)");
        return Ok(());
    }

    // The beacon's actorId in Phase 4a is the samod peer id (stable for this
    // process); Phase 5 swaps in the per-project actor id from /auth/actor.
    let self_actor_id = repo.peer_id().to_string();
    let provider = Provider::new(repo, index, self_actor_id, AuthzPolicy::AllowAll, None);

    eprintln!();
    eprintln!("Execution ENABLED for all collaborators (--allow-all).");
    eprintln!("This project's code will run on THIS machine on request. Press Ctrl-C to stop.");
    provider
        .run(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    eprintln!("Provider stopped.");
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
