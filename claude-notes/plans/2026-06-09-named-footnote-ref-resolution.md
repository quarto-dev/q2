# Resolve named footnote references `[^id]` (Span.quarto-note-reference)

**Strand:** bd-po3gn41h — "Named footnote refs `[^id]` never resolve
(Span.quarto-note-reference left unresolved)."
**Discovered from:** bd-9aknlx1j (Phase 2e part 2: per-slide footnote coalescing for revealjs)
**Related but distinct:** bd-1kly (reference-location: block/section numbering)

## Overview

Named / reference-style footnotes — a `[^id]` reference in prose plus a
block definition `::: ^id … :::` — never resolve in **any** format,
including plain `format: html`. The reference renders as an empty,
unresolved span and the definition's text is silently dropped. Inline
footnotes (`^[…]`) are unaffected.

This is **not** reveal-specific. It is a gap in the shared
`FootnotesTransform` in `quarto-core`.

### Reproduction (verified end-to-end 2026-06-09)

Input:

```qmd
---
format: html
---
Ref.[^bk]

::: ^bk
Note.
:::
```

`cargo run --bin q2 -- render` produces, in the body:

```html
<p>Ref.<span class="quarto-note-reference" data-reference-id="bk"></span></p>
```

and:

- `Note.` does **not** appear anywhere in the output (definition dropped).
- No `<section … role="doc-endnotes">` / `id="footnotes"` section is emitted.

Expected (matching the inline `^[…]` path): `Ref.` followed by a
`fnref`/`footnote-ref` superscript link, plus a footnotes section
(`Div` with `id="footnotes"`, class `footnotes`, `role="doc-endnotes"`)
containing `Note.` with a backlink.

## Root cause (verified in code + AST)

Two AST shapes are produced **during pampa parsing**, before any
quarto-core transform runs:

1. The `[^bk]` reference. pampa postprocess
   (`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:1138-1150`,
   the `.with_note_reference(...)` filter) lowers the parsed
   `NoteReference` into an **empty** `Inline::Span` with
   class `quarto-note-reference` and kv `reference-id=<id>`:

   ```rust
   .with_note_reference(|note_ref, _ctx| {
       let mut kv = LinkedHashMap::new();
       kv.insert("reference-id".to_string(), note_ref.id.clone());
       FilterResult(vec![Inline::Span(Span {
           attr: (String::new(), vec!["quarto-note-reference".to_string()], kv),
           content: vec![],
           ...
       })], false)
   })
   ```

   This lowering exists so the AST round-trips through standard Pandoc
   JSON (which has no `NoteReference` inline type). The inline `^[…]`
   form instead parses straight to `Inline::Note`, a standard Pandoc
   type — which is why it works.

2. The `::: ^bk … :::` definition becomes a
   `Block::NoteDefinitionFencedBlock { id: "bk", content: [Para "Note."] }`.

AST confirmed via `pampa -t json`: the paragraph holds
`Str("Ref.")` + `Span.quarto-note-reference[reference-id=bk]` (empty),
and a sibling `NoteDefinitionFencedBlock` with id `bk`.

`FootnotesTransform` (`crates/quarto-core/src/transforms/footnotes.rs`):

- `collect_note_definitions` (lines 226-282) **does** collect
  `NoteDefinitionPara` / `NoteDefinitionFencedBlock` into its
  `definitions` map and remove them from the AST — hence the
  definition's text disappears from the body.
- `process_inline` (lines 371-433) resolves only the typed
  `Inline::NoteReference` arm (lines 381-387). For `Inline::Span` it
  merely recurses into `span.content` (line 416-418) — which is empty
  for a note-reference span — so the reference is never resolved and the
  collected definition is never consumed.

Net effect: definition removed from body, reference left as a dead empty
span, no footnotes section. Because `definitions` is a local map that is
dropped at the end of the transform, the unresolved definition is lost.

