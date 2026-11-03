---
title: 'Honor execute options at top level and under `format: <fmt>:` (bd-hgy8z0tw)'
date: 2026-11-03
description: 'Q1 sorts bare execute-defaults keys (fig-width, echo, warning, ...) into format.execute; q2 dropped them silently. Fold them into execute: per metadata layer, and deep-merge execute: across the `format: <fmt>:` sublevel.'
braid:
  strand: bd-hgy8z0tw
---

**Issue:** <https://github.com/quarto-dev/q2/issues/1001> · **Strand:** bd-hgy8z0tw ·
**Branch:** `braid/bd-hgy8z0tw-honor-execute-options-top`.

## Overview

Current behaviour: engines read document-scope execute options only from
the merged `execute:` map (`merge_execute_scope` in
`stage/stages/engine_execution.rs` → `ExecutionContext.execute_scope`).
A bare top-level `fig-width: 9`, or `format: html: fig-width: 9`, is
silently ignored, and so is every other execute option (`echo: false`
at top level was verified to be ignored too). The issue reported only
`fig-*`; the gap is the whole execute-options group.

Quarto 1 (`metadataAsFormat` in
`external-sources/quarto-cli/src/config/metadata.ts`) moves every key in
`kExecuteDefaultsKeys` into `format.execute`. It does this once for the
top level of each metadata layer and once for its `format.<fmt>`
sublevel, then deep-merges the two (`mergeFormatMetadata`), so
format-specific values win. Layers (project, directory, document)
then merge in the usual order.

A second, related gap: `quarto_config::resolve_format_config` flattens
`format.<fmt>` over the top level *shallowly*. A `format: html: execute:`
map replaces the top-level `execute:` map instead of merging into it.
Q1 deep-merges. The fold below needs that deep merge to be correct
(top-level `fig-width: 9` plus `format: html: echo: false` must give
both), so it is fixed here too.

## Design

In `resolve_format_config` (one place, used by `MetadataMergeStage` for
every layer and by `format_paths`):

1. Fold the top-level map: collect the bare `kExecuteDefaultsKeys` and
   the `execute:` map into one `execute` map, in YAML order, with later
   entries winning. This is Q1's order-dependent behaviour exactly:
   `metadataAsFormat` iterates keys in order and both paths assign
   into `format.execute`. A scalar `execute: true/false` becomes
   `enabled:` (Q1's special case), but only when there are bare keys to
   fold. Otherwise the scalar is left untouched, so current behaviour is
   unchanged.
2. Fold the `format.<fmt>` map the same way.
3. Flatten as today, except `execute` is deep-merged (one level: the
   format sublevel's keys override the top level's keys).

The bare keys are **kept** in place as well as copied into `execute`.
Q1 removes them from the Pandoc metadata, but other q2 code may read a
top-level key (`engine`, `freeze`), and removing them is a larger
behaviour change than this fix needs. This is a judgment call, so it is
noted in the PR.

Key list: Q1's `kExecuteDefaultsKeys`, verbatim: `fig-width`,
`fig-height`, `fig-format`, `fig-dpi`, `fig-asp`, `mermaid-format`,
`df-print`, `error`, `eval`, `engine`, `cache`, `freeze`, `echo`,
`output`, `warning`, `include`, `keep-md`, `keep-ipynb`, `enabled`,
`ipynb`, `daemon`, `daemon-restart`, `debug`, `ipynb-filter`,
`ipynb-filters`, `ipynb-shell-interactivity`, `plotly-connected`.

Because the fold happens before the cross-layer merge, precedence
across layers is unchanged: for example, `_quarto.yml` `fig-width: 9`
loses to front-matter `execute: fig-width: 5`, as in Q1.

## Work items

- [x] Unit tests in `quarto-config/src/format.rs`: bare key folded;
      `format.<fmt>` bare key folded and wins; deep merge of `execute`
      across sublevels; `!prefer` on `format.<fmt>.execute` still replaces;
      YAML-order tie-break; boolean `execute:` → `enabled:`; scalar
      `execute:` untouched with no bare keys
- [x] Cross-layer test in `metadata_merge.rs`
      (`test_bare_execute_options_fold_per_layer`)
- [x] Implement the fold + deep merge in `resolve_format_config`
- [x] End to end with knitr (R 4.6): top level, `format: html:` and
      `execute:` all give a 9×4 in SVG (`viewBox="0 0 648 288"`); top-level
      and `format: html:` `echo: false` hide the source
- [x] Docs: `docs/guides/authoring/figures.qmd` already says "in the
      document header", which is now true; no change needed
- [x] Verify: `cargo xtask verify --skip-hub-build`. Rust tests (17650),
      clippy, fmt, the ts-packages build and the claude-notes render all pass.
      The hub-client vitest step needs the WASM bundle, which
      `--skip-hub-build` does not build, so CI covers it.
- [ ] Open a PR and check CI
