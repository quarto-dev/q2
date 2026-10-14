# tree-sitter-lmd grammar

Fork of [tree-sitter-grammars/tree-sitter-lua](https://github.com/tree-sitter-grammars/tree-sitter-lua)
at **v0.5.0** (MIT, see `LICENSE.md`), renamed to `lmd` and extended with one
rule, `markdown_fragment` (`{=` raw markdown `=}`), plus the three external
scanner tokens that delimit it.

Everything else in `grammar.js` and `src/scanner.c` is upstream. Keep the
diff small: search for `lmd` in both files to find the extension points.

Only what the Rust crate and `tree-sitter test` need is vendored:
`grammar.js`, `src/`, `test/corpus/`, `queries/`, `tree-sitter.json`. The
upstream language bindings, highlight/tag tests, and build files are not.

Regenerate and test with:

```sh
cargo xtask ts-test lmd
```
