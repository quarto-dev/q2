# P7-foundation — format-agnostic Pandoc CLI plumbing (extracted from P7)

**Date:** 2026-09-20
**Status:** Shape draft — extracted, not yet re-reviewed as its own plan.
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)  |  Epic: `2026-08-20-pandoc-hybrid-epic.md`
**Implementation task breakdown + test-seam prevalidation:** [`2026-09-20-pandoc-hybrid-P7-foundation-implementation.md`](2026-09-20-pandoc-hybrid-P7-foundation-implementation.md) — migrated verbatim from P7's companion Tasks 1, 2, 3, and 7 (renumbered 1-4 here), with only cross-reference fixes, no scope change.
**Depends on:** P1, P2, P4 — **all already landed**. Does not need P5 or P6 to compile or to be tested at the structural level this plan asserts (content correctness through the shim is a separate concern — see "What this plan does not claim" below).
**Extracted from:** [`2026-08-20-pandoc-hybrid-P7-format-tail.md`](2026-08-20-pandoc-hybrid-P7-format-tail.md), whose Coarse checklist and implementation companion originally carried this scope as Tasks 1 ("multi-format render warning"), 2 ("project-mode containment gate"), 3 ("relax the format gate ... route through `render_qmd_to_pandoc`"), and 7 ("B3 shared services wired into the Pandoc tail"). No new production scope is added by this extraction — every acceptance criterion, test-seam row, and revert hunk below is copied from those four tasks unchanged.

## Why this was split out of P7

A session reviewing whether typst/epub (both explicitly "blocked on the whole epic, P1-P8" in their own plan docs) could start sooner found that most of what they actually needed from P7 wasn't docx/pptx-specific at all — it was the generic machinery that makes *any* non-native format reachable through the CLI:

- the `render.rs` native-format gate relaxation (currently hardcoded to admit only html/revealjs),
- routing a `Pandoc(fmt)` render through P4's `render_qmd_to_pandoc` entry point,
- the multi-format-render warning P7's own relaxation requires as a guardrail (design doc §14),
- the project-mode containment gate P7's own relaxation makes reachable (design doc §13, Gordon's decision),
- and B3 shared-services wiring (resource staging, link rewriting) ahead of the wire-format handoff — needed by every Pandoc tail, not just docx/pptx's.

None of that needs P5 (the Lua shim) or P6 (numbering) to exist — it only needs P1's format identifiers, P2's wire format, and P4's `render_qmd_to_pandoc`/`PandocWriteStage`, all of which are already landed. Splitting it out means P7, typst, and epub can each depend on one small, already-unblocked phase instead of typst/epub informally "following whatever pattern P7 used" for code that hadn't been written yet, and instead of P7 gating its own docx/pptx-specific work behind CLI plumbing that has nothing to do with docx or pptx.

## What this plan does not claim

Landing this plan makes `q2 render f.qmd --to docx` and `--to pptx` **reach pandoc and produce a file**, and makes the CLI-level guardrails (multi-format warning, project-mode containment) correct. It does **not** claim the *content* of that file is correct — numbering, crossref presentation, and category passthrough are P6's and P5's job, and P7 (docx/pptx-specific facts + golden harness) is what proves parity with Q1. A typst/epub follow-on landing its own format arm on top of this plan is in the identical position: reachable and structurally sound, not yet golden-verified.

## In scope (= P7's former Tasks 1, 2, 3, 7)

- **The multi-format render warning** — a pure diagnostic (`Q-18-*`) naming the used and skipped keys when `format:` declares more than one, wired in *before* the gate relaxation lands so the guardrail the relaxation removes has a replacement the moment it's needed.
- **The project-mode containment gate** — gate `WebsiteProjectType::post_render`'s hook sequence on `format.identifier.is_html_based()`, so a website/book/manuscript project rendered to a Pandoc target doesn't write an HTML sitemap/alias-redirect set that doesn't correspond to what was actually produced.
- **Relax the `render.rs:680-684` native-format gate** to admit `Docx` and `Pptx` (the two formats P1/P4 already support end-to-end machinery for), route `Pandoc(fmt)` profiles through P4's `render_qmd_to_pandoc`, and wire the multi-format warning in at the same call site. Verified end-to-end for both formats, since they fail at two different lines today (docx at the gate itself, pptx one line earlier at `Format::from_format_string`).
- **B3 shared services wired into the Pandoc tail** — confirm, with tests, that `ResourceCollector`'s mediabag/resource staging and `LinkRewriteTransform` run before the wire-format handoff, so images and relative links resolve in the produced file.

## Out of scope (stays in P7, or belongs to a later format's own plan)

- The docx/pptx-specific `execute`/`pandoc` defaults table, the pandoc-defaults forwarding allow-list's specific key set, the docx callout-icon PNG vendoring, the latex stub, the `Meta`-block mapping, and the docx/pptx golden-test harness (`quarto-ooxml-extract`, `capture-pandoc-goldens`, the per-fixture accepted-divergence provenance). All of that is genuinely docx/pptx-shaped and stays in P7.
- Extending the `render.rs` admit-list to `Typst`/`Epub` themselves — each follow-on format adds its own arm when it lands, the same way P7 adds `Docx`/`Pptx` here. This plan establishes the *pattern and the guardrails*, not every format's membership in the allow-list.
- Any registration of a shared "invocation builder" trait/interface across formats — P7's own invocation builder (Task 4, unmoved) is still bespoke to docx/pptx; if a shared abstraction turns out to be worth it once typst/epub land their own, that is a follow-on refactor, not scope here.

## Consumes / Produces

- **Consumes:** P1 (`FormatIdentifier`, the `Pandoc`-kind transform exclude-list), P2 (wire format + `Meta` carriage), P4 (`render_qmd_to_pandoc`, `PandocWriteStage`, the pandoc-subprocess diagnostic). All landed.
- **Produces:** a working, guardrailed CLI path from `q2 render ... --to <pandoc-format>` to a real pandoc invocation, for any format identifier already known to `FormatIdentifier` and added to the (still format-by-format) admit-list — the foundation P7, and later typst/epub, build their format-specific tails on top of.

## Coarse checklist

- [x] Task 1 — the multi-format render warning (`Q-18-*` diagnostic, docs page, sidebar entry).
- [x] Task 2 — the project-mode containment gate (`is_html_based()` guard on `WebsiteProjectType::post_render`).
- [x] Task 3 — relax the format gate, admit docx and pptx, route through `render_qmd_to_pandoc`, wire the warning in.
- [x] Task 4 (formerly P7's Task 7) — B3 shared services (resource staging, link rewriting) confirmed wired into the Pandoc tail.

See the implementation companion for full acceptance criteria, test-seam specs, revert hunks, and vacuity checks — migrated verbatim from P7's companion.
