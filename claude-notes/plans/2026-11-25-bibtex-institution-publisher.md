---
title: 'citeproc: map BibTeX institution and school to publisher'
date: 2026-11-25
description: 'A BibTeX techreport lost its institution and a thesis lost its school, because the publisher was read only from the publisher field. Fall back to institution and school, as pandoc does.'
status: in-progress
---

**Braid:** bd-4rfhpfhn
**Issue:** <https://github.com/quarto-dev/q2/issues/1305>

## Overview

`biblatex_entry_to_reference` in `crates/pampa/src/citeproc_filter.rs` set the
CSL `publisher` from the BibTeX `publisher` field only. A `@techreport` names its
publisher in `institution` and a `@phdthesis`/`@mastersthesis` in `school`, so
those entries rendered without a publisher. Pandoc's citeproc maps both fields
to the publisher.

Now `publisher` falls back to `institution`, then `school`, when `publisher` is
absent.

## Work items

- [x] Extend `test_load_bibtex_macros_crossrefs_and_types` with publisher
  assertions for the existing techreport, phdthesis and mastersthesis entries
  (fails before the fix)
- [x] Add the `institution` / `school` fallback
- [x] Run pampa tests, clippy (host and Windows), xtask lint, and the notes render
