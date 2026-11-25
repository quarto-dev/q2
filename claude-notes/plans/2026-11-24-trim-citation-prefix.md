---
title: 'citeproc: trim leading and trailing space from citation prefixes'
date: 2026-11-24
description: 'The parser kept the space after a semicolon as the start of the next citation prefix, so a re-sorted citation started with a space. Trim prefixes the way the Pandoc reader does.'
status: in-progress
---

**Braid:** bd-whiavixk
**Issue:** <https://github.com/quarto-dev/q2/issues/1313>

## Overview

`make_cite_inline` in `crates/quarto-pandoc-types/src/inline.rs` splits a
bracketed citation at `;` and gives each citation everything before its key
as the prefix. The space after `;` therefore became the start of the next
prefix: `[@a; @b]` gave `@b` the prefix `[Space]`, and
`[see @x2, p. 3; also @x]` gave `[Str "see", Space]` and
`[Space, Str "also", Space]`. When a CSL style re-sorts the items, the item
that moves to the front renders as `( Aa, 2000; ...)`. The qmd writer also
round-tripped `[@a; @b]` as `[@a;  @b]`.

Pandoc's reader trims each prefix (`trimInlines`), giving `see` and `also`.
Each citation's prefix is now trimmed of leading and trailing
`Space`/`SoftBreak` the same way. The citeproc filter, HTML writer and qmd
writer already add their own space after a non-empty prefix.

One more consumer relied on the old spacing: heading auto-ids
(`crates/pampa/src/utils/autoid.rs`) rebuild citation text from prefix,
id, and suffix, and now insert the separators themselves.

## Work items

- [x] Trim prefixes in `make_cite_inline` (both the single-citation and
      the multi-citation branches), with unit tests for `[@a; @b]` and
      `[see @x2, p. 3; also @x]`.
- [x] Insert separators explicitly in the auto-id cite text.
- [x] Update the pampa native snapshots, the annotated-qmd JSON fixtures,
      the `typst-citations-csl` book guard golden (double space gone), and
      the `cite_prefix` shim test (now `see` plus nbsp, as in Q1).
