---
title: 'Engine round-trip rewrites project metadata as document markdown (bd-8svy3p89)'
date: 2026-11-10
description: 'On pages that run code, every metadata string comes back from the engine as markdown parsed in the intermediate file, which breaks paths, globs, expressions and plain strings from the project; restore the values the engine cannot have changed.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-8svy3p89
---

**Checkout:** main checkout (see the hand-off note for the branch).
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The strand as filed (make path consumers read
engine-reparsed values from their spans) treats one symptom. The
investigation shows the class is "the engine round-trip replaces the whole
merged metadata with a markdown re-parse", with at least two more
user-visible defects than GH #1152. The proposed fix moves to the round-trip
itself; the design questions below decide its shape.

## Issue context

Strand `bd-8svy3p89` (bug, P2, filed 2026-11-10 while fixing GH #1152):

> `config_paths::resolve_config_path` now reads markdown-parsed strings from
> their source span, but stage/transform `PathCtx` values are built from
> `doc.source_context`, which never registers engine intermediates
> (EngineExecutionStage adds them to `doc.ast_context.source_context` only).
> So on pages that run code, e.g. `template: _partials/_page.html` still
> resolves via flattened inlines and loses the underscores.

## Dependency graph

- **discovered-from** `bd-d50anjlg` (GH #1152, PR #1164, merged): extension
  `theme:` paths with two underscores failed with Q-14-4 on pages that run
  code. The fix reads a markdown-parsed value back from its source span,
  stores the engine intermediate's text in its source slot, and makes
  `theme:` literal before `ThemeConfig::from_config_value`. This strand is
  the rest of that class.
- No `blocks` or `related` edges.

## What the code looks like today

**The mechanism.** `EngineExecutionStage` serializes the AST, including the
fully merged metadata (project, `_metadata.yml`, extension, format and
document values), to qmd (`serialize_ast_to_qmd`), runs the engine, and
re-reads the output with `pampa::readers::qmd::read`.
`quarto_ast_reconcile::apply_reconciliation` then takes the re-read
metadata outright (`apply.rs`: "For v1, executed metadata wins entirely").
The re-read parses front matter with document rules, so every string
becomes markdown, and every span points into the intermediate file.

**Probe.** A temporary echo-engine test put one value of each kind into
the metadata and compared before and after
(`engine-metadata-round-trip-investigation/probe-output.txt`):

| Before | After the round-trip |
|---|---|
| `Path("_extensions/a/_b.scss")` | inlines; flattened `extensions/a/b.scss` |
| `Glob("posts/*_draft_*.qmd")` | inlines; flattened `posts/draft.qmd` |
| `Expr("params$x")` | inlines; text `!expr params$x` (the writer emits the tag as text) |
| a string holding a link and a code span | `Link`, `Code`; the markup is flattened away |
| `Scalar("<b>x</b>")` / `Scalar("$x$")` | raw HTML / `Math`; flattened `x` |
| `Scalar("@ref")` | `Cite` |
| `Scalar("line1\nline2")` | flattened `line1 line2` |
| `Scalar("")` | `PandocBlocks` |
| booleans, integers, reals, null | unchanged |

So, after the round-trip:

1. Every string loses its kind. `ConfigValue::as_str()` returns `None` for
   inlines, so a consumer that reads a project string with `as_str()` sees
   the key as absent on pages that run code. There are about 640
   non-test `as_plain_text()` calls and many more `as_str()` calls in the
   workspace.
2. `Path` values lose their pivot form. A marked path is
   project-relative, but its span now lies in the intermediate, so
   `config_paths` resolves it against the document's directory.
3. The text can change (emphasis, links, math, raw HTML, newlines).

**Reproduced at HEAD** (`engine-metadata-round-trip-investigation/subdir-repro.sh`,
a website with `format.html.css: styles.css` and
`include-in-header: _partials/_head.html` in `_quarto.yml`, and two pages in
`docs/`):

| | `docs/plain.qmd` | `docs/code.qmd` (python cell) |
|---|---|---|
| `css:` link | `../styles.css` (correct) | `styles.css` (broken: rule 2 above) |
| `include-in-header` | included | **silently dropped** (rule 3: `_partials/_head.html` reads as emphasis) |

No warning is printed for either.

**Engines do change front matter on purpose.** Inline code in the
document's *own* front matter is evaluated (EXR-2 decision 11;
`engine/jupyter/text_execute_e2e.rs` `e2e_span_in_front_matter_is_evaluated`),
so "the original metadata always wins" is wrong. Values from other layers
are masked before serialization (`nested_cell_mask::mask_foreign_meta`,
which decides by the value's root `FileId`), so the engine cannot change
them.

## Proposed phases (draft)

- **Phase 0, tests first.** An echo-engine test in `engine_execution.rs`
  asserting that foreign values keep their kind, text and span. A
  pipeline-level test from the subdirectory repro (a stub engine, so CI
  needs no Python). A test that a document-own front-matter span is still
  evaluated.
- **Phase 1, metadata reconcile.** After `apply_reconciliation`, restore
  every foreign value from the pre-engine metadata, using the same
  provenance rule as `mask_foreign_meta` (perhaps in
  `quarto-ast-reconcile` as the `reconcile_meta` the 2025-12-15
  reconciliation plan anticipated). Document-own values come from the
  engine output (question 1).
- **Phase 2, the #1152 machinery.** Decide whether `config_value_text`,
  `literal_config_strings` and the stored intermediate text stay
  (question 2).
- **Phase 3, consumer audit.** Grep for consumers that read metadata
  after the engine stage and assumed a round-trip-specific shape. The
  removed difference should make them correct without changes.
- **Phase 4, docs/notes.** Update the plan, `config_paths` module docs, and
  the reconcile comment in `apply.rs`.

## Open design questions for the user

1. **Document-own values.** For a value from the document's own front
   matter, should we take the engine's re-parsed value always, or restore
   the original when its text is unchanged? Restoring keeps spans in the
   real `.qmd` (better diagnostics, correct `declaring_dir`) and keeps a
   `!path`/`!str` tag the author wrote, at the cost of a text comparison per
   leaf.
2. **The #1152 span-reading machinery.** With foreign values restored, the
   extension-theme case no longer needs it. Keep `config_value_text`
   (a document's own `theme: _a/_b.scss` parses as emphasis with or
   without an engine, and it still rescues that case), or revert it and the
   stored intermediate text so there is one mechanism?
3. **Should project metadata be serialized into the engine's qmd at all?**
   Sending only the document's own front matter would remove the class at
   its source, but it changes `input_qmd` (execution-record capture hashes,
   research C2 in `2026-10-15-fr-round5-spikes.md`) and possibly what knitr
   sees. Proposed: out of scope; confirm.
4. **Keys added by the engine.** Can an engine add a new top-level key
   through the front matter (rather than `ExecuteResult.metadata`)? If so,
   take keys absent from the original from the engine output; the proposal
   does that.

## Risks / tradeoffs (draft)

- The reconcile runs once per engine in a multi-engine sequence, and the
  second engine sees metadata the first one may have evaluated; the
  provenance rule must use the document's file id, not the intermediate's.
- `apply_reconciliation` is shared with the record splice (EXR-14), so a
  spliced record must reconcile metadata the same way.
- `!expr` values: the qmd writer emits `!expr x` as a string, not a tag.
  Restoring foreign values fixes their reading; a document-own `!expr`
  still round-trips as text (question 1 covers it if originals are restored).
