---
title: "stringify: SoftBreak and LineBreak become a space"
date: 2026-11-02
description: "Match pandoc's pandoc.utils.stringify for break inlines (GH #1014)."
---

Strand: `bd-a55pzz19` · Issue: <https://github.com/quarto-dev/q2/issues/1014>

## Overview

`pandoc.utils.stringify` goes through `stringify_inline` in
`crates/pampa/src/lua/utils.rs`. That function mapped `SoftBreak` and
`LineBreak` to `"\n"`. Pandoc 3.12.1 and Quarto 1 map both to a single
space. I checked this locally: pandoc stringifies
`Para{a, SoftBreak, b, LineBreak, c}` to `"a b c"`, and a bare
`SoftBreak()` or `LineBreak()` to `" "`. After this fix q2 gives the
same results.

Block separators (for example, the newline between list items) are not
part of this change.

## Work items

- [x] Unit test `test_stringify_breaks_are_spaces` (fails before the fix)
- [x] Map both breaks to `" "` in `stringify_inline`
- [x] Verify: pampa nextest, clippy (host and Windows), and a strict
  render of this note
