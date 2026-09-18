# Plan: epub output format (pandoc-hybrid-writer follow-on)

**Date:** 2026-09-20
**Status:** This plan's Phase 1 core wiring may target `feature/pandoc-writer-hybrid` (the
epic's integration branch) directly, in parallel with the epic's remaining P5/P6/P7 work — it
does not need to wait for a `main` merge. The crossref/numbering verification bullet at the end
of Phase 1 still needs P3/P6 landed first.
**Design (authoritative, epic-side):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)
**Research (this follow-on):** [`../research/2026-09-18-typst-epub-pandoc-q1-inventory.md`](../research/2026-09-18-typst-epub-pandoc-q1-inventory.md) — Part 2 (pandoc's EPUB writer), Part 4 (Q1's epub format).
**Sibling follow-on:** [`2026-09-18-pandoc-hybrid-typst.md`](2026-09-18-pandoc-hybrid-typst.md) — independent, no shared implementation work beyond the epic itself.
**Before starting:** re-verify file/section references against the epic's current landed
state — plan docs and the epic may have moved since this was drafted.

## Overview

Add `epub` as a Q2 output format by reusing Pandoc's own EPUB writer, the same pattern the
epic establishes for docx/pptx: "Pandoc writes final bytes, done" — no post-Pandoc compile
step, unlike typst. Q1's own `format-epub.ts` is 51 lines and there is no epub-specific Lua
*file* — Pandoc's EPUB writer does the heavy lifting itself (it renders each chapter with
Pandoc's own HTML writer via `splitIntoChunks`, then zips the results), so most of what
would otherwise be "port Quarto's HTML rendering to a new target" is already handled
upstream.

Real epub-conditional logic lives inside *shared* filters, not a dedicated epub file:
`crossref/sections.lua:48` gates section-numbering on `isEpubOutput()`,
`customnodes/callout.lua:139-141` registers a **dedicated Callout renderer** shared between
epub and revealjs (~40 lines, not attribute cleanup), and `customnodes/panel-tabset.lua:264`
routes epub to `render_tabset_with_l4_headings`. epub is still the thinnest of the three
formats researched — there's no template, no bundled packages, no compiler — but "thin"
does not mean "nothing to port beyond a CSS file." See Phase 1 below.

**Explicitly out of scope:**
- **Book-project's `cover-image` derivation hook.** Q1's one book-mode-specific bit
  (`onSingleFilePreRender`, `format-epub.ts:38-46`) reads `format.pandoc[kEPubCoverImage]`
  as a guard and sets `format.metadata[kBookCoverImage]` — irrelevant outside book-mode, and
  Q2 has no book-project support at all today. Skip it.
- **Fixing Q2's general section-numbering gap** (`bd-5aklrxgi`). epub's
  `isEpubOutput()`-gated section-numbering branch in `crossref/sections.lua` only
  intersects that gap incidentally; this plan doesn't need to (and shouldn't try to) fix it.

## Checklist

### Phase 0 — Preconditions
- [ ] Confirm `2026-09-20-pandoc-hybrid-P7-foundation.md` has landed — it owns the `render.rs`
      gate-arm mechanism, `render_qmd_to_pandoc` routing, and the multi-format/project-mode
      guardrails Phase 1 bullet 1 below depends on. P7-foundation depends only on P1/P2/P4, so
      check it independently rather than assuming it tracks P7's own (later) completion.
- [ ] Confirm the epic has landed through at least P1 (neutral core), P2 (wire schema), P4
      (run machinery), P3 (upstream crossref split), P5 (Lua shim), and P6 (numbering wiring) —
      any document with a callout, tabset, or crossref goes through the shim's Route-R dispatch,
      and epub's own crossref branch in `crossref/sections.lua` is part of the crossref split P3
      covers.
- [ ] Re-read this plan against the epic's actual landed shapes before starting — names, module
      locations, and other details may have shifted during implementation.

### Phase 1 — Core writer wiring
- [ ] `FormatIdentifier::Epub` **already exists** (`crates/quarto-core/src/format.rs:31`,
      `as_str` at `:49`, `TryFrom<&str>` at `:89`) — no enum work needed. The gate is the
      native-format allow-match at `crates/quarto/src/commands/render.rs:680-684`; add an
      `Epub` arm there. **Depends on `2026-09-20-pandoc-hybrid-P7-foundation.md`** for the
      admit-list shape, `render_qmd_to_pandoc` routing, and the guardrails (multi-format
      warning, project-mode containment) relaxing this gate requires — P7-foundation depends
      only on P1/P2/P4 (already landed), so this is a concrete near-term dependency. This
      bullet's implementation may land directly on `feature/pandoc-writer-hybrid` alongside it.
- [ ] Decide the epub2/epub3 default. Pandoc's `--to epub` defaults to epub3
      (`writeEPUB3`, `Writers.hs:165-167`) — confirm Q2 should match that default (Q1
      exposes both as distinct format identifiers; check whether Q2 needs both `epub` and
      `epub2` variants, or just `epub` = epub3 with an option to downgrade).
