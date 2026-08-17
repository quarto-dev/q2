//! `provide-hub` — connect to a hub session as a code-execution provider.
//!
//! Joins an existing hub project's automerge session (authenticating via the
//! Node auth bridge) and runs the project's code on THIS machine. Every
//! execution is gated by an **interactive consent prompt** (bd-9lgiulr4) that
//! shows the operator the resolved document to be evaluated (the post-include,
//! pre-engine QMD — what the engine actually receives) before anything runs.
//!
//! Two modes:
//! - **One-shot (default):** connect, execute the single `--file` document once
//!   (after consent), push the capture back to every collaborator, and exit.
//!   No beacon, no request channel.
//! - **`--watch`:** stay online, broadcast the capability beacon, and serve
//!   `exec/request`s from the editor's "Run" button until Ctrl-C — each still
//!   consent-gated.
//!
//! `--dangerously-accept-requests` skips the prompt (unattended); a non-TTY
//! stdin fails safe (refuses to execute). See
//! `claude-notes/plans/2026-07-02-provide-hub-consent-gate.md`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use quarto_hub_provider::{
    AlwaysAccept, AlwaysReject, ConsentGate, ExecOutcome, InteractivePrompt, JoinConfig,
    NodeBridge, Provider, StaticTokenSource, TokenSource, join, stdin_is_terminal,
};

/// Arguments for `q2 provide-hub`.
pub struct ProvideHubArgs {
    /// A quarto-hub share URL (`https://quarto-hub.com/#/share/<id>?…`) or a
    /// bare index-document id of the project to join.
    pub project: String,
    /// The project-relative document to execute once. Required in one-shot
    /// (default) mode; ignored under `--watch`.
    pub file: Option<String>,
    /// Hub websocket URL. Defaults to `$QUARTO_HUB_SERVER`, else the canonical
    /// hub.
    pub server: Option<String>,
    /// Stay online and serve execution requests (the editor "Run" button)
    /// instead of the default one-shot run.
    pub watch: bool,
    /// Skip the interactive consent prompt and auto-accept every execution.
    /// Dangerous — unattended remote code execution.
    pub dangerously_accept_requests: bool,
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
    // bd-hxhnnlzs: kernels spawned for execution requests stay warm for
    // the provider's lifetime; the scope drops after `block_on` returns
    // (Ctrl-C resolves `run_watch` normally), shutting them all down.
    let _kernel_scope = quarto_core::engine::jupyter::kernel_scope();
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

/// How long one-shot waits for the hub to acknowledge the capture before
/// giving up (it may still be in flight).
const FLUSH_TIMEOUT: Duration = Duration::from_secs(15);

async fn run(args: ProvideHubArgs) -> Result<()> {
    let index_doc_id = parse_index_doc_id(&args.project);
    let server_ws = resolve_server_ws(args.server);
    let server_ws_url = url::Url::parse(&server_ws)
        .with_context(|| format!("invalid hub server URL: {server_ws}"))?;

    // Validate mode/args before connecting so mistakes fail fast.
    validate_mode(args.watch, &args.file)?;

    // Build the consent gate up front. `--dangerously-accept-requests` opts out
    // of the prompt entirely; otherwise we prompt interactively, and if there
    // is no terminal we fail safe (refuse to execute). The "accept all future"
    // option is offered only under --watch (where more than one run can occur).
    let consent = build_consent_gate(args.dangerously_accept_requests, args.watch);

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

    // The beacon's actorId is the samod peer id (stable for this process);
    // Phase 5 swaps in the per-project actor id from /auth/actor.
    let self_actor_id = repo.peer_id().to_string();
    let provider = Provider::new(repo, index, self_actor_id, consent, None);

    if args.watch {
        run_watch(&provider).await;
    } else {
        // Guaranteed present by the early validation above.
        let file = args.file.expect("one-shot requires --file");
        run_one_shot(&provider, &file).await?;
    }
    Ok(())
}

/// One-shot mode needs a `--file`; surface a clear error before connecting.
fn validate_mode(watch: bool, file: &Option<String>) -> Result<()> {
    if !watch && file.is_none() {
        bail!(
            "one-shot mode requires --file <path> (the document to execute once).\n\
             Pass --watch to instead stay online and serve the editor's Run button."
        );
    }
    Ok(())
}

/// The consent gate to use, decided purely from flags + TTY state (no I/O, so
/// it is unit-testable without touching stdin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateChoice {
    /// `--dangerously-accept-requests`: auto-accept, unattended.
    AutoAccept,
    /// Interactive requested but no terminal: refuse (fail safe).
    RefuseNoTty,
    /// Prompt the operator; `allow_accept_all` offers option 3 (watch only).
    Prompt { allow_accept_all: bool },
}

