//! LSP server command implementation.

use anyhow::Result;

/// Execute the LSP server.
///
/// This starts the Quarto Language Server Protocol server,
/// communicating over stdio with JSON-RPC messages.
pub fn execute() -> Result<()> {
    quarto_lsp::run_server()
}
