# The Quarto 2 VS Code extension

`editors/vscode/` is a minimal VS Code extension (npm workspace
`quarto2-vscode`). It is a thin client: **all features come from `q2 lsp`**,
so other editors get them too.

| Feature | Server side | Extension side |
| --- | --- | --- |
| Semantic highlighting | `textDocument/semanticTokens/full` (`crates/quarto-lsp/src/semantic_tokens.rs`) | `contributes.semanticTokenTypes` / `semanticTokenScopes` |
| Diagnostics, outline | `publishDiagnostics`, `documentSymbol` | — (vscode-languageclient) |
| Preview | `quarto/previewStart` / `previewStop` / `previewStatus` (`crates/quarto-lsp/src/preview.rs`, launcher in `crates/quarto/src/commands/lsp.rs`) | `src/preview.ts`: button, webview panel, status bar |

Design and history: `claude-notes/plans/2026-11-17-q2-vscode-extension.md`
(epic bd-xnpyb2vv).

## Architecture

```text
VS Code ──stdio JSON-RPC──▶ q2 lsp ─┬─ Workspace (quarto-lsp-project): analysis, diagnostics, outline
  extension (quarto2)               ├─ semantic tokens (quarto-highlight spans → LSP legend)
                                    └─ PreviewRegistry ──▶ static-preview sessions (in-process, tokio)
                                                            └─ HTTP server ◀── iframe in a webview panel
```

- **Language id `quarto2`**, distinct from Quarto 1's `quarto`, so the two
  extensions can coexist. Pick per workspace with `files.associations`.
- **Highlighting follows the user's theme.** Embedded-code tokens use the
  standard LSP types. Markup tokens use custom `qmd*` types, which the
  extension maps to the TextMate scopes themes already style. A Rust test
  (`the_vscode_extension_declares_every_custom_token_type`) reads
  `editors/vscode/package.json` and fails if the two sides drift. When you
  add a token type, add it to both.
- **Previews run inside `q2 lsp`.** There is one session per project root,
  stopped on `previewStop`, when the panel closes, or when the server
  exits. Because renders happen in the LSP process, `q2 lsp` keeps the
  JSON-RPC stream on a private copy of stdout and points the process's
  stdout at stderr (`crates/quarto-lsp/src/stdio.rs`). Never write
  protocol-adjacent output to stdout; it would go to the log anyway.

## Develop

```bash
npm install                          # repo root
cargo build -p quarto                # target/debug/q2
npm run build -w quarto2-vscode
npm test -w quarto2-vscode           # vitest unit tests
```

- **Interactive.** Open `editors/vscode/` in VS Code and press F5. In the
  dev host, set `q2.path` to `<repo>/target/debug/q2`.
  `test-workspace/sample.qmd` exercises most token types.
- **Smoke test in a real VS Code.** This is not run in CI, because it needs
  a q2 build and a display.

  ```bash
  cd editors/vscode
  Q2_SMOKE_BINARY=../../target/debug/q2 \
    VSCODE_EXECUTABLE="/Applications/Visual Studio Code.app/Contents/MacOS/Code" \
    npm run test:smoke
  ```

  Without `VSCODE_EXECUTABLE`, `@vscode/test-electron` downloads a VS Code
  build into `.vscode-test/`.
- **Package.** Run `npm run package -w quarto2-vscode`. It writes
  `editors/vscode/quarto2-vscode-<version>.vsix`. Install it with
  `code --install-extension <file>`.

## CI

`.github/workflows/vscode-extension.yml` typechecks, unit-tests and packages
the extension. It uploads the `.vsix` as the `quarto2-vscode` artifact. It
runs on changes under `editors/vscode/` (and the root npm manifests).
`cargo xtask verify` step 6 runs the same typecheck, tests and build. The
server-side tests are in the Rust suites:

- `crates/quarto-lsp` unit and integration tests;
- `crates/quarto/tests/integration/lsp_preview.rs`, which drives the real
  binary and asserts that stdout stays clean JSON-RPC while a pre-render
  script prints to it.

There is no Marketplace or OpenVSX publishing yet.