Note: by the time `FootnotesTransform` runs, the `Inline::NoteReference`
variant has already been lowered to a Span by pampa, so the existing
`NoteReference` arm in `process_inline` is effectively **dead code** for
the qmd render path. (It may still be reachable if some other producer
emits `NoteReference` directly; we will not remove it.)

Pre-existing acknowledgement of the gap:
`crates/quarto-core/src/pipeline.rs:2246-2254` (a doc comment on
`render_qmd_to_preview_ast_emits_inline_footnote_section`).

## Scope

In scope: `reference-location: document` (default) and `margin`.
These are the modes where `FootnotesTransform` actively builds the
footnotes structure.

Out of scope: `reference-location: block` / `section`. There
`FootnotesTransform` early-returns as a no-op (lines 99-105) and never
collects definitions; resolving `[^id]` in those modes is bd-1kly's
domain (block/section numbering). We will add a regression note, not a
fix, for those modes.

## Fix approach

**Chosen: option (a)** — teach `FootnotesTransform` to treat a
`Span` carrying class `quarto-note-reference` with a `reference-id` kv
exactly like `Inline::NoteReference`: resolve against the collected
definitions and replace the whole inline with the standard footnote-ref
superscript (`create_footnote_ref`).

Rationale vs. option (b) (convert the Span back to `NoteReference`
before the transform in a separate pass):

- Keeps all footnote resolution in one place; no new transform/pass and
  no new ordering constraint.
- Once `[^id]` resolves to the standard `Span#fnref…` form, the reveal
  per-slide coalescing (bd-9aknlx1j) picks it up automatically with no
  further reveal work, because it already consumes `FootnotesTransform`'s
  resolved output. This is the behavior bd-9aknlx1j is counting on.
- Minimal blast radius: the only producer of `quarto-note-reference`
  spans is the pampa postprocess above, and nothing downstream consumes
  them today (confirmed by the pipeline.rs comment and by grep —
  `quarto-note-reference` appears only at the producer and this one new
  consumer).

The resolution must happen in `process_inline`'s `Inline::Span` arm:
detect the marker class + `reference-id` kv **before** recursing into
(empty) content, resolve via `collector.resolve_reference(id, …)`, and on
success `*inline = create_footnote_ref(number, &source_info, is_margin)`.
On failure (no matching definition) leave the span as-is (current
broken-reference behavior — a later, separate improvement could warn).

## Work items (TDD — tests first, per CLAUDE.md)

### Phase 1 — Failing tests

- [x] **Unit test** in `crates/quarto-core/src/transforms/footnotes.rs`
      (`mod tests`): construct a `Pandoc` with a paragraph containing
      `Str("Ref.")` + an empty `Inline::Span` with class
      `quarto-note-reference` and kv `reference-id="bk"`, plus a sibling
      `Block::NoteDefinitionFencedBlock { id: "bk", … }`. Run
      `FootnotesTransform` with default (document) reference-location.
      Assert: (1) the span is replaced by a `Span` with id `fnref1`
      wrapping `Superscript(Link …)` with class `footnote-ref`;
      (2) a footnotes `Div` (`id="footnotes"`) is appended;
      (3) the definition block is gone. Mirror the existing
      `test_note_definition_and_reference` test (lines 788-830) but use
      the Span form instead of `Inline::NoteReference`.
      **Run it and confirm it fails** before implementing.
- [x] **End-to-end HTML test** (the contract a real `quarto render`
      relies on). Added as `test_render_to_file_resolves_named_footnote_reference`
      in `crates/quarto-core/src/render_to_file.rs` (`mod tests`), driving
      `render_to_file` — the exact entry point the native CLI pass-2
      renderer uses (`project/pass2_renderer.rs` → `render_document_to_file`),
      confirmed by tracing the `q2 render` path. (Chose this over a new
      `tests/integration/` module because `render_to_file` is the faithful
      CLI file-writing path and already has sibling e2e tests there.)
      Asserts the rendered HTML contains `doc-endnotes`, a `footnote-ref`
      link, and `Note.`, and that no `quarto-note-reference` span remains.
      **Confirmed failing** — output still showed the empty
      `<span class="quarto-note-reference" data-reference-id="bk"></span>`.

