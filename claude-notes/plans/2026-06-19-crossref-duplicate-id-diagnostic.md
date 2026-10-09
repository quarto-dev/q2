---
title: 'Duplicate crossref identifier — diagnosis & structured diagnostic'
date: 2026-06-19
description: 'Turns the unstructured duplicate crossref identifier error into a located, coded diagnostic first, and later relabels the two duplicate figure cells in the docs so the docs render is clean.'
status: in-progress  # decisions settled 2026-06-22; implementing **Facet B first** (Stage 1).
braid:
  strand: bd-rr6qzcvu  # discovered-from bd-bxrkxblx
  priority: P2
---

## Decisions (settled 2026-06-22)
- **A — relabeling:** approved (`fig-charts-jupyter` / `fig-charts-knitr`), but
  **deferred to Stage 2**. The user wants to *see* the improved error message on
  a real `docs/` render before the underlying duplicate is removed.
- **B — catalog code:** dedicated `crossref` subsystem **15**, code `Q-15-1`.
- **C — scope/order:** **staggered, two stages.**
  - **Stage 1 (now): Facet B** — the structured diagnostic. Leave the duplicate
    in `figures.qmd` in place so `q2 render docs/` still fires it and shows the
    new boxed `[Q-15-1]` form.
  - **Stage 2 (later): Facet A** — relabel the two cells; `docs/` render goes to
    0 errors.

## Symptom

`cargo run --bin q2 -- render docs/` finishes with:

```
Rendered 168 of 168 files to …/docs/_site — 1 error, 27 warnings
```

The lone error is:

```
Error: duplicate crossref identifier `fig-charts`
```

It is **collected**, not fatal — the render completes (168/168). It prints as a
bare one-line string: no error code, no source span, no ariadne box (contrast
the `Q-5-6`/`Q-13-4` diagnostics in the same run, which render with boxes).

There are **two independent problems** here. One clears the error; the other is
the durable improvement the strand is really about.

---

## Facet A — the docs content bug (what trips the error)

`docs/guides/authoring/figures.qmd` has **two** code cells both labeled
`#| label: fig-charts`:

- line \~428 — a **Jupyter** (`{python}`) cell, inside a `::: panel-tabset`
- line \~448 — a **Knitr** (`{r}`) cell, the sibling tab in the same tabset

They are the "Subcaptions" example shown in two engines (Jupyter tab / Knitr
tab). Because both panels live in the rendered DOM simultaneously (tabsets are
CSS show/hide), two elements end up with `id="fig-charts"` — invalid HTML and an
ambiguous cross-reference target. The crossref indexer rejects the second one.

**Safe to fix:** there is **no `@fig-charts` reference anywhere in `docs/`**
(grep is empty), so relabeling breaks nothing.

### Options (Decision A)
- **(A1, recommended) Relabel both** to engine-specific ids:
  `fig-charts-jupyter` and `fig-charts-knitr`. Keeps both figures
  cross-referenceable and unambiguous; matches the "same example, two engines"
  intent.
- **(A2) Keep one labeled, drop the label from the other.** The unlabeled tab
  loses numbering/cross-referenceability. Simpler diff, but asymmetric for no
  real reason.

This is the change that makes `q2 render docs/` report **0 errors**.

> Note: this is genuinely a per-document authoring fix, *not* a sign the
> crossref engine is wrong to reject it — two live DOM nodes sharing an id is a
> real defect regardless of tabs.

---

## Facet B — the engineering bug (the diagnostic is unstructured)

The diagnostic is built by `duplicate_id_diagnostic` in
`crates/quarto-core/src/transforms/crossref_index.rs:369`:

```rust
fn duplicate_id_diagnostic(
    id: &str,
    _first: &quarto_source_map::SourceInfo,
    _second: &quarto_source_map::SourceInfo,
) -> DiagnosticMessage {
    // SourceInfo attachment to DiagnosticMessage would let ariadne render
    // both occurrences; plumb that through when the broader source-context
    // threading in transforms is sorted out (same TODO as the metadata
    // diagnostics).
    DiagnosticMessage::error(format!("duplicate crossref identifier `{id}`"))
}
```

