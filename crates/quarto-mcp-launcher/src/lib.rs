//! `q2 mcp` launcher: embeds the bundled TypeScript hub MCP server
//! (`ts-packages/quarto-hub-mcp/dist-bundle/`, built by
//! `cargo xtask build-hub-mcp-bundle`), extracts it to a per-user
//! cache, finds an ambient Node.js, and delegates execution.
//!
//! The TS server is the canonical MCP implementation (auth, sync, tool
//! surface); this crate is deliberately a *thin* launcher so all
//! protocol and security behavior stays single-sourced. Design:
//! claude-notes/plans/2026-06-11-q2-mcp-hub-auth.md (bd-81cfshmw).
//!
//! Launcher contract:
//! - never writes to stdout (it belongs to the MCP protocol), except
//!   for the explicit, pre-protocol `--launcher-info` query;
//! - all arguments pass through to the TS server verbatim (its
//!   `--help` is the user-facing usage);
//! - `QUARTO_NODE` overrides node discovery; `QUARTO_MCP_CACHE_DIR`
//!   overrides the cache location (mainly for tests).

mod bundle;
mod cache;
mod defaults;
mod delegate;
mod node;

pub use bundle::{BundleFile, content_hash, embedded_files, is_placeholder};
pub use cache::{
    DEFAULT_MAX_AGE, ExtractedBundle, LAST_USED_FILE, LOCK_FILE, default_cache_root,
    extract_and_lock, gc,
};
pub use defaults::{BundledDefault, Source, bundled_defaults, classify, injections, sources};
pub use node::{Discovery, MIN_NODE_MAJOR, NodeError, NodeInfo, find_node, parse_version};

use anyhow::{Result, bail};
use std::path::PathBuf;

/// What `run` should do with the args, decided up front by
/// [`classify_args`] before any I/O. `q2 mcp` is a verbatim passthrough
/// to the embedded TS server; this enumerates the few launcher-level
/// flags we intercept before delegating.
#[derive(Debug, PartialEq, Eq)]
pub enum LauncherAction {
    /// `--launcher-info` (sole arg): print embed/cache/node diagnostics
    /// and exit.
    LauncherInfo,
    /// `--print-config` (sole arg): print a ready-to-paste `.mcp.json`
    /// entry and exit.
    PrintConfig,
    /// `--help`/`-h` (anywhere): print the launcher-options preamble,
    /// then delegate `--help` to the server so its own usage shows too.
    HelpPreambleThenDelegate,
    /// Everything else: pass through to the server verbatim.
    Delegate,
}

/// Decide how to handle `q2 mcp` args. Pure (no I/O) so the
/// interception rules are unit-testable.
///
/// Precedence: `--help`/`-h` anywhere wins (so `q2 mcp --server x
/// --help` still helps); otherwise the sole-arg launcher queries
/// (`--launcher-info`, `--print-config`) match only when they are the
/// single argument — combined with anything else they flow through to
/// the server (which rejects the unknown flag). They are always invoked
/// alone in practice.
pub fn classify_args(args: &[String]) -> LauncherAction {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return LauncherAction::HelpPreambleThenDelegate;
    }
    if args.len() == 1 {
        match args[0].as_str() {
            "--launcher-info" => return LauncherAction::LauncherInfo,
            "--print-config" => return LauncherAction::PrintConfig,
            _ => {}
        }
    }
    LauncherAction::Delegate
}

/// A ready-to-paste `.mcp.json` entry that drives this binary as the
/// MCP server. Pure JSON on stdout so it pipes
/// (`q2 mcp --print-config > .mcp.json`). For a non-canonical hub,
/// append `"--server", "wss://your-hub/ws"` to `args`.
pub fn config_snippet() -> String {
    // A fixed literal (not serialized) so the formatting is stable and
    // matches the repo's own .mcp.json exactly.
    "{\n  \"mcpServers\": {\n    \"quarto-hub\": {\n      \"command\": \"q2\",\n      \"args\": [\"mcp\"]\n    }\n  }\n}\n"
        .to_string()
}

