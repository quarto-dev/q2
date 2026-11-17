---
title: 'Built-in highlight grammars: ini, toml (+conf), xml, dockerfile, powershell'
date: 2026-11-17
description: 'Adds five tree-sitter grammars to `quarto-highlight` as built-ins, closing the gaps the Posit Connect docs build reported, and moves the user-grammar worked example off TOML now that TOML is built in.'
status: in-progress
braid:
  strand: bd-z6xw3psd  # parent: bd-n7x2, syntax highlighting epic
  priority: P2
---

## Overview

The survey in
`claude-notes/research/2026-11-17-highlight-grammar-coverage-survey.md`
picked five grammars to bundle. Everything else waits for on-demand grammar
loading (bd-iow5o4x4).

| Class | Aliases | Crate | Language constant | Query |
|---|---|---|---|---|
| `ini` | | `tree-sitter-ini` 1.4.0 | `LANGUAGE` | `HIGHLIGHTS_QUERY` |
| `toml` | `conf` | `tree-sitter-toml-ng` 0.7.0 | `LANGUAGE` | `HIGHLIGHTS_QUERY` |
| `xml` | | `tree-sitter-xml` 0.7.0 | `LANGUAGE_XML` | `XML_HIGHLIGHT_QUERY` |
| `dockerfile` | | `tree-sitter-containerfile` 0.9.2 | `LANGUAGE` | `HIGHLIGHTS_QUERY` |
| `powershell` | `ps1` | `tree-sitter-powershell` 0.26.6 | `LANGUAGE` | `HIGHLIGHTS_QUERY` |

