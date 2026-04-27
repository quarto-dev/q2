# user-grammar-toml — test fixture

A pre-built tree-sitter-toml grammar + matching `highlights.scm`,
vendored here as a **test fixture** for `quarto-highlight`'s user-grammar
loader (`tree_sitter::WasmStore` path). TOML is deliberately NOT in the
built-in language registry so this test unambiguously exercises the
"new language loaded at runtime" code path.

## Provenance

- **Grammar**: [tree-sitter-grammars/tree-sitter-toml](https://github.com/tree-sitter-grammars/tree-sitter-toml)
- **Tag**: v0.7.0
- **Commit**: `64b56832c2cffe41758f28e05c756a3a98d16f41`
- **License**: MIT
- `toml.wasm` — downloaded from the v0.7.0 release asset
  (`tree-sitter-toml.wasm`, 24 040 bytes; renamed to `toml.wasm` for the
  loader's directory-convention matching).
- `highlights.scm` — copied verbatim from `queries/highlights.scm` at
  the same commit.

## Refreshing

```sh
curl -fsL 'https://github.com/tree-sitter-grammars/tree-sitter-toml/releases/download/v0.7.0/tree-sitter-toml.wasm' -o toml.wasm
curl -fsL 'https://raw.githubusercontent.com/tree-sitter-grammars/tree-sitter-toml/64b56832c2cffe41758f28e05c756a3a98d16f41/queries/highlights.scm' -o highlights.scm
```

If refreshing to a newer tag, also update the commit SHA above.
