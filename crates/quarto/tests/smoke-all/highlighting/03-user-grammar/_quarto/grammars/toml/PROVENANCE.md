# user-grammar fixture: TOML

Vendored copy of the tree-sitter-toml grammar + its `highlights.scm`
query, kept alongside the `.qmd` so smoke-all can discover it as a
user-supplied grammar via
`CodeHighlightStage::load_user_grammars`'s scan of
`<project>/_quarto/grammars/`.

This mirrors the fixture at
`crates/quarto-highlight/tests/fixtures/user-grammar-toml/`. Both copies
should have identical content — the quarto-highlight one exercises the
library-level loader, this one exercises the CLI-level pipeline path.

## Source

- **Grammar**: [tree-sitter-grammars/tree-sitter-toml](https://github.com/tree-sitter-grammars/tree-sitter-toml)
- **Tag**: v0.7.0 (commit `64b56832c2cffe41758f28e05c756a3a98d16f41`)
- **License**: MIT
- `toml.wasm` — 24 040 bytes, downloaded from the v0.7.0 release asset.
- `highlights.scm` — copied verbatim from `queries/highlights.scm` at
  the same commit.

If either file drifts from the quarto-highlight fixture, update both
together or add a CI check that diffs them.
