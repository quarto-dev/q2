---
title: 'Engine round-trip rewrites project metadata as document markdown (bd-8svy3p89)'
date: 2026-11-10
description: 'On pages that run code, the engine output''s re-parsed metadata replaces the real metadata wholesale; reconcile it three ways instead, against a parse of the exact markdown the engine received, so only changes the engine made are taken.'
status: active  # Implemented 2026-11-11; PR open
braid:
  strand: bd-8svy3p89
---

**Branch:** `braid/bd-8svy3p89-engine-metadata-round-trip` (main checkout,
based on `main` @ `d84b9ad12`).

## Status

Investigation done (2026-11-10). Approach agreed with the user on
2026-11-11: a three-way metadata reconcile (§ Approach). This plan replaces
the first skeleton, which proposed restoring values by provenance.

## Issue context

Strand `bd-8svy3p89` (bug, P2), filed while fixing GH #1152
(`bd-d50anjlg`, PR #1164, merged and closed): path consumers built from
`doc.source_context` cannot read engine-reparsed values from their spans,
so on pages that run code `template: _partials/_page.html` still loses its
underscores. The investigation found that this is one symptom of a wider
class, described below.

## The problem

**Pipeline today.** `MetadataMergeStage` merges project, `_metadata.yml`,
extension, format and document values into `doc.ast.meta` before the
engine runs. `EngineExecutionStage` then serializes the whole AST,
including that merged metadata, to qmd (`serialize_ast_to_qmd`), masks
nested cells and foreign front-matter spans, runs the engine, unmasks its
output, and re-reads it (`merge_markdown_into_ast`). Reconciliation
compares **blocks only** (`compute_reconciliation` takes `.blocks`);
`apply_reconciliation` takes the engine output's metadata wholesale
(`apply.rs`: "For v1, executed metadata wins entirely"). Nothing ever asks
whether the engine changed a metadata value.

**Why the two-way block diff works and metadata cannot use it.** An
unchanged block survives the writer and reader with the same structural
hash, so "hash matches the original" means "the engine left it alone".
Metadata does not survive the trip. The re-read parses front matter with
document rules, so a value can come back different even when the engine
never touched it.

**Probe** (a temporary echo-engine test;
`engine-metadata-round-trip-investigation/probe-output.txt`):

| Before | After the round-trip |
|---|---|
| `Path("_extensions/a/_b.scss")` | inlines; flattened `extensions/a/b.scss` |
| `Glob("posts/*_draft_*.qmd")` | inlines; flattened `posts/draft.qmd` |
| `Expr("params$x")` | inlines; text `!expr params$x` |
| a string holding a link and a code span | `Link`, `Code`; markup flattened away |
| `Scalar("<b>x</b>")` / `Scalar("$x$")` | raw HTML / `Math`; flattened `x` |
| `Scalar("@ref")` | `Cite` |
| `Scalar("line1\nline2")` | flattened `line1 line2` |
| `Scalar("")` | `PandocBlocks` |
| booleans, integers, reals, null | unchanged |

Consequences on pages that run code: every string loses its kind
(`as_str()` returns `None`, so a key read that way looks absent); a marked
path loses its project-relative base, because its span now lies in the
intermediate file, which `config_paths` resolves against the document's
directory; and text with markdown syntax changes.

**Reproduced at HEAD**
(`engine-metadata-round-trip-investigation/subdir-repro.sh`): a website
with `format.html.css: styles.css` and
`include-in-header: _partials/_head.html` in `_quarto.yml`, pages in
`docs/`:

| | `docs/plain.qmd` | `docs/code.qmd` (python cell) |
|---|---|---|
| `css:` link | `../styles.css` (correct) | `styles.css` (wrong base) |
| `include-in-header` | included | silently dropped (emphasis) |

**Engines do change front matter on purpose.** Inline code in the
document's own front matter is evaluated (EXR-2 decision 11;
`engine/jupyter/text_execute_e2e.rs`,
`e2e_span_in_front_matter_is_evaluated`), so the fix cannot simply keep
the original metadata.

## Approach: three-way metadata reconcile

Three versions of the metadata exist at reconcile time:

- **S0**: `doc.ast.meta` before the engine: the real values, with their
  kinds, spans and path bases.
- **S1**: the metadata of a parse of the exact text the engine received
  (`unmask(qmd)`).
- **S2**: the metadata of the parse of the engine's output (today's
  executed AST).

S1 and S2 went through the same writer and the same reader, so a
difference between them is the engine's doing; a difference between S0
and S1 is the round-trip's. The rule, applied recursively from the root:

| S1 vs S2 at this node | Result |
|---|---|
| equal (structural hash, spans ignored) | **S0** |
| both maps | per key: recurse where the key is in S0, S1 and S2; S2's value for a key only in S2 (added by the engine); drop a key the engine removed (in S1, not S2; decision 3) |
| both arrays of the same length | per index, recurse |
| anything else (kinds differ, lengths differ) | **S2** |

Equality uses `quarto_ast_reconcile::compute_meta_hash_fresh`, which skips
`source_info` and `key_source`. A key in S0 but missing from S1 (the
writer did not emit it) keeps S0.

What this buys: an untouched value keeps its kind, its real span (so
`declaring_dir` and diagnostics point at the right file) and any `!path`
or `!str` tag; an evaluated front-matter span comes from the engine; keys
an engine adds come through. It does not depend on masking being right
for every engine, and needs no provenance rule.

Blocks keep today's two-way reconcile (decision 4).

## Work items

### Phase 0: tests first

- [x] `quarto-ast-reconcile`: unit tests for the meta reconcile over
      hand-built S0, S1 and S2: unchanged leaf keeps S0 (kind and
      `source_info`); changed leaf takes S2; key added; key removed; key
      missing from S1 keeps S0; nested map with one changed leaf keeps the
      siblings from S0; array of equal length with one changed item;
      array of different length takes S2.
