## Testing

The qmd grammar is a single unified grammar living in
`crates/tree-sitter-qmd/tree-sitter-markdown` (block structure and
inline content are parsed by the same grammar; there is no longer a
separate `tree-sitter-markdown-inline` directory). From that
directory, run its tree-sitter test suite with:

```
$ tree-sitter test
```

The test corpus lives under
`crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/`, including
shortcode coverage in `shortcode.txt` and `inline-shortcodes.txt`.

Many tests there were inherited from the grammar we forked. Some of
those fail, and some shouldn't actually pass.
