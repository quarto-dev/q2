---
title: "tree-sitter-qmd: accept a colon in attribute keys"
date: 2026-11-03
description: Let the external scanner read `:` inside an attribute key, as Pandoc does, so `html:style="..."` parses.
---

Strand: bd-aidsrl7n · Issue: https://github.com/quarto-dev/q2/issues/1027

## Overview

An attribute key containing a colon, such as `::: {html:style="color: red;"}`,
stopped the qmd parser with a generic `Parse error`. The key token comes from
`parse_language_specifier` in
`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`, which accepted
letters, digits, `_`, `%`, `.` and `-` and returned false at `:`. Pandoc's
attribute keys admit `-`, `_`, `:` and `.`, and extensions such as
quarto-prism key attributes by output format.

New behaviour: `:` is a word character in that scanner loop, so the key token
is produced and the attribute keeps its name (`html:style`). The same loop
also produces language specifiers and naked values (both for attributes and
shortcode arguments); those inputs used to fail at `:` or fall back to the
`shortcode_naked_string` regex, which aliases to the same node kind.

## Work items

- [x] Corpus test `attribute key with colon (GH #1027)` in `test/corpus/inline-attributes.txt` (failed before the fix)
- [x] Integration tests `crates/pampa/tests/integration/test_attr_key_colon.rs`: div, span, code block, mixed attrs, qmd writer round trip (failed before the fix)
- [x] Add `:` to the scanner's key character set
- [x] `cargo xtask ts-test`, `cargo nextest run -p pampa`, `cargo xtask verify --skip-hub-build`
