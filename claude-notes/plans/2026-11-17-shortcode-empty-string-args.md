---
title: "Shortcodes: accept empty quoted arguments"
date: 2026-11-17
description: Let `""` and `''` parse as empty-string shortcode arguments, positional and keyword, matching Quarto 1.
---

Strand: bd-shortcode-empty-string-arg-4m36n6ur

## Overview

The Posit Connect doc authors report that `{{< kbd x="" >}}` is a fatal
parse error in q2 0.43.0 ("unexpected character or token here" at the
closing quote), so the whole document is skipped. Quarto 1 passes an empty
string. Connect does not depend on this, but empty strings should be valid
shortcode arguments in general.

Root cause: in `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`,
`shortcode_string` is built only from `_commonmark_single_quote_string` /
`_commonmark_double_quote_string`, and both require at least one character
between the quotes. The attribute key/value rule already special-cases
`''` and `""`; the shortcode rule does not.

Downstream code is already ready for empty strings:

- `treesitter.rs` strips the quotes from the `shortcode_string` node text,
  which yields `""` for a two-character node.
- The qmd writer (`shortcode_string_needs_quoting`) already writes an empty
  `ShortcodeArg::String` as `""`, so round-tripping works once the parser
  accepts it.

## Work items

- [x] tree-sitter corpus tests: positional `""` and `''`, keyword `x=""`
      and `x=''`, both in plain and escaped (`{{{< >}}}`) shortcodes.
- [x] pampa tests: the parsed AST carries `ShortcodeArg::String("")`
      (positional and keyword), and qmd writer round-trips them.
- [x] Grammar: add `"''"` and `'""'` alternatives to `shortcode_string`;
      regenerate with `cargo xtask ts-test`.
- [x] Regenerate the error table (`crates/pampa/scripts/build_error_table.ts`):
      the grammar change renumbers LR states, and two `meta.rs` tests that
      expect a specific Q-code title failed until it was rebuilt.
- [x] `cargo xtask verify` green (one unrelated timing flake,
      bd-head-peek-timeout-flake-g1mll754, filed separately).

## Notes

- The new corpus cases failed with `(ERROR)` before the grammar change and
  pass after it; the full corpus (852 cases) stays green.
- End-to-end check: `A {{< kbd x="" >}} B` now renders (the `kbd` handler
  reports `x` as an unknown OS name, which is `kbd`\'s own semantics rather
  than a parse problem).
- Escaped shortcodes (`{{{< >}}}`) already accepted `x=""` through the
  attribute key/value rule; positional `""` there needed the same fix.
