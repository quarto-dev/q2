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
mod delegate;
mod node;

pub use cache::{
    DEFAULT_MAX_AGE, ExtractedBundle, LAST_USED_FILE, LOCK_FILE, extract_and_lock, gc,
};
pub use node::{Discovery, MIN_NODE_MAJOR, NodeError, NodeInfo, find_node, parse_version};

use anyhow::{Result, bail};
use std::path::PathBuf;

/// Entry point for `q2 mcp [args…]`. Returns the exit code to use
/// (Windows delegation path); on Unix a successful delegation never
/// returns.
pub fn run(args: &[String]) -> Result<i32> {
    if args.len() == 1 && args[0] == "--launcher-info" {
        // Human/tool-facing metadata query; this is not an MCP session,
        // so stdout is correct here.
        println!("{}", launcher_info()?);
        return Ok(0);
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
    delegate::delegate(
        &node.path,
        &extracted.dir.join("index.mjs"),
        args,
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
