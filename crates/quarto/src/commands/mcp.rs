//! `q2 mcp` — run the Quarto Hub MCP server.
//!
//! A thin launcher (bd-81cfshmw): the canonical MCP server is the
//! TypeScript `ts-packages/quarto-hub-mcp`, embedded in this binary as
//! a self-contained bundle. The launcher extracts it to a per-user
//! cache and delegates to an ambient Node.js, passing all arguments
//! through verbatim — see `quarto-mcp-launcher` for the mechanics
//! (locking, GC, node discovery) and
//! `claude-notes/plans/2026-06-11-q2-mcp-hub-auth.md` for the design.
//!
//! The launcher never writes to stdout: in an MCP session stdout
//! carries the JSON-RPC protocol. (`--launcher-info`, an explicit
//! pre-protocol query, is the one exception.)

use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    let code = quarto_mcp_launcher::run(args)?;
    // Unix delegation execs and never returns; this path is reached on
    // Windows (forwarding the child's exit code) and for
    // --launcher-info.
    std::process::exit(code);
}