**The fix is unusually clean** — most of the wiring already exists:

1. **Both spans are already passed in and discarded.** At the call site
   (`crossref_index.rs:262-268`) the indexer has `existing_src` (the first
   occurrence, stored in the index) and `node.source_info` (the duplicate). It
   passes both to `duplicate_id_diagnostic` as `_first`/`_second`. The TODO is
   the only thing standing between us and a located diagnostic.
2. **The render path already carries them to output.** `walker.diagnostics` is
   pushed onto `ctx.diagnostics` (`crossref_index.rs:89-92`) →
   `render_output.diagnostics`, rendered against `render_output.source_context`
   — the exact path the new `Q-5-6` warning rides. The doc's source is in that
   context, so `Original` spans resolve.
3. **The plain-text rendering is purely a missing-location artifact.** `to_text`
   gates the ariadne box on `has_any_location = self.location.is_some() || any
   detail has a location` (`diagnostic.rs:357`). Today that's false → plain
   string. Attaching a location flips it to the boxed render automatically.

### The change
Rewrite `duplicate_id_diagnostic` to use `DiagnosticMessageBuilder`:

```rust
DiagnosticMessageBuilder::error("Duplicate crossref identifier")
    .with_code("Q-15-1")
    .with_location(second.clone())                 // underline the duplicate
    .problem(format!(
        "The crossref identifier `{id}` is defined more than once. \
         Each crossref id must be unique within a document."
    ))
    .add_detail_at("first defined here", first.clone())   // the original site
    .add_hint("Give one of the targets a different `label:` / `#id`.")
    .build()
