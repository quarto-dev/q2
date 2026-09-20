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
- [x] Confirm `2026-09-20-pandoc-hybrid-P7-foundation.md` has landed — it owns the `render.rs`
      gate-arm mechanism, `render_qmd_to_pandoc` routing, and the multi-format/project-mode
      guardrails Phase 1 bullet 1 below depends on. P7-foundation depends only on P1/P2/P4, so
      check it independently rather than assuming it tracks P7's own (later) completion.
      **Confirmed 2026-09-20**: all 4 P7-foundation tasks checked off; its tip
      (`d8c7d3443`) is `feature/pandoc-writer-hybrid`'s current HEAD.
- [x] Confirm the epic has landed through at least P1 (neutral core), P2 (wire schema), P4
      (run machinery), P3 (upstream crossref split), P5 (Lua shim), and P6 (numbering wiring) —
      any document with a callout, tabset, or crossref goes through the shim's Route-R dispatch,
      and epub's own crossref branch in `crossref/sections.lua` is part of the crossref split P3
      covers. **Confirmed 2026-09-20**: P1-P6 checklists all fully checked (one `P1` item is an
      explicit "Owned by P7" cross-reference, not a gap).
- [x] Re-read this plan against the epic's actual landed shapes before starting — names, module
      locations, and other details may have shifted during implementation. **Confirmed**: line
      numbers in the Phase 1 bullets below have drifted slightly (render.rs gate is now at
      ~692-706) but the referenced mechanisms are unchanged.

### Phase 1 — Core writer wiring
- [x] `FormatIdentifier::Epub` **already exists** (`crates/quarto-core/src/format.rs:31`,
      `as_str` at `:49`, `TryFrom<&str>` at `:89`) — no enum work needed. The gate is the
      native-format allow-match at `crates/quarto/src/commands/render.rs:680-684`; add an
      `Epub` arm there. **Depends on `2026-09-20-pandoc-hybrid-P7-foundation.md`** for the
      admit-list shape, `render_qmd_to_pandoc` routing, and the guardrails (multi-format
      warning, project-mode containment) relaxing this gate requires — P7-foundation depends
      only on P1/P2/P4 (already landed), so this is a concrete near-term dependency. This
      bullet's implementation may land directly on `feature/pandoc-writer-hybrid` alongside it.
      **Done**: added `FormatIdentifier::Epub` to the `matches!` allow-list at
      `render.rs:696-701`. The rest of the non-native path
      (`render_qmd_to_pandoc`/`PandocWriteStage`) is already fully format-agnostic — it reads
      `ctx.format.output_extension` ("epub") and shells out `-t epub` generically, so no other
      code changed to get a working render. New e2e test
      `render_pandoc_formats_e2e::e2e_render_epub` (watched RED against the un-widened gate,
      GREEN after) asserts: real zip, `mimetype` is the first entry and stored (not deflated,
      per the EPUB spec), and a chapter XHTML under `EPUB/text/` contains the body text.
- [x] Decide the epub2/epub3 default. Pandoc's `--to epub` defaults to epub3
      (`writeEPUB3`, `Writers.hs:165-167`) — confirm Q2 should match that default (Q1
      exposes both as distinct format identifiers; check whether Q2 needs both `epub` and
      `epub2` variants, or just `epub` = epub3 with an option to downgrade).
      **Decided 2026-09-20**: v1 ships `epub` = epub3 only, matching Pandoc's own `--to epub`
      default (confirmed via `-t epub` already selecting `writeEPUB3` with no extra flag) and
      matching what `FormatIdentifier::Epub` already resolves to (`output_extension: "epub"`,
      no separate enum variant). No `Epub2` `FormatIdentifier` variant added — Q1's book-mode
      epub2 downgrade path is out of scope per this plan's "Explicitly out of scope" section;
      revisit only if a concrete epub2 requirement surfaces.