`conf` and `ps1` follow Pandoc: skylighting resolves both through file
globs (`*.conf` is in TOML's globs, `*.ps1` in PowerShell's), so Quarto 1
highlights them. Class matching is already case-insensitive, so
```` ```Dockerfile ```` resolves to `dockerfile`.

Powershell goes in its own commit so it is easy to drop if its size cost
is judged too high.

## Findings that shape the work

- **`@spell` co-captures.** The ini and containerfile queries write
  `(comment) @comment @spell`. That puts two captures over one node, and
  `flatten_spans` resolves equal-extent ties by keeping the later capture,
  so `spell` would win and comments would render uncolored. Neovim's
  `spell`/`nospell` captures are spell-check directives, not highlight
  classes, so `captures_from_tree` drops them. This also fixes user
  grammars that ship Neovim queries.
- **wasm32 headers.** containerfile and powershell compile against the
  `tree-sitter-language-wasm-shim` headers (through
  `DEP_TREE_SITTER_LANGUAGE_WASM_HEADERS`). containerfile needs `memchr`
  (already provided at link time by `wasm-c-shim`) declared in `string.h`,
  and `iswxdigit` in `wctype.h`. Both were added upstream in
  `tree-sitter-language` 0.1.9; cherry-pick them rather than re-sync the
  whole mirror.
- **TOML was the user-grammar example.** The unit fixtures, the golden
  snapshot, smoke fixture `03-user-grammar`, the docs page
  `docs/guides/authoring/code-highlighting.qmd` and
  `examples/highlighting/01-user-grammar` all use TOML as the language
  that "isn't built in". `toml_is_not_a_built_in` guards that assumption.
  Move them to `tree-sitter-properties` v0.3.0 (Java `.properties`,
  10 KB release asset, not in Pandoc's list). The equal-extent fixture
  stays on `toml.wasm`: it loads into its own `UserGrammars` and never
  consults the built-in registry, so only its README wording changes.
- **Tests that use these classes as "Pandoc knows it, q2 has no
  grammar".** `annotate.rs` (`marks_pandoc_known_language_without_grammar`),
  `quarto-core/src/pipeline.rs` (`test_render_wraps_code_blocks_like_quarto1`)
  and the pampa HTML writer test use `ini`, `conf` and `toml` for that
  role. Switch them to `apache` and `makefile`, which stay grammar-less.

## Checklist

### Phase 1: tests first

- [x] `tests/integration/all_languages.rs`: cases for `ini`, `toml`, `conf`,
      `xml`, `dockerfile`, `Dockerfile`.
- [x] `tests/fixtures/builtin-snippets.json`: one entry per new grammar (it
      drives the insta goldens and the hub-client vitest harness).
- [x] `captures.rs` unit test: a `@comment @spell` co-capture yields
      `comment`.
- [x] Smoke-all fixture `12-builtin-config-languages.qmd`.
- [x] Run, and confirm the failures are the expected ones.

### Phase 2: implementation

- [x] Fix the wasm shim headers (`memchr`, `iswxdigit`); README note.
- [x] Drop `spell`/`nospell` captures in `captures_from_tree`.
- [x] Grammar crates, `langs/<name>.rs`, `langs/mod.rs`, registry entries.
- [x] Move the "no grammar" tests to `apache`/`makefile`.
- [x] Move the user-grammar fixture, golden, smoke 03, docs example and
      `examples/` to `properties`.
- [x] Review and accept the new insta snapshots.

### Phase 3: powershell (separate commit)

- [x] Test cases, snippet, crate, module, registry, `ps1` alias.

### Phase 4: bookkeeping and verification

- [x] Docs built-in language table; smoke fixture `01` prose; smoke README.
- [x] Grammar crate scout table.
- [x] `cargo nextest run -p quarto-highlight -p quarto-core -p pampa -p quarto`.
- [x] `cargo xtask verify` (full, including the hub WASM build). Record the
      WASM size before and after.
- [x] End-to-end `q2 render` of a probe document, output inspected.
- [ ] PR, then watch CI.

## Results

### Verification (2026-11-17)

`cargo xtask verify` was green apart from two kinds of failure, both
resolved:

- Tests broken by this change and fixed in follow-up commits: the TS
  user-grammar tests still pointed at the removed toml fixture, and
  `shortcode_text_contexts::fenced_code_block_substitutes_meta_shortcode`
  matched raw HTML that is now split by `hl-*` spans. The TS mirror of
  the capture extraction (`ts-packages/preview-runtime/src/userGrammar/Highlight.ts`)
  also needed the `spell`/`nospell` drop.
- `docs_serve_cli::open_is_serve_and_deep_links_a_page` failed once with
  `ConnectionReset` and passed on rerun; it is unrelated and filed as
  bd-gr1v3kbh.

After the fixes: `cargo nextest run --workspace --no-fail-fast` had one
failure (the shortcode test, then fixed and rerun green), and
`npm run test:ci` in `hub-client` and `npm test -w ts-packages/preview-runtime`
passed. The wasm32 hub build linked all five grammars, and
`highlight.wasm.test.ts` (driven by `builtin-snippets.json`) passed against
it.

Windows cross-check: `cargo clippy --target x86_64-pc-windows-msvc` cannot
run from macOS for this crate, because every grammar crate (including the
existing json and lua ones) compiles C in its build script. The change adds
no platform-specific Rust.

### Hub WASM size

`wasm_quarto_hub_client_bg.wasm` after `wasm-opt -Oz`:

| Build | Bytes | gzip -9 |
|---|---|---|
| `main` (59be796b2) | 42,204,297 | 11,673,361 |
| ini, toml, xml, dockerfile | 42,376,836 (+172,539) | 11,717,644 (+44,283) |
| plus powershell | 43,867,013 (+1,490,177) | 11,825,276 (+107,632) |

PowerShell is about 90% of the added bytes.

### End-to-end render

`cargo run --bin q2 -- render probe.qmd` with `ini`, `conf`, `Dockerfile`,
`xml`, `powershell` and `apache` blocks. Output inspected; spans per block:

```text
ini        -> comment "; Connect config", punctuation-bracket, type "Server", property "Address", operator
conf       -> type "listen", operator, string "\"0.0.0.0:3939\""
Dockerfile -> comment "# base image", keyword "FROM", operator ":"
xml        -> tag "config", tag "item", property "key", string "a", markup "1", punctuation
powershell -> variable "$env:CONNECT_SERVER", string "\"https://connect.example.com\""
apache     -> (none; pre.sourceCode wrapper only, as before)
```
