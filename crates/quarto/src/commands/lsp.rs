//! LSP server command implementation.

use anyhow::Result;

/// Execute the LSP server.
///
/// This starts the Quarto Language Server Protocol server,
/// communicating over stdio with JSON-RPC messages.
pub fn execute() -> Result<()> {
    // Create a new tokio runtime for the LSP server
    let runtime = tokio::runtime::Runtime::new()?;

    // Run the LSP server
    runtime.block_on(async {
        quarto_lsp::run_server().await;
    });

    Ok(())
}