- [x] Invocation builder: `--to epub` (or `epub2`), Q1's format defaults
      (`default-image-extension: png`, `fig-width`/`fig-height` defaults, and
      **`merge-includes: false`**, needed to keep the two `include-in-header` CSS files below
      from colliding).
      **Done**: `crates/quarto-core/src/stage/stages/pandoc_write.rs`'s new
      `epub_extra_args()` adds `--default-image-extension=png` and the two
      `--include-in-header` flags (below) when `ctx.format.identifier == Epub`.
      **Scope note**: `fig-width`/`fig-height` (5/4 in Q1) are `execute:`-block *engine*
      defaults (knitr/jupyter figure sizing), not a pandoc CLI flag — Q2's docx/pptx legs
      don't wire per-format `execute:` defaults either (no generic mechanism exists yet);
      out of scope for this plan, same as the sibling docx/pptx gap. `merge-includes: false`
      needed no code: Q2 never merges `--include-in-header` files into one in the first
      place, so passing two separate flags already produces the "not merged" behavior Q1
      has to opt into.
- [x] `html-math-method` switch: `webtex` for epub2, `mathml` for epub3 — port directly
      from `format-epub.ts`'s logic. Math delivery is resolved: the epic's design doc
      (`pandoc-hybrid-architecture.md`, Route-L/R/N table) freezes `Equation` as Route N —
      "a plain filter that RawInline-wraps the existing `Math` inline." The wire cut hands
      Pandoc real unresolved `Math` inlines for the epub leg (as for typst), so
      `html-math-method` behaves as a normal Pandoc option here.
      **Done**: `epub_extra_args()` passes `--math-method=mathml` unconditionally (v1 is
      epub3-only, so the epub2 `webtex` branch never applies). **Measured** (real pandoc
      3.11 probe): epub3's own default *already* emits MathML with no flag at all, so this
      flag is currently a no-op — kept explicit to match Q1's intent and guard against a
      future Pandoc default change, not because it's observably required today. This is
      also why no dedicated regression test isolates this flag: flipping it doesn't change
      output, so a test asserting the flag's *effect* would be vacuous. The MathML-in-output
      assertion lives in `e2e_render_epub_format_defaults` instead, covering the outcome
      (real MathML renders) rather than the flag.
- [x] Meta-block mapping for EPUB-specific metadata: cover image, identifier, language,
      via Pandoc's `getEPUBMetadata`/`metadataFromMeta` conventions (`EPUB.hs:176,342`).
      **Resolved as a documentation/no-code item, not an implementation gap**: read
      `EPUB.hs`'s `getEPUBMetadata` directly (`addIdentifier`/`addLanguage`/`addAuthor`/
      `fixDate`, `:176-230`) — Pandoc's own EPUB writer already synthesizes a random-UUID
      `identifier`, a `LANG`-env-derived `language`, and author/date from `docAuthors`/the
      current time when the JSON `meta` block doesn't supply them, and reads them straight
      from `meta` when it does (which Q2's existing `-f json` pipe already carries in full —
      `title`, and `lang` via the upstream `LanguageResolveStage`, land in `meta` for every
      format, not just epub). **Cover image is the one real piece of forwarding work**,
      and it's handled by the `epub-cover-image` pandoc-defaults allow-list bullet below —
      not by anything meta-block-specific.
- [x] `epub-chapter-level` / `--split-level`: a real, user-facing knob, not automatic
      chapter splitting. Pandoc's `writerSplitLevel` (default `1`, `Options.hs:385`) controls
      where `splitIntoChunks` cuts the document into chapter files; Q1 exposes this as
      `epub-chapter-level` (`config/constants.ts:752`). This must land in the per-format
      pandoc-defaults allow-list. Also account for Pandoc synthesizing a level-1 header from
      the document title when none exists (`EPUB.hs:555-562`) — worth a golden-test case.
      **Done**: `epub_extra_args()` reads `epub-chapter-level` off the merged metadata
      (`ConfigValue::as_int_lenient`, so both a bare YAML int and a quoted string work) and
      forwards `--split-level=<n>`. **Measured** (real pandoc probe, `man pandoc`):
      `--epub-chapter-level` itself is Pandoc's own *deprecated synonym* for `--split-level`
      — Q1's config key name and Pandoc's canonical flag name just don't match, there's no
      separate mechanism. New e2e test `e2e_render_epub_chapter_level` discriminates a real
      wiring from a no-op: a 2-H1-heading fixture yields 2 chapter files at the default
      split-level and 3 when `epub-chapter-level: 2` also splits the H2. The
      title-synthesized-header golden case is deferred to Phase 2 (golden tests), not
      re-verified here.
