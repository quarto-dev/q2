# user-grammar fixture: Java properties

Vendored copy of the tree-sitter-properties grammar + its `highlights.scm`
query, kept alongside the `.qmd` so smoke-all can discover it as a
user-supplied grammar via
`CodeHighlightStage::load_user_grammars`'s scan of
`<project>/_quarto/grammars/`.

This mirrors the fixture at
`crates/quarto-highlight/tests/fixtures/user-grammar-properties/`. Both
copies should have identical content — the quarto-highlight one exercises
the library-level loader, this one exercises the CLI-level pipeline path.

This fixture used TOML until TOML became a built-in language
(bd-z6xw3psd).

## Source

- **Grammar**: [tree-sitter-grammars/tree-sitter-properties](https://github.com/tree-sitter-grammars/tree-sitter-properties)
- **Tag**: v0.3.0 (commit `579b62f5ad8d96c2bb331f07d1408c92767531d9`)
- **License**: MIT
- `properties.wasm` — 10 297 bytes, downloaded from the v0.3.0 release asset.
- `highlights.scm` — copied verbatim from `queries/highlights.scm` at
  the same commit.

If either file drifts from the quarto-highlight fixture, update both
together or add a CI check that diffs them.
