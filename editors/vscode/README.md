# Quarto 2 for VS Code

Language support for Quarto 2 (`q2`) documents, served by `q2 lsp`:

- **Semantic highlighting** from Quarto 2's own highlighter (the same one
  the hub editor and `q2 render` use), including code cells. Colours follow
  your VS Code theme.
- **Diagnostics** for parse errors and YAML validation.
- **Outline** of headings and code cells.
- **Preview** (the preview icon in the editor title, or Cmd/Ctrl+Shift+K):
  renders the document with `q2 preview --static` inside `q2 lsp` and
  shows it beside the editor. Saving re-renders and the page reloads.
  Documents of one project share a preview. Closing the panel stops it.
  The status bar shows render progress and errors (details in the
  Quarto 2 output).

The extension registers the language `quarto2` for `.qmd` files. If the
Quarto 1 extension (`quarto.quarto`) is also installed, choose per
workspace with `files.associations`:

```json
{ "files.associations": { "*.qmd": "quarto2" } }
```

## Requirements

A `q2` binary. The extension looks at the `q2.path` setting, then `q2` on
`PATH`, then `~/.local/bin` (where `install.sh` / `install.ps1` put it).

## Settings

- `q2.path`: path to `q2`. Relative paths resolve against the first
  workspace folder; `~` expands.
- `q2.preview.target`: `panel` (default) or `browser`.
- `q2.trace.server`: `off` / `messages` / `verbose` LSP tracing.

## Commands

- **Quarto 2: Preview**
- **Quarto 2: Restart Language Server**
- **Quarto 2: Show Output**

## Development

From the repository root:

```bash
npm install                          # the extension is an npm workspace member
cargo build -p quarto                # target/debug/q2
npm run build -w quarto2-vscode      # dist/extension.js
npm test -w quarto2-vscode           # unit tests (vitest)
```

Open `editors/vscode/` in VS Code and press F5 ("Run Quarto 2 extension").
The dev host opens `test-workspace/`, whose `.vscode/settings.json` points
`q2.path` at the repo's `target/debug/q2`, so build that first. To use another
`q2`, edit that file, or open Settings (Cmd/Ctrl+,), search for `q2.path`,
and set it under the Workspace tab.

Smoke test in a real VS Code (downloads one unless `VSCODE_EXECUTABLE` is
set):

```bash
cd editors/vscode
Q2_SMOKE_BINARY=../../target/debug/q2 npm run test:smoke
```

Package a `.vsix`: `npm run package -w quarto2-vscode`.

The custom semantic token types (`qmdHeading`, …) in `package.json` must
match the server's legend; `crates/quarto-lsp/src/semantic_tokens.rs` has a
test that reads this `package.json` and fails on drift.

Plan: `claude-notes/plans/2026-11-17-q2-vscode-extension.md`.