- [x] `engine_execution.rs`: echo-engine test over the probe matrix
      (`Path`, `Glob`, `Expr`, the string cases): every value comes back
      equal to S0, kind and span included.
- [x] `engine_execution.rs`: an engine that rewrites one front-matter
      value (as span evaluation does) and leaves the rest: that value is
      the engine's, the others are S0's.
- [x] Pipeline-level test from the subdirectory repro (css link and
      `include-in-header` on a page with a stub engine). First find a
      harness that renders a project with an injected engine registry;
      if none exists, assert on the merged metadata after the engine
      stage instead.
- [x] Watch each fail on HEAD for the right reason.

### Phase 1: the reconcile

- [x] `quarto-ast-reconcile`: `reconcile_meta(original, baseline,
      executed) -> ConfigValue` implementing the table above.
      `apply_reconciliation` keeps its blocks job; the "executed metadata
      wins" comment goes away.
- [x] `merge_markdown_into_ast` takes the baseline text, parses it, and
      replaces `executed.meta` with `reconcile_meta(...)` before applying.
      Parse warnings from the baseline parse are dropped (they repeat the
      executed parse's for unchanged text).
- [x] Engine loop: pass `unmask(qmd)` as the baseline for each engine; in
      a multi-engine sequence S0 is the metadata reconciled after the
      previous engine.

### Phase 2: record splice

- [x] `record_splice.rs`: pass the unmasked serialized `qmd` (before the
      edits) as the baseline. Splice never edits front-matter units, so
      every value should come back as S0; add a test that says so.

### Phase 3: revert the #1152 machinery (decision 1)

- [x] Remove `config_value_text`, `literal_config_strings`,
      `ConfigPath::from_value_in`, the theme-stage `literal_theme_meta`,
      and storing the intermediate's text in its source slot; keep the
      regression test for GH #1152 (an extension `theme:` with two
      underscores on an engine page), now passing through the reconcile.

### Phase 4: real engines

- [x] Run the GH #1152 repro and `subdir-repro.sh` with jupyter and knitr.
      Check that the front matter knitr emits parses to the same values as
      the front matter it received, so S1 equals S2 for untouched values.
      If knitr reformats YAML in a way that changes values, those values
      fall back to the current behaviour (S2); record what happens.

### Phase 5: notes and docs

- [x] The `apply.rs` comment. (`config_paths` needed nothing: Phase 3
      restored it to its state before #1164.)
- [x] Comment on the strand; mark this plan done when the PR merges.

## Outcome (2026-11-11)

- **Tests.** `quarto-ast-reconcile/src/meta.rs` (11 unit tests);
  `engine_execution.rs` `engine_round_trip_keeps_untouched_metadata` (the
  probe matrix) and `engine_change_to_metadata_is_taken`; `record_splice.rs`
  `splice_keeps_the_metadata_as_it_was`;
  `tests/integration/engine_metadata_round_trip.rs`, three pages in `sub/`
  rendered through `render_to_file` with a `FixtureEngine`: project
  `brand:` (bd-08hfh3ut), project `include-in-header`, `css:` and a theme
  layer (bd-bfp32yw9), and an extension theme partial (GH #1152). Each
  fails with the reconcile disabled, for the reported reason (Q-14-6 on
  `sub/_brand.yml`; the missing header).
- **Real engines.** The GH #1152 repro, `subdir-repro.sh` and the
  bd-08hfh3ut fixture all render correctly with knitr and jupyter, with
  the #1152 machinery reverted, so the front matter knitr emits reads
  back to the values it received. An evaluated front-matter span (a `title:`
  holding an inline Python expression for 6 times 7) still renders as
  `Answer 42`.
- **Hidden double delivery.** `marimo_records::marimo_record_replays_without_engine`
  failed: the replay carried the islands header includes twice. The test
  set its empty registry on `ctx.engine_registry_override`, which
  `render_qmd_to_html` overwrites from `HtmlRenderConfig::engine_registry`,
  so the real marimo engine ran during the "replay without engine". Its
  includes plus the record splice's folded ones made two copies; the old
  round-trip dropped the folded copy. Fixed in the test (registry through
  the config). The only production record splice
  (`pass2_renderer.rs`, the hub-client HTML preview) renders with
  `execute: false`, so no engine runs after a splice there.

## Decisions (2026-11-11)

1. **The #1152 machinery: revert it** in Phase 3, and see whether anything
   still needs it before adding redundancy back.
2. **Baseline parse: the whole engine input**, with the same reader as the
   engine output. Correctness first; no measurement gate.
3. **A key the engine removed is dropped**: the engine's rewrite is
   followed. Rarely wanted, but the simpler rule.
4. **Blocks stay two-way.** A three-way block reconcile would stop writer
   infidelity from changing blocks the engine never touched, but it needs
   an S0-to-S1 block alignment and touches every engine page. No
   writer-fidelity measurement now: the planned port of the Quarto 1
   smoke-all tests will reason about round-trip mismatches deliberately,
   and any leftover writer bugs it finds are the evidence to revisit this.

## Risks / tradeoffs

- **Multi-engine sequences.** Each engine's baseline is its own input
  text, and S0 must advance after each engine, or a value the first
  engine evaluated would be reverted by the second.
- **Engines that reformat front matter.** Equality is on parsed values,
  not bytes, so re-quoting or re-indenting is harmless; a reformat that
  changes a value degrades that value to today's behaviour, not worse.
- **Merge operators.** The hash includes `merge_op`; S1 and S2 come from
  the same reader, so they agree unless an engine adds a tag.
- **Cost.** One extra parse per engine run, of the document-sized input
  rather than the (often larger) output.
