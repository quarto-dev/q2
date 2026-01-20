# Quarto Rust LSP - VS Code Extension

A VS Code extension that provides language support for Quarto documents (`.qmd` files) using the Rust-based Quarto LSP server.

## Features

- **Diagnostics**: Real-time error and warning markers for parse errors and YAML validation
- **Document Symbols**: Outline view showing headers and code cells for easy navigation

## Prerequisites

1. **Rust Quarto binary**: Build the `quarto` binary from the kyoto repository:
   ```bash
   cd /path/to/kyoto
   cargo build --release -p quarto
   ```

2. **Node.js**: Required for building the extension (v18 or later recommended)

## Development Setup

1. Navigate to the extension directory and install dependencies:
   ```bash
   cd editors/vscode-quarto-rust
   npm install
   npm run compile
   ```

2. Open the extension folder in VS Code:
   ```bash
   code .
   ```

   **Important**: You must open `editors/vscode-quarto-rust/` as the workspace (not the root kyoto folder) for the debug configuration to work.

3. Launch the extension in development mode:
   - Press F5 (or select "Run Extension" from the Debug panel)
   - A new VS Code window (Extension Development Host) will open with the `test-workspace/` folder
   - The `test-workspace/` folder has preconfigured settings and a sample `test.qmd` file

4. **First time setup**: Update the Rust Quarto path in `test-workspace/.vscode/settings.json`:
   ```json
   {
     "quartoRustLsp.path": "/your/path/to/kyoto/target/debug/quarto"
   }
   ```

   This only needs to be done once. The setting is committed to the repo but gitignored paths may differ per developer.

## Configuration

### Settings

- **`quartoRustLsp.path`**: Path to the `quarto` binary. If not specified, the extension searches for `quarto` in PATH.

- **`quartoRustLsp.logLevel`**: Log level for the language server. Options: `trace`, `debug`, `info`, `warn`, `error`. Default: `warn`.

- **`quartoRustLsp.trace.server`**: Traces communication between VS Code and the language server. Options: `off`, `messages`, `verbose`. Default: `off`.

### Example settings.json

```json
{
  "quartoRustLsp.path": "/path/to/kyoto/target/release/quarto",
  "quartoRustLsp.logLevel": "info",
  "quartoRustLsp.trace.server": "messages"
}
```

## Commands

- **Quarto Rust LSP: Restart Server** (`quartoRustLsp.restartServer`): Restart the language server
- **Quarto Rust LSP: Show Output** (`quartoRustLsp.showOutput`): Show the extension's output channel

## Troubleshooting

### Language server doesn't start

1. Check that `quarto` is in your PATH or configure `quartoRustLsp.path`
2. Verify the binary supports the `lsp` subcommand: `quarto lsp --help`
3. Check the "Quarto Rust LSP" output channel for errors

### No diagnostics appearing

1. Ensure the file has a `.qmd` extension
2. Check that the language mode is set to "Quarto" (shown in the VS Code status bar)
3. Enable `quartoRustLsp.trace.server` to see LSP communication

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    VS Code Extension                         │
│   - Spawns `quarto lsp` via stdio                           │
│   - Uses vscode-languageclient for LSP communication        │
└─────────────────────────────────────────────────────────────┘
                            │
                       stdio (JSON-RPC)
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                       quarto lsp                             │
│   - tower-lsp server implementation                         │
│   - Wraps quarto-lsp-core for analysis                      │
└─────────────────────────────────────────────────────────────┘
```

## License

MIT
