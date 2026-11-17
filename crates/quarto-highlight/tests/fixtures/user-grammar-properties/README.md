# user-grammar-properties — test fixture

A pre-built tree-sitter-properties grammar (Java `.properties` files) plus
its matching `highlights.scm`, vendored here as a **test fixture** for
`quarto-highlight`'s user-grammar loader (the `tree_sitter::WasmStore`
path). `properties` is deliberately NOT in the built-in language registry,
so this test unambiguously exercises the "new language loaded at runtime"
code path. It is also not one of the syntaxes Pandoc knows, so it is not a
candidate for becoming a built-in.

This fixture used TOML until TOML became a built-in language
(bd-z6xw3psd).

## Provenance

- **Grammar**: [tree-sitter-grammars/tree-sitter-properties](https://github.com/tree-sitter-grammars/tree-sitter-properties)
- **Tag**: v0.3.0
- **Commit**: `579b62f5ad8d96c2bb331f07d1408c92767531d9`
- **License**: MIT
- `properties.wasm` — downloaded from the v0.3.0 release asset
  (`tree-sitter-properties.wasm`, 10 297 bytes; renamed to
  `properties.wasm` for the loader's directory-convention matching).
- `highlights.scm` — copied verbatim from `queries/highlights.scm` at
  the same commit.

## Refreshing

```sh
curl -fsL 'https://github.com/tree-sitter-grammars/tree-sitter-properties/releases/download/v0.3.0/tree-sitter-properties.wasm' -o properties.wasm
curl -fsL 'https://raw.githubusercontent.com/tree-sitter-grammars/tree-sitter-properties/579b62f5ad8d96c2bb331f07d1408c92767531d9/queries/highlights.scm' -o highlights.scm
```

If refreshing to a newer tag, also update the commit SHA above.