- [ ] Invocation builder: `--to epub` (or `epub2`), Q1's format defaults
      (`default-image-extension: png`, `fig-width`/`fig-height` defaults, and
      **`merge-includes: false`**, needed to keep the two `include-in-header` CSS files below
      from colliding).
- [ ] `html-math-method` switch: `webtex` for epub2, `mathml` for epub3 — port directly
      from `format-epub.ts`'s logic. Math delivery is resolved: the epic's design doc
      (`pandoc-hybrid-architecture.md`, Route-L/R/N table) freezes `Equation` as Route N —
      "a plain filter that RawInline-wraps the existing `Math` inline." The wire cut hands
      Pandoc real unresolved `Math` inlines for the epub leg (as for typst), so
      `html-math-method` behaves as a normal Pandoc option here.
- [ ] Meta-block mapping for EPUB-specific metadata: cover image, identifier, language,
      via Pandoc's `getEPUBMetadata`/`metadataFromMeta` conventions (`EPUB.hs:176,342`).
- [ ] `epub-chapter-level` / `--split-level`: a real, user-facing knob, not automatic
      chapter splitting. Pandoc's `writerSplitLevel` (default `1`, `Options.hs:385`) controls
      where `splitIntoChunks` cuts the document into chapter files; Q1 exposes this as
      `epub-chapter-level` (`config/constants.ts:752`). This must land in the per-format
      pandoc-defaults allow-list. Also account for Pandoc synthesizing a level-1 header from
      the document title when none exists (`EPUB.hs:555-562`) — worth a golden-test case.
- [ ] Pandoc-defaults forwarding allow-list — name the actual keys: at minimum
      `epub-cover-image`, `epub-metadata`, `epub-fonts`/`epub-embed-font`, `epub-subdirectory`,
      `split-level`, `css`.
- [ ] Wire the two HTML-family CSS includes Q1 uses: `styles-callout.html` (shared verbatim
      with the plain HTML format — confirm Q2's HTML format already has an equivalent to
      reuse rather than duplicate) and a new `formats/epub/styles.html` (mostly the same
      `.quarto-layout-*` panel/figure CSS the HTML writer already ships — check whether
      Q2's HTML writer's CSS can be reused directly instead of vendoring a second copy).
- [ ] **The three epub-conditional renderers below are already vendored — nothing to copy.**
      `crossref/sections.lua`, `customnodes/callout.lua`, and `customnodes/panel-tabset.lua`
      all already exist byte-identical in `resources/pandoc-filters/filters/` (landed
      wholesale by the epic's P4 Task 1, same as every other Q1 Lua file). This item is:
      **exercise these already-vendored epub branches through the shim and verify their
      output**, not port/write anything new:
      - `crossref/sections.lua:48`'s `isEpubOutput()` guard on section-numbering.
      - `customnodes/callout.lua:139-141`'s dedicated Callout renderer, shared with
        revealjs (`isEpubOutput() or isRevealJsOutput()`) — confirm Q2's HTML-family
        callout CSS actually matches the class names this renderer emits; don't assume
        parity just because Q2 already has *a* callout renderer for HTML.
      - `customnodes/panel-tabset.lua:264`'s `render_tabset_with_l4_headings` routing for
        epub.
      Re-verify against the epic's actual landed shape first, in case the vendored tree has
      moved.
- [ ] Confirm the wire-format cut feeds Pandoc's EPUB writer correctly for chapter
      splitting — render a multi-heading fixture, confirm chapter boundaries land where
      `epub-chapter-level` says they should.
- [ ] Confirm crossref/numbering (P3/P6) renders sanely through Pandoc's EPUB writer,
      exercising the `isEpubOutput()` branch above — render a document with a figure/table/
      equation crossref through `--to epub`, unzip the result, and inspect the generated
      XHTML.

### Phase 2 — Golden tests & docs
- [ ] Golden-test methodology: EPUB **is** a zip container (like docx/pptx), so P7's
      unzip-and-walk approach may be directly adaptable — evaluate whether to reuse/extend
      it (unzip → walk the chapter XHTML files → insta-snapshot) rather than inventing a
      third methodology from scratch. epub's payload is already plain XHTML (no OOXML to
      walk into a semantic tree first), which may need a lighter touch — e.g. snapshot the
      extracted XHTML text directly.
- [ ] `docs/` page for the epub format (usage-focused).
- [ ] End-to-end verification per the repo's standing rule: render a real `.qmd` fixture
      (including a callout and a tabset, to exercise Phase 1's ported renderers, plus a
      crossref) to `--to epub`, open the resulting `.epub` (unzip and inspect the
      XHTML/OPF/nav, or open in an actual e-reader/preview app), and record the invocation +
      what was inspected.
- [ ] Full workspace verification per this repo's standing rules: `cargo xtask verify`
      (full, since this touches `quarto-core`) before any push.
