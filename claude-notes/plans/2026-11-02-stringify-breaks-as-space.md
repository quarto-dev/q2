---
title: 'pandoc.utils.stringify renders a soft break and a line break as a space (bd-q56ezik1)'
date: 2026-11-02
description: 'stringify_inline in pampa maps SoftBreak and LineBreak to a newline; Pandoc and Quarto 1 map both to a space.'
braid:
  strand: bd-q56ezik1
---

**Issue:** <https://github.com/quarto-dev/q2/issues/1014> · **Strand:** bd-q56ezik1 ·
**Branch:** `braid/bd-q56ezik1-stringify-breaks-as-space`.

## Overview

`pandoc.utils.stringify` goes through the private helper `stringify_inline`
in `crates/pampa/src/lua/utils.rs`, which mapped `SoftBreak` and `LineBreak`
to `"\n"`. Pandoc's `stringify` (and so Quarto 1) maps both to `" "`, so a
paragraph wrapped over two lines read as `first\nsecond` in q2 and
`first second` in Quarto 1.

New behaviour: both breaks stringify as a space, like `Space`. Block
separators (newline between blocks) are unchanged.

`pandoc.utils.blocks_to_inlines` uses a `LineBreak` separator by default, so
its stringified output now also reads with a space; the unit test that
checked it through `stringify` is updated.

## Work items

- [x] Failing unit tests: `test_stringify_linebreak` expects `" "`, new
      `test_stringify_breaks_as_space`.
- [x] Map `SoftBreak` and `LineBreak` to `" "` in `stringify_inline`.
- [x] Update `test_blocks_to_inlines_accepts_nodes` expectations.
- [x] `cargo nextest run -p pampa`, `-p quarto-core`; clippy.
