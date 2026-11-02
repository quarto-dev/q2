---
title: 'pandoc.utils.stringify returns an empty string for a Pandoc document or a Meta table (bd-af9vl9d6)'
date: 2026-11-02
description: 'stringify_value in pampa reads only the array part of a Lua table, so a Pandoc document or a metadata map stringifies to an empty string; stringify them by their contents.'
braid:
  strand: bd-af9vl9d6
---

**Issue:** <https://github.com/quarto-dev/q2/issues/1015> · **Strand:** bd-af9vl9d6 ·
**Branch:** `braid/bd-af9vl9d6-lua-pandocutilsstringify-returns-empty` (worktree).

## Overview

Current: `stringify_value` (`crates/pampa/src/lua/utils.rs`) handles every
Lua table as a list of elements, reading only its array part. A document
(`{blocks, meta, ...}` with the `Pandoc` metatable) and a metadata map (a
`Meta` table, or a nested map, which is a plain table) have no array part,
so both stringify to `""`.

New: in the table arm,

- a table with the `Pandoc` metatable is read with `peek_pandoc_doc` and
  stringifies as its metadata followed by its blocks (pandoc's `query`
  over `Pandoc meta blocks` visits meta first);
- a non-empty table with no array part is read with `peek_config_value`
  and stringified by `stringify_config_value`: its values in key order,
  the existing MetaMap rule;
- other tables keep the list behaviour.

As Carlos noted on the issue, q2's document metadata also carries
`_quarto.yml` / `_metadata.yml` content, so `stringify(doc)` includes
more text than in Quarto 1. Metadata scalars (booleans, numbers) also
count here, where pandoc's document walk only collects inlines. Both
are recorded under D-stringify-nil in
`crates/pampa/tests/lua-conformance/divergences.md`.

## Work items

- [x] Failing unit tests `test_stringify_pandoc_doc`, `test_stringify_meta`
- [x] Fix in `stringify_value`
- [x] End-to-end check of the issue's repro with `q2 render`
- [x] Divergence note
- [x] `cargo nextest run -p pampa`, clippy (host and Windows cross)