fn choose_gate(dangerously_accept: bool, is_tty: bool, watch: bool) -> GateChoice {
    if dangerously_accept {
        GateChoice::AutoAccept
    } else if !is_tty {
        GateChoice::RefuseNoTty
    } else {
        GateChoice::Prompt {
            allow_accept_all: watch,
        }
    }
}

/// Build the concrete consent gate, printing the mode-specific notice.
fn build_consent_gate(dangerously_accept: bool, watch: bool) -> Arc<dyn ConsentGate> {
    match choose_gate(dangerously_accept, stdin_is_terminal(), watch) {
        GateChoice::AutoAccept => {
            eprintln!(
                "WARNING: --dangerously-accept-requests is set. Code will run UNATTENDED on this \
                 machine with no per-request review."
            );
            Arc::new(AlwaysAccept)
        }
        GateChoice::RefuseNoTty => {
            eprintln!(
                "No interactive terminal detected; executions will be REFUSED (fail-safe).\n\
                 Pass --dangerously-accept-requests to run unattended in a trusted session."
            );
            Arc::new(AlwaysReject)
        }
        GateChoice::Prompt { allow_accept_all } => {
            Arc::new(InteractivePrompt::new(allow_accept_all))
        }
    }
}

/// One-shot: execute `file` once (consent-gated), push the result to the hub,
/// and exit.
async fn run_one_shot(provider: &Arc<Provider>, file: &str) -> Result<()> {
    eprintln!();
    eprintln!("One-shot: preparing to execute \"{file}\" once.");
    let outcome = provider
        .execute_once(file)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    match outcome {
        ExecOutcome::Executed(doc_id) => {
            eprintln!("Executed. Pushing the result to the hub…");
            provider.flush_to_hub(&doc_id, FLUSH_TIMEOUT).await;
            provider.stop().await;
            eprintln!("Done — collaborators will see the executed output.");
        }
        ExecOutcome::Rejected => {
            eprintln!("Execution declined; nothing was run.");
            provider.stop().await;
        }
    }
    Ok(())
}

/// Watch: stay online, broadcast the beacon, and serve consent-gated requests
/// until Ctrl-C.
async fn run_watch(provider: &Arc<Provider>) {
    eprintln!();
    eprintln!("Watching for execution requests. This project's code will run on THIS machine");
    eprintln!("after you accept each request. Press Ctrl-C to stop.");
    Arc::clone(provider)
        .run(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    eprintln!("Provider stopped.");
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

    #[test]
    fn one_shot_requires_a_file() {
        // Default (one-shot) mode without --file is an error.
        assert!(validate_mode(false, &None).is_err());
        // With --file it is fine.
        assert!(validate_mode(false, &Some("doc.qmd".into())).is_ok());
        // Watch mode does not need --file (the path comes from the request).
        assert!(validate_mode(true, &None).is_ok());
    }

    #[test]
    fn dangerously_accept_wins_regardless_of_tty_or_mode() {
        assert_eq!(choose_gate(true, true, false), GateChoice::AutoAccept);
        assert_eq!(choose_gate(true, false, true), GateChoice::AutoAccept);
    }

    #[test]
    fn no_tty_without_dangerous_refuses() {
        assert_eq!(choose_gate(false, false, false), GateChoice::RefuseNoTty);
        assert_eq!(choose_gate(false, false, true), GateChoice::RefuseNoTty);
    }

    #[test]
    fn interactive_offers_accept_all_only_under_watch() {
        assert_eq!(
            choose_gate(false, true, false),
            GateChoice::Prompt {
                allow_accept_all: false
            }
        );
        assert_eq!(
            choose_gate(false, true, true),
            GateChoice::Prompt {
                allow_accept_all: true
            }
        );
    }
}
