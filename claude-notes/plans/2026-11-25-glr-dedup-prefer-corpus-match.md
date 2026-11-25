---
title: 'parse-errors: GLR dedup prefers the corpus-matched error state'
date: 2026-11-25
description: 'When two GLR versions fail at the same source position, report the one whose parser state has an error-corpus entry instead of whichever was visited first.'
status: in-progress
---

**Braid:** bd-pdqngppm · **Issue:** <https://github.com/quarto-dev/q2/issues/230>

## Overview

`produce_diagnostic_messages` in `crates/quarto-parse-errors/src/error_generation.rs`
dedups error states by `(row, column)` only. After an earlier error forks the
GLR parse, two versions can fail at the same position with different lookahead
symbols. Before this change, the first version visited won the position, even
when it had no corpus entry and a later one did. The result was a generic
"Parse error" instead of, for example, Q-2-12 for an unclosed `*`.

Now each position keeps the first version whose `(state, sym)` yields a
corpus diagnostic, and the generic fallback is used only when no version at
that position has one. Iteration order is already deterministic
(`LinkedHashMap` in `tree_sitter_log.rs`, from #222).

## Work items

- [x] `error_diagnostic_from_parse_state` returns `Option`, with the fallback
      split into `generic_parse_error` (and the span math into
      `error_source_info`)
- [x] The dedup replaces a fallback with a later corpus match at the same
      position
- [x] Unit tests with forked versions at one position (match second, both
      match, neither matches)
- [x] pampa regression test `test_glr_fork_dedup` with the issue's input
