## Testing

tree-sitter-markdown-inline has a tree-sitter test suite that can be run with

```
$ tree-sitter test
```

Many tests there were inherited from the grammar we forked. Many of those fail, and some shouldn't actually pass.

In addition to a fixed test suite, we have `./tree-sitter-markdown-inline/scripts/shortcode_generator.py` to test the shortcode parsing subsystem specifically.
It uses random testing to generate large numbers of shortcodes, calls `tree-sitter parse` on them, and checks if the output matches expectations.

We use it to generate failing tests that are then fixed and added to the test suite (crates/tree-sitter-qmd/tree-sitter-markdown-inline/test/corpus/shortcodes.txt).
At present time, we have generated over 10k random tests without failures.
