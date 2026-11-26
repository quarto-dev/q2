---
title: "crossref: honor crossref fig-title / fig-prefix in HTML"
date: 2026-11-25
description: "GH #1317: map the crossref type-title and type-prefix options into the resolved language table so the native HTML path uses them."
---

Strand: `bd-2qnjzifv` · Issue: <https://github.com/quarto-dev/q2/issues/1317>

## Overview

**Current behaviour.** `crossref: {fig-title: Fig, fig-prefix: Fig.}` changes
the caption label and reference text in docx (Quarto 1's Lua filters read the
option directly), but not in HTML. The native HTML path
(`transforms/crossref_render.rs`, `crossref/registry.rs`) takes the caption
title and reference prefix from the resolved language table
(`LanguageTerms::crossref_title` / `crossref_prefix`), and
`LanguageResolveStage` builds that table only from the shipped catalog,
`_language.yml`, and `language:`.

**New behaviour.** `LanguageResolveStage`
(`crates/quarto-core/src/stage/stages/language_resolve.rs`) adds a
top-precedence layer that maps each `crossref.<type>-title` and
`crossref.<type>-prefix` string option to the `crossref-<type>-title` and
`crossref-<type>-prefix` terms. This matches Quarto 1, where the crossref
option overrides the language default. HTML renders "Fig 1: Cap" and
"See Fig. 1".

## Work items

- [x] Failing tests in `crates/quarto-core/tests/integration/language_pipeline.rs`
      (term table precedence; end-to-end HTML caption and reference text).
- [x] Add `crossref_options_layer` to `LanguageResolveStage`.
- [x] Verify: quarto-core nextest, clippy (host and Windows cross), xtask lint,
      notes render.
