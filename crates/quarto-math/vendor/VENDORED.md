# Vendored mitex front end

Four crates copied from <https://github.com/mitex-rs/mitex> at commit
`985d8e7` (2026-07-07), license Apache-2.0 (the upstream `LICENSE` sits in each
crate directory). They are the TeX reader of `quarto-math`; see
`claude-notes/plans/2026-09-21-quarto-math-and-native-docx.md`, decisions 2
and 6, for why we vendor rather than depend on crates.io (stale at 0.2.4) or
git (we patch the parser).

| Crate | Upstream path | Role |
| --- | --- | --- |
| `mitex-glob` | `crates/mitex-glob` | glob matcher for environment argument patterns (itself a vendored `glob-match`, MIT, see its header) |
| `mitex-spec` | `crates/mitex-spec` | command-spec types (`CommandSpec`, `ArgShape`, …) |
| `mitex-lexer` | `crates/mitex-lexer` | logos lexer + `\newcommand` macro engine |
| `mitex-parser` | `crates/mitex-parser` | rowan CST builder |

Not vendored: `mitex` (the Typst converter; we write our own emitters),
`mitex-spec-gen` (the Typst/rkyv spec build step; replaced by the JSON dump in
`../spec/upstream/`), `mitex-cli`, `mitex-wasm`.

## Local patches (keep this list exhaustive)

1. **Manifests rewritten** (`Cargo.toml` in each crate): explicit
   `version = "0.2.7"`, `edition = "2021"`, `publish = false`, concrete
   dependency versions instead of `workspace = true`, no `[lints]` table
   (upstream's `missing_docs`/`uninlined_format_args` warnings would fail our
   `-D warnings` gate for nothing), benches and `divan` dropped.
2. **rkyv removed from `mitex-spec`**: the `rkyv`/`rkyv-validation` features,
   the `#[cfg_attr(feature = "rkyv", …)]` derives, the
   `to_bytes`/`from_bytes` impl block and `src/stream.rs`. `serde` stays (default
   feature) because the spec is loaded from JSON.
3. **Tests consolidated** into one `tests/integration/` binary per crate
   (`.claude/rules/integration-tests.md`): `mitex-parser`'s `tests/ast.rs` and
   `tests/properties.rs` became `tests/integration/{ast,properties}.rs` with
   their wrapper modules unwrapped; `mitex-lexer`'s `tests/expand_macro.rs`
   moved likewise. Test bodies are unchanged.
4. **`DEFAULT_SPEC` for tests** comes from
   `tests/integration/common/mod.rs` in each crate, a `once_cell::Lazy` that
   deserializes `../spec/upstream/mitex-default-spec.json`, instead of
   `mitex_spec_gen::DEFAULT_SPEC`.

5. **Span side table in `mitex-parser`** (`src/parser.rs`, `src/lib.rs`):
   `Parser` keeps the input and a `spans: Vec<Option<Range<usize>>>`; every
   `builder.token` site calls `record_span`, which recovers the token text's
   byte range from its address within the input (lexer texts are always
   slices of the input, macro bodies included). The two sites that used to
   emit a fresh one-character `String` (`single_char`, the word-splitting
   argument matcher) now emit subslices so they map too. New entry points
   `Parser::parse_with_spans` and `mitex_parser::parse_with_spans`;
   `parse` is unchanged in behavior and output.
6. **Macro expansion budget in `mitex-lexer`** (`src/macro_engine.rs`):
   `MacroEngine` counts the tokens it produces by expansion and, past
   `EXPANSION_BUDGET` (200 000), stops expanding and emits
   `(Token::Error, "macro expansion limit exceeded")` in place of the macro
   call. Upstream loops forever on `\newcommand{\loop}{\loop} \loop`; a
   reader fed `.qmd` content must terminate and diagnose instead.
7. **`\renewcommand` of a built-in** (`mitex-lexer/src/macro_engine.rs`):
   upstream emits an error token when `\renewcommand` names something that
   is not in its macro table, which rejects `\renewcommand{\vec}[1]{…}`.
   TeX allows redefining any existing command; the vendored engine just
   installs the new definition.

## Updating

Upstream velocity on these four crates is near zero (two commits since
2025-01). Quarterly:

```bash
cd external-sources/mitex && git fetch && \
  git log --oneline 985d8e7..origin/main -- crates/mitex-glob crates/mitex-spec crates/mitex-lexer crates/mitex-parser
```

If anything landed, diff `crates/<crate>/src` against `vendor/<crate>/src`,
re-apply the patches above, bump the commit in this file and in each
manifest header, re-run `cargo nextest run -p mitex-lexer -p mitex-parser`, and
re-dump the spec if `packages/mitex/specs` changed.
