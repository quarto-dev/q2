---
title: 'Engine round-trip rewrites project metadata as document markdown (bd-8svy3p89)'
date: 2026-11-10
description: 'On pages that run code, the engine output''s re-parsed metadata replaces the real metadata wholesale; reconcile it three ways instead, against a parse of the exact markdown the engine received, so only changes the engine made are taken.'
status: draft  # Design agreed 2026-11-11 (three-way metadata reconcile); awaiting the user's read of this plan before implementation
braid:
  strand: bd-8svy3p89
---

**Branch:** `braid/bd-8svy3p89-engine-metadata-round-trip` (main checkout,
based on `main` @ `d84b9ad12`).
**Do not start implementation until the user gives the go-ahead.**

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
| both maps | per key: recurse where the key is in S0, S1 and S2; S2's value for a key only in S2 (added by the engine); drop a key the engine removed (in S1, not S2) |
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

Blocks keep today's two-way reconcile (out of scope; see Risks).

## Work items

### Phase 0: tests first

- [ ] `quarto-ast-reconcile`: unit tests for the meta reconcile over
      hand-built S0, S1 and S2: unchanged leaf keeps S0 (kind and
      `source_info`); changed leaf takes S2; key added; key removed; key
      missing from S1 keeps S0; nested map with one changed leaf keeps the
      siblings from S0; array of equal length with one changed item;
      array of different length takes S2.
- [ ] `engine_execution.rs`: echo-engine test over the probe matrix
      (`Path`, `Glob`, `Expr`, the string cases): every value comes back
      equal to S0, kind and span included.
- [ ] `engine_execution.rs`: an engine that rewrites one front-matter
      value (as span evaluation does) and leaves the rest: that value is
      the engine's, the others are S0's.
- [ ] Pipeline-level test from the subdirectory repro (css link and
      `include-in-header` on a page with a stub engine). First find a
      harness that renders a project with an injected engine registry;
      if none exists, assert on the merged metadata after the engine
      stage instead.
- [ ] Watch each fail on HEAD for the right reason.

### Phase 1: the reconcile

- [ ] `quarto-ast-reconcile`: `reconcile_meta(original, baseline,
      executed) -> ConfigValue` implementing the table above.
      `apply_reconciliation` keeps its blocks job; the "executed metadata
      wins" comment goes away.
- [ ] `merge_markdown_into_ast` takes the baseline text, parses it, and
      replaces `executed.meta` with `reconcile_meta(...)` before applying.
      Parse warnings from the baseline parse are dropped (they repeat the
      executed parse's for unchanged text).
- [ ] Engine loop: pass `unmask(qmd)` as the baseline for each engine; in
      a multi-engine sequence S0 is the metadata reconciled after the
      previous engine.

### Phase 2: record splice

- [ ] `record_splice.rs`: pass the unmasked serialized `qmd` (before the
      edits) as the baseline. Splice never edits front-matter units, so
      every value should come back as S0; add a test that says so.

### Phase 3: the #1152 machinery (pending question 1)

- [ ] If reverted: remove `config_value_text`, `literal_config_strings`,
      `ConfigPath::from_value_in`, the theme-stage `literal_theme_meta`,
      and storing the intermediate's text in its source slot; keep the
      regression test for GH #1152 (an extension `theme:` with two
      underscores on an engine page), now passing through the reconcile.

### Phase 4: real engines

- [ ] Run the GH #1152 repro and `subdir-repro.sh` with jupyter and knitr.
      Check that the front matter knitr emits parses to the same values as
      the front matter it received, so S1 equals S2 for untouched values.
      If knitr reformats YAML in a way that changes values, those values
      fall back to the current behaviour (S2); record what happens.

### Phase 5: notes and docs

- [ ] Update the `config_paths` module docs and the `apply.rs` comments.
- [ ] Mark this plan done; comment on the strand.

## Open questions for the user

1. **The #1152 machinery.** Revert it in Phase 3 (proposed: one mechanism;
   it also stops keeping every engine output's text for the document's
   lifetime), or keep `config_value_text` as a second line of defence?
2. **Baseline parse cost.** Parse the whole baseline text with
   `pampa::readers::qmd::read` (simple, same reader as S2), or only its
   front matter? Proposed: the whole text first, measure on a large
   document, and narrow only if it shows up.
3. **A key the engine removed.** The table drops it (follows the engine).
   Agree, or keep S0's value?
4. **Blocks.** The same three-way idea would stop blocks from depending on
   a faithful writer. Proposed: out of scope; file a strand if wanted.

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
