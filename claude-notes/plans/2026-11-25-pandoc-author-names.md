---
title: 'pandoc-hybrid: give pandoc plain author names'
date: 2026-11-25
description: 'An author written as a map (name, affiliation) reached the pandoc writers as a MetaMap, which docx, pptx and epub ignore. Rewrite such entries to plain names before pandoc runs and keep the raw entries in authors, as Quarto 1 does.'
status: in-progress
---

**Braid:** bd-ivuvakyv
**Issue:** <https://github.com/quarto-dev/q2/issues/1316>

## Overview

Current behaviour: in the Pandoc-hybrid leg, an `author` entry in Quarto's
object form (`- name: Jane Doe`, `affiliation: …`) is serialized as a
`MetaMap`. Pandoc's docx writer only reads inline or string authors, so the
author is missing under the title and `dc:creator` is empty.

New behaviour: `PandocWriteStage::prepare`
(`crates/quarto-core/src/stage/stages/pandoc_write.rs`) calls
`pandoc_filters::author_names::author_names_for_pandoc` next to
`coerce_meta_blocks_to_inlines`. It rewrites each map entry of `author` to
its plain name (via `metadata::authors::parse_authors`) and, when `authors`
is absent, keeps the raw entries in `authors` for the vendored `authors.lua`.
This matches Quarto 1's `src/command/render/pandoc.ts` ("Resolve the author
metadata into a form that Pandoc will recognize"), including its strictness:
if any entry is not recognized as an author, nothing changes. Plain-name
entries are left as they are, so documents without object-form authors
serialize exactly as before.

## Work items

- [x] Helper `author_names_for_pandoc` with unit tests
- [x] Call it from `PandocWriteStage::prepare`
- [x] Integration case in `tests/integration/pandoc_meta_mapping.rs`
- [x] Manual render of the issue's repro to docx, epub and typst