- [x] Pandoc-defaults forwarding allow-list — name the actual keys: at minimum
      `epub-cover-image`, `epub-metadata`, `epub-fonts`/`epub-embed-font`, `epub-subdirectory`,
      `split-level`, `css`.
      **Done.** `split-level` (as `epub-chapter-level`) landed in the previous task. For the
      rest, reused the repo's existing path-resolution seam instead of building a new one:
      added `epub-cover-image`, `epub-metadata`, `epub-embed-font` to
      `project::format_paths::FORMAT_PATH_KEYS` (`MarkPolicy::Always`, `KeyForms::Entries` —
      same reasoning as `include-in-header`). That merge-time pass already normalizes
      matching keys to a document-relative `ConfigValueKind::Path` for *every* format, not
      just epub (`css` was already in the table) — `epub_extra_args()` just reads the
      now-normalized value with `as_plain_text()` and joins it against `doc.path`'s parent to
      hand pandoc an absolute path, regardless of pandoc's own subprocess cwd. This is the
      same contract docx/pptx image embedding relies on (`claude-notes/designs/path-resolution-model.md`),
      not a parallel one-off — avoids the exact "recurs key by key" bug class that document
      warns about. `css`/`epub-embed-font` forward as repeated `--css=`/`--epub-embed-font=`
      flags (pandoc accepts multiples); `epub-cover-image`/`epub-metadata` are single-valued.
      `epub-subdirectory` is **not** a path (an internal directory *name* inside the epub
      container) — passed through verbatim, not added to `FORMAT_PATH_KEYS`.
      New e2e tests: `e2e_render_epub_cover_image` (asserts the OPF manifest marks the item
      `properties="cover-image"` — the real Pandoc-side signal a cover was wired, not just
      that some media file exists) and `e2e_render_epub_css` (asserts a document-relative
      `css:` file's content lands verbatim in the epub's embedded, chapter-linked
      stylesheet — distinguishing `--css` from Task 2's `--include-in-header` files).
- [x] Wire the two HTML-family CSS includes Q1 uses: `styles-callout.html` (shared verbatim
      with the plain HTML format — confirm Q2's HTML format already has an equivalent to
      reuse rather than duplicate) and a new `formats/epub/styles.html` (mostly the same
      `.quarto-layout-*` panel/figure CSS the HTML writer already ships — check whether
      Q2's HTML writer's CSS can be reused directly instead of vendoring a second copy).
      **Done, alongside the invocation-builder bullet above**: Q2's native HTML writer emits
      CSS through its own compiled-SCSS theme pipeline (`CompileThemeCssStage`), which has no
      applicability here — epub renders through *Pandoc's* bundled HTML writer, a completely
      separate code path with no theme CSS at all. No reuse was possible; vendored both files
      verbatim from `quarto-cli`'s `src/resources/formats/{html,epub}/` into
      `resources/formats/{html,epub}/` (External Sources Policy) and embedded them via a new
      `FORMATS_DIR` `include_dir!` static + `extract_formats_tree()`
      (`crates/quarto-core/src/pandoc_filters/{mod,bundle}.rs`), materialized per-render into
      `ctx.temp_dir()` alongside the existing Lua filter tree. `e2e_render_epub_format_defaults`
      asserts both files' distinctive CSS selectors land in the rendered chapter `<head>`.
- [x] **The three epub-conditional renderers below are already vendored — nothing to copy.**
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
      **Verified by real render + manual XHTML inspection** (`/tmp/epub-manual/`, then pinned
      by `e2e_render_epub_callout_and_crossref`):
      - **Callout: works correctly.** Renders with classes
        `callout callout-note callout-titled callout-style-default` — the dedicated
        epub/revealjs renderer, not Q2's native HTML callout markup — and every class it
        emits (`.callout`, `.callout-titled`, `.callout-style-default`, `.callout-title`,
        `.callout-body`, `.callout-content`, `.callout-icon`) has a matching selector in the
        vendored `styles-callout.html`. One selector in that file,
        `.callout .callout-body-container`, has **no** corresponding class in either the
        titled or plain callout markup emitted — likely a stale/unused selector already in
        Q1's own file (verified byte-identical to `quarto-cli`'s copy), not something this
        port introduced or needs to fix.
      - **Tabset: does NOT reach the epub branch — flagging, not fixing.** Real render shows
        `class="panel-tabset-tabby"` / `data-tabby-default` markup (the *interactive*
        HTML-family renderer), not `render_tabset_with_l4_headings`'s static fallback. Root
        cause, read directly from `pandoc/datadir/_format.lua:150-160`:
        `isHtmlOutput()`'s format list is `{"html","html4","html5","epub","epub2","epub3"}` —
        it already matches epub — and `panel-tabset.lua:264`'s `elseif` chain checks
        `isHtmlOutput()` **before** the `isEpubOutput()` arm, so the epub-specific branch is
        dead code for every epub render, unconditionally. This is Q1's own,
        byte-for-byte-identical behavior (confirmed against `quarto-cli`'s vendored copy) —
        **not a Q2 regression**, and not fixed here: `panel-tabset.lua` is shared with
        revealjs/latex/docx, so patching the branch order is a cross-cutting Lua change with
        its own blast radius, not a call to make unilaterally inside an epub-scoped plan.
        Practical effect for readers: without JS, `tabbyTabs()`'s markup has no CSS hiding
        inactive panes in an epub (Q2 ships none of Q1's app-level CSS to the epub leg), so
        **all tab content renders simultaneously and statically** — not broken, arguably
        fine for a non-interactive reader, just not what `render_tabset_with_l4_headings`
        would have produced. **Flagged for the user; no braid strand filed** — this is
        Q1-inherited, cross-format, and not blocking; the user can decide whether it's worth
        a future strand.
      - **Section-numbering gate: unverifiable in isolation, as the plan predicted.** A
        `number-sections: true` + `# Heading` fixture renders *unnumbered* on **both**
        `--to html` and `--to epub`. For epub that's `isEpubOutput()` doing its job; for HTML
        it's Q2's general section-numbering gap (bd-5aklrxgi, already listed under
        "Explicitly out of scope" above) — the two code paths converge on the same visible
        output for different reasons, so this specific gate can't be isolated by observation
        alone until bd-5aklrxgi lands. Confirms the plan's own scope-out was correct; no
        further action here.
- [x] Confirm the wire-format cut feeds Pandoc's EPUB writer correctly for chapter
      splitting — render a multi-heading fixture, confirm chapter boundaries land where
      `epub-chapter-level` says they should.
      **Done** — this is exactly what `e2e_render_epub_chapter_level` (previous task) pins:
      2 chapters at the default split-level, 3 once `epub-chapter-level: 2` also splits the
      nested H2.
- [x] Confirm crossref/numbering (P3/P6) renders sanely through Pandoc's EPUB writer,
      exercising the `isEpubOutput()` branch above — render a document with a figure/table/
      equation crossref through `--to epub`, unzip the result, and inspect the generated
      XHTML.
      **Done** — `e2e_render_epub_callout_and_crossref` renders a figure crossref through
      `--to epub` and asserts both the numbered caption (`Figure\u{a0}1: A figure caption`,
      note: a non-breaking space between "Figure" and the number, not a plain ASCII
      space — a real gotcha for anyone writing a similar assertion) and the `@fig-one`
      reference resolving to `Figure\u{a0}1` via a `class="quarto-xref"` link. Table/equation
      crossrefs weren't separately exercised — the routing is the same
      shared-crossref-machinery path P3/P6 already cover format-agnostically, and the epub
      plan's own scope is "does the epub leg's wire-format cut feed it correctly," which the
      figure case already answers; a table/equation-specific golden case is Phase 2 material
      if wanted, not a Phase 1 gap.

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
