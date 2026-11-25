---
title: 'pampa: incremental writer keeps CRLF for rewritten blocks and separators'
date: 2026-11-25
description: 'In a CRLF document, the incremental writer emitted bare LF for blocks it rewrote from scratch and for the separators it synthesized next to them. It now converts that generated text to CRLF.'
status: in-progress
---

**Braid:** bd-vhgpvq6n
**Issue:** <https://github.com/quarto-dev/q2/issues/966>

## Overview

`incremental_write` in `crates/pampa/src/writers/incremental.rs` copies
unchanged blocks verbatim, but when it rewrites a block (`CoarsenedEntry::Rewrite`)
it uses the standard qmd writer, which always emits `\n`. The fallback
separator in `compute_separator` is also a literal `"\n"`. A CRLF document
therefore came back with mixed line endings after an edit that added or
replaced a block.

New behaviour: `assemble` checks once whether the original contains `\r\n`.
If it does, rewritten block text has every bare `\n` converted to `\r\n`, the
fallback separator is `"\r\n"`, and the "previous block already ends with a
blank line" check also recognises `\r\n\r\n`. When the input was padded with a
trailing newline, the result (and edit replacement) drops a trailing `\r\n`
as well as a bare `\n`.

## Work items

- [x] Tests in `crates/pampa/tests/integration/incremental_writer_tests.rs`
  (appended paragraph, rewritten multi-line block, missing trailing newline,
  LF control)
- [x] Convert rewritten block text and synthesized separators
- [x] Strip `\r\n` when the input was padded
- [x] Verify: pampa nextest, clippy, xtask lint, notes render