```

(`add_detail_at` / `add_faded_at` already exist on the builder — see
`builder.rs`.) Keep it **error-level**: a duplicate id is genuinely ambiguous.

### New catalog code (Decision B)
No `crossref` subsystem exists yet. The `CONTRIBUTING-ERRORS.md` table predates
the catalog's real growth (it stops at 11 and marks "12+ Reserved", yet the
catalog already has listing=12, navigation=13, theme=14 — each a dedicated
subsystem with a descriptive string). Following that established practice:

- **(B1, recommended) Dedicated `crossref` subsystem = number 15, code
  `Q-15-1`.** Mirrors listing/navigation/theme; gives `docs/errors/crossref/` a
  natural home. Next new subsystem number after theme(14).
- **(B2) Fold under `Q-4` "Rendering and Formats"** (currently unused). Honors
  the CONTRIBUTING table's reservation, but crossref isn't really "formats", and
  no Q-4 code exists to anchor it.

If B1: also refresh the stale `CONTRIBUTING-ERRORS.md` subsystem table to record
12/13/14/15 (small, optional, keeps the registry honest).

### Scope (Decision C)
The strand description floated splitting A and B. Recommendation: **do both in
this one strand/PR.** Facet A is what clears the actual CI/docs error; Facet B
is small and is the reason the strand is interesting. They're tightly related
(B's regression test is most naturally a duplicate-id fixture, and A is the
real-world instance B improves the reporting of).

---

## Test plan (TDD — write tests first)

1. **Catalog presence test** (`catalog.rs`): `Q-15-1` exists, subsystem
   `crossref`, title mentions "duplicate"/"crossref", `docs_url` ends in the
   code. (Red → green when the catalog entry lands.)
2. **Transform unit test** (`crossref_index.rs` tests): index two custom
   crossref targets sharing an id, each with a *distinct* `SourceInfo`; assert
   the single emitted diagnostic has `code == Q-15-1`, a primary
   `location.is_some()` (the second), and at least one detail whose
   `location.is_some()` (the first). The existing test scaffolding
   (`run …` returning `(ast, index, diagnostics)`, the `si()` helper) makes this
   direct.
3. **Render smoke (optional)**: assert the rendered diagnostic now satisfies
   `has_any_location` (i.e. it would take the ariadne branch) — guards against a
   future regression dropping the spans again.
4. **Docs build**: `q2 render docs/errors/crossref/Q-15-1.qmd` (via the project)
   builds clean.
5. **End-to-end**:
   - After **Facet A**: `q2 render docs/` reports **0 errors** (down from 1).
   - For **Facet B**: a tiny fixture qmd with a deliberate duplicate id, rendered
     through the binary, shows the boxed `[Q-15-1]` diagnostic underlining both
     label sites. Paste the output here.

## Work items

### Stage 1 — Facet B (the structured diagnostic) — IN PROGRESS

- [x] Settle Decisions A (relabel approach), B (subsystem number), C (one PR vs
      split) with the user. *(2026-06-22: relabel both but defer to Stage 2;
      crossref=15; staggered, B first.)*
- [x] **(B)** Add `Q-15-1` to `error_catalog.json` (subsystem `crossref`);
      catalog presence test (`error_catalog_has_q_15_1_crossref`) — red → green.
- [x] **(B)** Transform unit test `duplicate_id_diagnostic_is_coded_and_located`
      (code + primary location + located detail) — red → green.
- [x] **(B)** Rewrite `duplicate_id_diagnostic` to the builder form
      (`with_code` / `with_location(second)` / `add_detail_at(first)` / hint);
      the `first`/`second` spans were already passed in. Stale TODO removed.
- [x] **(B)** Authored `docs/errors/crossref/Q-15-1.qmd` (new `crossref/`
      subsystem dir).
- [x] **(B)** Refreshed the `CONTRIBUTING-ERRORS.md` subsystem table
      (added 12/13/14/15 + a note on how new subsystems are numbered).
- [x] End-to-end (Facet B): fixture with two `#fig-charts` images → boxed
      `[Q-15-1]` underlining **both** sites ("first defined here" + problem),
      exit 0. Real `docs/` render: the `figures.qmd` tabset duplicate now renders
      the same boxed multi-line diagnostic (cells 427–441 and 447–457). Tally
      stays "1 error" by design (duplicate still present — that's Stage 2).
- [ ] `cargo nextest run --workspace` (10,289 passed) ✓; `cargo xtask verify`
      (full — `quarto-core` is in the WASM closure) — *running.*

### Stage 2 — Facet A (relabel the docs cells) — DONE

- [x] **(A)** Relabeled the two `fig-charts` cells in
      `docs/guides/authoring/figures.qmd` to `fig-charts-jupyter` /
      `fig-charts-knitr`. *(2026-06-22)* `q2 render docs/` now reports
      **`Rendered 169 of 169 files … — 27 warnings`** (was `1 error, 27
      warnings`): no `Error:` lines, no `Q-15-1`. The 27 warnings are the
      pre-existing `Q-5-6` (missing images) and `Q-13-4` (body links) — out of
      scope here.

## Key references
- `crates/quarto-core/src/transforms/crossref_index.rs:262-268` — call site
  (both spans available) and `:369` — `duplicate_id_diagnostic` (drops them).
- `crates/quarto-core/src/transforms/crossref_index.rs:89-92` — diagnostics →
  `ctx.diagnostics`.
- `crates/quarto-error-reporting/src/diagnostic.rs:357` — the `has_any_location`
  gate that currently forces plain-text rendering.
- `crates/quarto-error-reporting/src/builder.rs` — `with_code`, `with_location`,
  `add_detail_at`, `add_hint`.
- `crates/quarto-error-reporting/CONTRIBUTING-ERRORS.md:83` — subsystem-number
  registry (stale past 11).
- `docs/guides/authoring/figures.qmd:428,448` — the duplicate-labeled cells.
- Precedent: bd-bxrkxblx's `resource_copy_diagnostics.rs` + `Q-5-6`/`Q-5-7`
  pages — same shape of fix (code + span + docs page).