/// The launcher-options section printed ahead of the server's own
/// `--help`. These controls live in the launcher and never reach the TS
/// server, so they are invisible in its usage block — this preamble is
/// where they are discoverable.
pub fn help_preamble() -> String {
    "\
q2 mcp — launch the Quarto Hub MCP server (embedded; needs Node.js).

Launcher options (handled by q2 before the server starts):
  --launcher-info       Print embed/cache/node diagnostics and exit
  --print-config        Print a .mcp.json entry for this server and exit
  --help, -h            Show this help (launcher options, then server options)

Launcher environment variables:
  QUARTO_NODE           Path to the Node.js binary (when PATH discovery fails)
  QUARTO_MCP_CACHE_DIR  Override the extracted-bundle cache location

Embedded MCP server options:
"
    .to_string()
}

/// Entry point for `q2 mcp [args…]`. Returns the exit code to use
/// (Windows delegation path); on Unix a successful delegation never
/// returns.
pub fn run(args: &[String]) -> Result<i32> {
    // stdout in the next two branches is correct: these are explicit,
    // pre-protocol queries, not a live MCP session (where stdout carries
    // JSON-RPC). The help preamble is printed before delegation below.
    match classify_args(args) {
        LauncherAction::LauncherInfo => {
            println!("{}", launcher_info()?);
            return Ok(0);
        }
        LauncherAction::PrintConfig => {
            print!("{}", config_snippet());
            return Ok(0);
        }
        LauncherAction::HelpPreambleThenDelegate => {
            // Launcher half first; the server's own `--help` follows via
            // delegation (args still carry --help/-h). On Unix the exec
            // below replaces this process, so this stdout flushes first.
            print!("{}", help_preamble());
        }
        LauncherAction::Delegate => {}
    }

    if bundle::is_placeholder() {
        bail!(
            "this q2 binary was built without the hub MCP bundle.\n\
             Run `cargo xtask build-hub-mcp-bundle`, then rebuild the q2 binary\n\
             (`cargo build --bin q2`). See CLAUDE.md § hub MCP bundle."
        );
    }

    let files = bundle::embedded_files();
    let hash = bundle::content_hash(&files);
    let cache_root = cache_root()?;

    let extracted = cache::extract_and_lock(&cache_root, &files, &hash)?;
    // Opportunistic, best-effort; must never break the launch.
    cache::gc(&cache_root, &hash, cache::DEFAULT_MAX_AGE);

    let node = node::find_node(&node::Discovery::from_env())?;
    // Bundled quarto-hub.com defaults (release builds only): injected
    // into the child env for any hub variable the user hasn't set.
    let hub_defaults = defaults::bundled_defaults();
    let extra_env = defaults::injections(&hub_defaults, |var| std::env::var(var).ok());
    delegate::delegate(
        &node.path,
        &extracted.dir.join("index.mjs"),
        args,
        &extra_env,
        extracted.lock,
    )
}

fn cache_root() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("QUARTO_MCP_CACHE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    cache::default_cache_root()
}

/// Diagnostic blob for `q2 mcp --launcher-info`: which bundle is
/// embedded (the stale-embed tripwire — see CLAUDE.md), where it
/// extracts, and which node would run it.
fn launcher_info() -> Result<String> {
    let mut out = String::new();
    // Hub-connection variable sources (env / bundled / absent). Values
    // are never printed: the source is the diagnostic, and the release
    // workflow asserts `: bundled` on all three. Reported even for
    // placeholder builds — the defaults are a property of the binary,
    // not of the embedded bundle.
    let hub_defaults = defaults::bundled_defaults();
    for (var, source) in defaults::sources(&hub_defaults, |v| std::env::var(v).ok()) {
        out.push_str(&format!("default {var}: {source}\n"));
    }
    if bundle::is_placeholder() {
        out.push_str("bundle: PLACEHOLDER (run `cargo xtask build-hub-mcp-bundle` and rebuild)\n");
        return Ok(out);
    }
    let files = bundle::embedded_files();
    out.push_str(&format!("bundle-hash: {}\n", bundle::content_hash(&files)));
    out.push_str(&format!("bundle-files: {}\n", files.len()));
    if let Some(info) = bundle::build_info_json() {
        out.push_str("build-info: ");
        out.push_str(info.trim());
        out.push('\n');
    }
    out.push_str(&format!("cache-root: {}\n", cache_root()?.display()));
    match node::find_node(&node::Discovery::from_env()) {
        Ok(node) => out.push_str(&format!(
            "node: {} (v{}.{}.{})\n",
            node.path.display(),
            node.version.0,
            node.version.1,
            node.version.2
        )),
        Err(e) => out.push_str(&format!("node: NOT FOUND — {}\n", e)),
    }
    Ok(out)
}
