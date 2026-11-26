---
title: "crossref: capitalized references (@Fig-a)"
date: 2026-11-25
description: "Resolve `@Fig-a` against `#fig-a` and capitalize the reference prefix, as Quarto 1 does (GH #1319)."
---

Strand: bd-li6dx0t2. Issue: <https://github.com/quarto-dev/q2/issues/1319>.

## Overview

Quarto 1 treats `@Fig-a` as a reference to `#fig-a` whose prefix is
capitalized (`refs.lua:31-32`, `format.lua` `refPrefix(type, upper)`). Q2
classified `@Fig-a` as a crossref and set `label_upper: true`, but looked it
up in the index with the raw id, so it came back unresolved (`?Fig-a?`).
The HTML renderer and the preview renderer also ignored `label_upper`.

New behaviour:

- `classify_cite` (`crates/quarto-core/src/transforms/crossref_resolve.rs`)
  lowercases the id's first character before the index lookup and stores
  that as `identifier`; `label_upper` is read from the cite's raw id.
- `render_resolved_ref` (`crates/quarto-core/src/transforms/crossref_render.rs`)
  uppercases the prefix's first letter when `label_upper` is set.
- `CrossrefResolvedRef.tsx` (preview) does the same.
- The pandoc shim already passed `label_upper` to `refPrefix`; it now
  receives a resolvable `identifier`.

## Work items

- [x] Unit tests in `crossref_resolve` and `crossref_render`, vitest in
  `CrossrefResolvedRef.test.tsx`
- [x] Resolve-side fix
- [x] HTML render capitalization
- [x] Preview capitalization
- [x] Update `custom_node_schema_conformance` T3.2, which pinned the old
  unresolved behaviour
