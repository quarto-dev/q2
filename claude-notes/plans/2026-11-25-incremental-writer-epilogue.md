---
title: 'pampa: incremental writer keeps the bytes after the last block'
date: 2026-11-25
description: 'The incremental QMD writer dropped everything after the last block, such as trailing blank lines, on every write. Copy that region verbatim, the way the front matter region before the first block is copied.'
status: in-progress
---

**Braid:** bd-0qpc59zp · **Issue:** <https://github.com/quarto-dev/q2/issues/963>

## Overview

`assemble` in `crates/pampa/src/writers/incremental.rs` emitted the
metadata prefix and then each block with its separator, but nothing for
the bytes from the end of the original's last block to the end of the
input. So `incremental_write` with no edits turned
`"First paragraph.\n\nLast paragraph.\n\n\n"` into a string without the
two trailing blank lines, and an edit to any block dropped them too.

The other half of #963 (a front-matter-only document came out empty) was
already fixed on main by `a2a5aa10d`, in `emit_metadata_prefix`.

New behaviour: after the block loop, `assemble` appends
`original_qmd[last_span.end..]`, where `last_span` is the `preimage_in`
span of the original AST's last block. The epilogue is anchored to the
original document, not to the last emitted entry, so it survives edits
to the last block. It is skipped when no block was emitted or when the
last block has no preimage in the target file.

## Work items

- [x] Idempotence tests for trailing blank lines and front-matter-only
  input, plus a test that rewrites the last block, in
  `crates/pampa/tests/integration/incremental_writer_tests.rs`
- [x] Epilogue step in `assemble`
- [x] pampa tests, clippy, lint