### Phase 2 — Implementation

- [x] In `process_inline`'s `Inline::Span` arm
      (`footnotes.rs:416-418`), detect
      `span.attr.1.contains("quarto-note-reference")` with a
      `reference-id` kv; resolve via `collector.resolve_reference` and
      replace the inline with `create_footnote_ref(...)` on success.
      Leave unresolved spans untouched (broken-ref fallback). Keep the
      existing `NoteReference` arm intact.

### Phase 3 — Verify

- [x] `cargo nextest run -p quarto-core` — 2178 passed, no regressions.
- [x] `cargo nextest run --workspace` — 9560 passed, monorepo clean.
- [x] End-to-end: `cargo run --bin q2 -- render` on the repro fixture.
      Output (verified, see snippet below): resolved `fnref1`
      superscript link + `doc-endnotes` section + `Note.` with backlink;
      no `quarto-note-reference` span. Additional manual cases checked:
      undefined ref (`[^missing]`) → span left untouched, no section
      (correct broken-ref fallback); mixed named + inline footnotes →
      source-order numbering 1/2/3 with correct bodies; margin mode →
      identical output to the inline `^[…]` path (so `[^id]` now has full
      parity with `^[…]` in every mode).
- [x] `cargo xtask verify --skip-rust-tests` — **full verify** (touched
      pampa + quarto-core, both in hub-client's closure): strict
      `-D warnings` workspace build + WASM/hub-client build + hub tests
      all passed. (`--skip-rust-tests` only because the workspace
      nextest run above already passed.)

Verified e2e output (`q2 render` of the repro):

```html
<p>Ref.<span id="fnref1"><sup><a href="#fn1" class="footnote-ref" role="doc-noteref">1</a></sup></span></p>
<section id="footnotes" class="footnotes section" role="doc-endnotes">
  …<li><div id="fn1"><p>Note.<a href="#fnref1" class="footnote-back" role="doc-backlink">↩︎</a></p>…
```

### Phase 4 — Follow-ups / notes

- [x] Confirmed the reveal per-slide coalescing path (bd-9aknlx1j) now
      sees `[^id]` footnotes resolved: `[^id]` produces the identical
      `Span#fnref…` + `doc-endnotes` form that the inline `^[…]` path
      already produces, which is what the reveal transform consumes. No
      reveal code change needed. Recorded on bd-9aknlx1j.
- [x] Added cross-reference comments on both halves: the new Span arm in
      `footnotes.rs` points back to the pampa lowering, and the pampa
      `with_note_reference` lowering in `postprocess.rs` now points
      forward to the `FootnotesTransform` consumer (bd-po3gn41h).
- [x] `reference-location: block`/`section` left to bd-1kly. (Note: a
      separate, pre-existing issue surfaced during manual testing —
      `reference-location: margin` set in *front matter* is not honored
      for **either** footnote path, including the long-standing inline
      `^[…]`. The transform's `is_margin` logic itself is correct, so
      this is a meta-plumbing gap, not part of bd-po3gn41h. Not filed
      here; mention to user.)

## Risk / blast radius

- Only consumer added for `quarto-note-reference` spans; sole producer is
  pampa postprocess. Grep confirms no other reader.
- `resolve_reference` already handles the dedupe/number-assignment and
  the "undefined → leave as-is" path; we reuse it unchanged.
- Block/section modes untouched (early return preserved).
- Snapshot tests: watch for any `.snap` deltas in quarto-core if footnote
  output is snapshotted; document per the snapshot-change policy in
  `CLAUDE.md`.
