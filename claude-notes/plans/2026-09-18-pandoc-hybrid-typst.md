# Plan: typst output format (pandoc-hybrid-writer follow-on)

**Date:** 2026-09-20
**Status:** This plan's Phase 1 core wiring may target `feature/pandoc-writer-hybrid` (the
epic's integration branch) directly, in parallel with the epic's remaining P5/P6/P7 work — it
does not need to wait for a `main` merge. The crossref/numbering verification bullet at the end
of Phase 1 still needs P3/P6 landed first.
**Design (authoritative, epic-side):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)
**Research (this follow-on):** [`../research/2026-09-18-typst-epub-pandoc-q1-inventory.md`](../research/2026-09-18-typst-epub-pandoc-q1-inventory.md) — Part 1 (pandoc's Typst writer), Part 3 (Q1's typst format), Part 5 (Q2's binary-discovery infra + the compile step's real shape). Read Part 5 in full before starting Phase 2 — the compile step is substantially more than "shell out to `typst compile`".
**Sibling follow-on:** [`2026-09-18-pandoc-hybrid-epub.md`](2026-09-18-pandoc-hybrid-epub.md) — independent, no shared implementation work beyond the epic itself.
**Before starting:** re-verify file/section references against the epic's current landed
state — plan docs and the epic may have moved since this was drafted.

## Overview

Add `typst` as a Q2 output format by reusing Pandoc's own Typst writer plus vendored
Quarto 1 Lua filters and templates — the pattern the epic establishes for docx/pptx — with
one structurally new piece: **typst is the first Pandoc-hybrid format that needs a
post-Pandoc compile step** (Pandoc emits `.typ` source; turning that into a PDF means
invoking a `typst` compiler, plus real orchestration around it — see Part 5 of the research
note). Target is **full Q1 feature parity** (brand/CSS mapping, margin notes, template
partials, vendored packages/fonts) as the end state — the cost of vendoring Q1's existing,
working Lua wholesale is not meaningfully different whether the target is a trimmed subset or
the whole thing, so there's no reason to ship a permanently smaller feature set.

## Decisions

- Typst output **compiles to PDF** via a new post-Pandoc stage that invokes the `typst`
  compiler — not `.typ`-source-only.
- v1 targets **full Q1 parity**, vendored together (see Vendoring approach below), not split
  across a basic/advanced boundary.
- The `typst` binary is discovered via **PATH / `QUARTO_TYPST` only** — no `quarto install
  typst` auto-installer in this plan. `BinaryDependencies.typst` already does this
  discovery (`crates/quarto-core/src/render.rs:133,150-158`); it's just unconsumed today.
  Because Q2 doesn't bundle a known-good typst the way Q1 does, **version validation must
  run unconditionally** (Q1 only validates when its bundled default is overridden).
- **Package staging uses `typst-gather` as a linked Rust dependency, not a second bundled
  binary.** `typst-gather` (`quarto-dev/typst-gather`, checked out at
  `/Users/gordon/src/typst-gather`) already exposes a library API (`analyze()`,
  `gather_packages()`, `Config` in `src/lib.rs`) behind a thin CLI wrapper. Depend on the
  crate directly and call it in-process from the compile stage, rather than porting Q1's
  "shell out to a second binary" architecture (which exists only because Q1 is
  TypeScript/Deno and cannot link Rust).

## Vendoring approach

Vendor everything (all 7 Lua files, all 8 template files, all 5 packages + 3 fonts) in one
phase before attempting any compile. `definitions.typ` — part of the "basic" template — does
an **unconditional** `#import "@preview/marginalia:0.3.1"` and defines `callout()` using
symbols (`equation-numbering`, `callout-numbering`, etc.) that live in `numbering.typ`,
another partial. There is no version of "the template" that doesn't need the marginalia
package staged and the full partial set present. "Phased for review size, not scope" means
grouping the vendoring work into reviewable sub-chunks *within* Phase 1 (e.g. one commit for
Lua, one for templates, one for packages/fonts), not gating capability behind a compile-step
checkpoint that can't actually be reached partway through.

The 7 typst Lua files are **already vendored wholesale by the epic itself** (P4 Task 1) — see
Phase 1's Lua bullet below, which is a verification task, not a copy task. The two vendoring
tasks that remain real are templates (8 files) and packages/fonts (5 + 3).

**Explicitly out of scope** (carried forward from epic-wide limitations, not fixed here):
- **Book-project support.** Design doc §13 notes typst/latex are Q1's real "book" output
  formats, but Q2 has no book-project support at all today — that's a separate, later
  epic's problem, not this plan's.
- **Section numbering** (`number-sections`, `@sec-` refs) — no Q2 implementation for *any*
  format (`bd-5aklrxgi`); this plan inherits that gap unchanged. (Typst's own
  `section-numbering: "1.1.a"` metadata key, set by `format-typst.ts` when
  `number-sections` is on, is ported as a value passthrough — it doesn't require Q2 to have
  implemented section numbering itself.)
- **Crossref presentation options on Q2's native HTML writer** (`fig-prefix`,
  `title-delim`, etc. are honored by Q1's Lua — hence "free" here — but not by Q2's native
  HTML writer; tracked as `bd-wqdi1pd2`, unrelated to this plan).
- **`quarto install typst`** — see Decisions above.

## Checklist

### Phase 0 — Preconditions
- [ ] Confirm `2026-09-20-pandoc-hybrid-P7-foundation.md` has landed — it owns the `render.rs`
      gate-arm mechanism, `render_qmd_to_pandoc` routing, and the multi-format/project-mode
      guardrails Phase 1 bullet 1 below depends on. P7-foundation depends only on P1/P2/P4, so
      check it independently rather than assuming it tracks P7's own (later) completion.
- [ ] Confirm the epic has landed through at least P1 (neutral core), P2 (wire schema), P4
      (run machinery), P5 (Lua shim), P3 (upstream crossref split), and P6 (numbering
      wiring) — this plan's Phase 1 depends on the Pandoc-write stage,
      `BinaryDependencies::discover`, the vendored filter tree layout
      (`resources/pandoc-filters/`), and the shim's Route R/N dispatch all existing and
      working for at least one format (docx or pptx), for its core-wiring bullets. Core
      wiring (writer registration, filter params, meta mapping, execute defaults, Lua/template/
      package vendoring) does not need P3/P6; only the crossref/numbering verification bullet
      at the end of Phase 1 does — if P3/P6 haven't landed yet, everything else in Phase 1 can
      still proceed, but stop before that one bullet.
- [ ] Re-read this plan against the epic's *actual* landed shapes before starting — names,
      module locations, and the `PipelineProfile` variant shape may have shifted during
      implementation. Treat every code reference below as best evidence, not a guarantee.

### Phase 1 — Writer wiring + full vendoring (no compile step yet)

*Core wiring:*
- [ ] `FormatIdentifier::Typst` **already exists** (`crates/quarto-core/src/format.rs:33`,
      `as_str` at `:50`, `TryFrom<&str>` at `:90`) — no enum work needed. The actual gate is
      the native-format allow-match at `crates/quarto/src/commands/render.rs:680-684`
      (`is_native()` defined at `format.rs:61-63`); add a `Typst` arm there. **Depends on
      `2026-09-20-pandoc-hybrid-P7-foundation.md`** for the actual admit-list shape, the
      `render_qmd_to_pandoc` routing, and the guardrails (multi-format warning, project-mode
      containment) that relaxing this gate requires — P7-foundation depends only on P1/P2/P4
      (already landed), so it is a real, concrete, near-term dependency. This bullet's
      implementation may land directly on `feature/pandoc-writer-hybrid` alongside it.
- [ ] Write the invocation builder: `--to typst`, `standalone: true`,
      `default-image-extension: svg`, `wrap: none`, `citeproc: false` (+ the `-citations`
      variant when the user opts into Pandoc's native citeproc, per `format-typst.ts`).
      **Design the `extractTypstFilterParams` contributor from scratch** — it is a row in
      a survey table in the epic's P4 plan ("typst-specific | No — typst only"), not an
      existing code seam to wire. It needs to supply, at minimum: the serialized brand
      params (see brand bridge below). **Leave `typst-available-fonts` unset in Phase 1** —
      its Lua consumer (`filters/modules/typst_css.lua:681`'s `ensure_available_fonts` /
      `init_available_fonts`) fails open when the param is unset: the guard at
      `typst_css.lua:697` (`if not _available_fonts or ...`) short-circuits permissive, so
      every font name passes through unfiltered until Phase 2 wires the real `typst fonts`
      discovery. This is working-as-designed degradation — call it out explicitly in Phase
      1's golden tests so an unfiltered font name in a snapshot isn't mistaken for a
      regression. **Drop `quarto-environment.paths.Typst` from this list entirely** — zero
      Lua filters in Q1's `src/resources/filters/` tree read `paths.Typst` (it's set
      generically alongside `Rscript`/`TinyTexBinDir` in `quartoEnvironmentParams()`,
      `command/render/filters.ts:200-208`, but nothing consumes the `Typst` entry); not
      load-bearing for parity. Also design `extractColumnParams`/`quartoColumnParams`
      (another unplumbed P4 table row, "HTML/typst margin-notes feature") — Phase 1's
      margin-notes vendoring needs it.
- [ ] Meta-block mapping: Q2's normalized title/date/authors → Typst's `Meta` conventions,
      matching the `typst-show.typ` / `article()` parameter shape.
- [ ] Format-specific `execute` defaults consumed by `EngineExecutionStage`:
      `default-image-extension: svg`, `wrap: none`. These two need to be resolved before code
      execution (they affect how the engine renders figures), so `EngineExecutionStage` is the
      right place for them.
- [ ] `section-numbering: "1.1.a"` when `number-sections` is set — this is a pure metadata-flag
      check (`format-typst.ts:82-90`), no AST dependency, safe to compute anywhere before the
      wire cut.
- [ ] **`shift-heading-level-by: -1` when the document has no level-1 heading
      (`format-typst.ts:92-100`) — compute this from the fully resolved AST immediately
      before the wire-format cut, not from `DocumentProfile.outline` and not inside
      `EngineExecutionStage` itself.** Q1's own check (`hasLevelOneHeadings`,
      `core/lib/markdown-analysis/level-one-headings.ts`) runs a dedicated Pandoc+Lua-filter
      subprocess over the **final markdown after code execution**, so a heading emitted by
      an executed code cell (e.g. `results: asis` printing `# Section`) counts. Q2's
      `DocumentProfile.outline` (`crates/quarto-core/src/document_profile.rs`) is extracted
      by `DocumentProfileStage`, which the pipeline runs *before* `EngineExecutionStage`
      (`crates/quarto-core/src/pipeline.rs:225-233`) — per the profile contract's own
      invariant ("profiles are read-only; a feature needing state not yet in the profile
      should move its producer earlier, not back-patch," see
      `claude-notes/designs/document-profile-contract.md`), consuming
      `DocumentProfile.outline` for this decision would silently misclassify any document
      whose only level-1 heading is produced by executed code. Implement as a local
      heading-level scan over the final `Block` list immediately before the wire-format cut,
      not a `DocumentProfile` consumer.
- [ ] Pandoc-defaults forwarding allow-list — **name the actual keys**: at minimum
      `template` (custom template path — register in `FORMAT_PATH_KEYS` per
      `claude-notes/designs/path-resolution-model.md`), `pdf-standard`, `font-paths`,
      `columns`. Each forwarded key needs its own test per P7's methodology.
- [ ] Math delivery: the epic's design doc freezes `Equation` as Route N ("a plain filter
      that RawInline-wraps the existing `Math` inline") — the wire cut hands Pandoc real
      unresolved `Math` inlines for the typst leg, which Pandoc's typst writer renders itself
      via `texmath`. This is a one-line confirmation against P1's landed code, not open
      design work.

*Lua and template vendoring (do this as one coherent unit — see Vendoring approach above):*
- [ ] **The 7 typst Lua files are already vendored — nothing to copy.** Verified
      byte-identical in the epic's `resources/pandoc-filters/filters/` (landed by the epic's
      P4 Task 1, commit `ef0e30020`): `filters/quarto-post/typst.lua` (330 lines — margin
      notes, DPI/alt-text fixes, `#align()`, `typst:no-figure` marking),
      `filters/layout/typst.lua` (385 lines — wideblock/margin-figure/panel layout),
      `filters/modules/typst.lua` (112 lines), `filters/modules/typst_css.lua` (844 lines),
      `filters/quarto-finalize/typst.lua` (29 lines, including its own `-- FIXME finish this`
      gap — already there verbatim), `filters/quarto-post/typst-brand-yaml.lua` (379 lines),
      `filters/quarto-post/typst-css-property-processing.lua` (337 lines). **This item is:
      confirm each file is reachable through the shim's Route R/N dispatch for the `typst`
      `FormatIdentifier`** — re-verify against the epic's actual landed shape first, in case
      the vendored tree has moved.
- [ ] **Investigate the brand bridge correctly**: the parser to serialize from is the
      `crates/quarto-brand` crate (`ResolvedBrand`, `BrandFont`) plus `crates/quarto-sass`'s
      `brand_to_layers` — **not** `crates/quarto-core/src/brand_fonts.rs` (that's narrowly
      about publishing `source: file` fonts, a different concern). The task is
      schema-matching `ResolvedBrand` into the `brand` filter-param shape
      `typst-brand-yaml.lua`/`typst-css-property-processing.lua` expect — brand parsing
      itself already exists, this is a bridge, not a new parser.
- [ ] Vendor all 8 template files from
      `src/resources/formats/typst/pandoc/quarto/`: `template.typ` (orchestrator),
      `numbering.typ`, `definitions.typ`, `typst-template.typ`, `page.typ`,
      `typst-show.typ`, `notes.typ`, `biblio.typ`. **Confirmed genuinely not vendored
      anywhere yet** (unlike the Lua files above — no `.typ` files exist in the repo).
      **Implement the delivery mechanism**: Pandoc has no `--partial`
      flag, so materialize all 8 files into one directory at render time (or from a fixed
      resource location) and pass `--template` pointing at `template.typ`. **Follow the
      existing embed-and-materialize pattern** rather than inventing a new one:
      `crates/quarto-core/src/pandoc_filters/{mod.rs,bundle.rs,harness.rs}` already solves
      "ship a Q1 resource tree, materialize it to disk at render time" for the Lua filters
      via `include_dir!`; extend the `vendored-pandoc-filters` lint rule (or add a sibling
      rule) to cover this new subtree's pin bookkeeping. Verify against `harness.rs`'s
      actual `pandoc` invocation shape whether the template staging directory has any
      positional relationship to the `<share>/filters/` + `<share>/pandoc/datadir/` sibling
      layout the Lua filters require — likely independent (different Pandoc mechanisms:
      `--template` vs. the Lua filter search path) but don't assume.
- [ ] Vendor the 5 Typst packages Q1 bundles (fontawesome, marginalia, octique, showybox,
      theorion) and the 3 embedded Font Awesome fonts into a local, version-controlled
      directory (e.g. `resources/typst-packages/`, mirroring `resources/scss/README.md`'s
      Source/Updating pattern) — copy from `external-sources/quarto-cli`, never reference
      `external-sources/` at runtime (`external-sources-in-macro` lint will catch a
      violation). **Marginalia specifically must always be staged** — `definitions.typ`
      imports it unconditionally, regardless of whether the document uses margin notes.
      **Do not stage these via `typst-gather`'s `Config.local` at compile time — see Phase
      2's package-staging bullet: the destination directory it produces
      (`local/{name}/{version}`) does not match what a `@preview/...` import resolves
      against.** This bullet is about vendoring the bytes into the Q2 repo; Phase 2 decides
      how those bytes reach the compiler's package-path at render time.
- [ ] Vendor the font-availability filtering workaround (issue #12556) — this needs the
      `typst-available-fonts` filter param (see Phase 2's `getAvailableTypstFonts`
      equivalent); confirm against whatever Typst version this plan targets, since the
      workaround is Typst-version-sensitive and may already be obsolete.

*Verification (deliberately deferred here — see next item):*
- [ ] Confirm the crossref/numbering machinery (P3/P6) produces correct Typst output. The
      real mechanism is Q1's own Lua — `crossref/refs.lua:89-91`
      emits `RawInline('typst', '#ref(<label>, supplement: [...')` directly; Pandoc's own
      `reference-type` handling is fed only by its LaTeX *reader* and never applies to
      Q2's input. The shim's Route-R reconstruction must produce this `RawInline` path.
      This check is **only reachable once the full template (including `numbering.typ`) is
      staged** — don't attempt it before the vendoring above is complete.
- [ ] Confirm `crossref/equations.lua`'s `isTypstOutput()` branch (`:115`) actually engages
      — equation numbering should use Typst's native `#math.equation(numbering:
      equation-numbering)`, consuming the symbol `numbering.typ` defines, not a generic
      `(N)` text fallback. Same staging precondition as above.
- [ ] Golden-test methodology: typst emits plain text (`.typ`), not binary/XML like
      docx/pptx — P7's unzip-and-walk-XML approach does not apply. Use direct
      text-diff/insta-snapshot on the `.typ` source for this phase (compile-to-PDF checks
      are Phase 2's problem).

### Phase 2 — Compile step

This is substantially more than "shell out to `typst compile`" — see the research note's
Part 5 table in full before starting.

- [ ] Add a new pipeline stage (name TBD, e.g. `TypstCompileStage`) that runs after the
      Pandoc-write stage produces `.typ` + staged resources (from Phase 1), and invokes
      `typst compile` to produce the final PDF. Follow the established subprocess pattern
      (`crates/quarto-core/src/engine/knitr/subprocess.rs`: cached binary discovery,
      `Command::new` + `Stdio::piped()` + `.spawn()` + `.wait_with_output()`).
- [ ] Consume the existing `BinaryDependencies.typst` field
      (`runtime.find_binary("typst", "QUARTO_TYPST")`) — already implemented, just unread
      today.
- [ ] Pass the flags Q1's `typstCompile` passes: `--root` (**exactly Q2's leading-`/`-means-
      project-root convention** — omitting this breaks every project-root-relative image or
      brand logo), `--package-path`/`--package-cache-path` (pointed at wherever
      `typst-gather`'s staged output lands — see below), `--pdf-standard`, `--font-path`
      (ordering matters: Quarto's own font paths must come first, per `fontPathsArgs`).
- [ ] **Run unconditional typst version validation** (min `>=0.8`, matching
      `validateRequiredTypstVersion`) — unconditional because, unlike Q1, Q2 has no bundled
      known-good binary to skip validation for.
- [ ] **Package staging via `typst-gather` as a linked crate, not a subprocess.** Add
      `typst-gather` as a Cargo dependency and call `analyze()`/`gather_packages()`
      in-process before invoking `typst compile`, so packages (marginalia at minimum, plus
      anything else the document's Lua-emitted Typst references) are staged locally before
      compilation — otherwise `typst compile` attempts a network fetch. This is a design
      change from Q1's architecture (which shells out to `typst-gather` as a second bundled
      binary only because Q1 can't link Rust); confirm `typst-gather`'s current public API
      is stable enough to depend on directly, and file any refactoring `typst-gather` itself
      needs as its own small piece of work in that repo.
      **Two concrete usage traps, verified against `typst-gather`'s source — both fail
      silently (wrong output or a live network call) rather than erroring:**
      - **Point `Config.discover` at the whole staged template directory** (all 8 files),
        not just the single `.typ` file Pandoc emits. `analyze()`/`discover_imports` scan a
        given directory non-recursively, reading each file's own text for
        `#import`/`#include` statements — they do not follow `#include` edges. Marginalia's
        import lives in `definitions.typ` (a partial), not in Pandoc's own output; pointing
        discovery at only the latter silently misses it.
      - **Pre-seed the package-cache destination with Quarto's 5 vendored packages' bytes
        under `preview/{name}/{version}/` before calling `gather_packages`/`analyze`, or copy
        them there directly and skip `typst-gather`'s entry types for these 5 specifically.**
        Do **not** register them via `Config.local` — `gather_local()` hardcodes its
        destination to a `local/{name}/{version}` subdirectory, which a `@preview/...` import
        will never resolve against. Reserve `typst-gather`'s own discover/fetch machinery for
        packages beyond Quarto's bundled 5 (arbitrary `@preview` packages a user's own
        document might reference) — that's its actual value-add here.
- [ ] **WASM-gate the `typst-gather` dependency and the new compile-stage module.** Add
      `typst-gather` under `crates/quarto-core/Cargo.toml`'s existing
      `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` block, and gate the compile
      stage's module with `#![cfg(not(target_arch = "wasm32"))]`, mirroring
      `crates/quarto-core/src/engine/knitr/mod.rs:29`. `wasm-quarto-hub-client` compiles
      `quarto-core` to `wasm32-unknown-unknown`; `typst-gather`'s dependencies (`typst-kit`,
      `typst-syntax`) are native crates (font enumeration, package HTTP fetching) with no
      expectation of wasm32 support. Unconditionally adding this dependency would risk
      breaking the hub-client WASM build.
- [ ] Implement `getAvailableTypstFonts`-equivalent font discovery: a `typst fonts`
      subprocess invocation, cached, feeding the `typst-available-fonts` filter param
      (needed by the #12556 font-fallback workaround vendored in Phase 1).
- [ ] Decide and implement the missing-binary / compile-failure diagnostic. The epic's own
      P4 plan commits to a new `pandoc` error-catalog subsystem for pandoc binary-missing/
      nonzero-exit. Decide explicitly whether typst compile failures get their own
      `Q-<n>-*` subsystem or fold into the `pandoc` one as a distinct case. Whichever is
      chosen, follow the epic's process: `docs/errors/<subsystem>/` page(s) in the same
      commit (`error-docs-page-missing`) and a sidebar entry
      (`error-docs-sidebar-unlisted`).
- [ ] Capture `typst` compiler stderr unconditionally (mirroring the epic's decision for
      `pandoc`'s stderr).
- [ ] Decide and document: does Q2 keep the intermediate `.typ` file by default? Q1 already
      ships `keep-typ` (`kKeepTyp`, `config/constants.ts:88`), forced on in debug mode
      alongside `keep-tex`, discarded otherwise. Port that default directly.
- [ ] End-to-end verification per the repo's standing rule: render a real `.qmd` fixture
      (including a callout, a figure crossref, and an equation crossref — reachable now
      that Phase 1 staged everything) to `--to typst`, inspect the actual PDF, and record
      the invocation + a description of what was inspected in this plan or the session
      transcript.

### Phase 3 — Docs & polish
- [ ] `docs/` page for the typst format (usage-focused, per the repo's docs/ convention —
      not technical internals).
- [ ] Audit the Q1-cited upstream Pandoc bugs (image alt-text #11394, width/height units
      #9945, table-in-figure #10438 — all in `filters/quarto-post/typst.lua`; Skylighting
      block styling #14126 in `format-typst.ts`'s postprocessor) against **the epic's
      pinned minimum pandoc version (3.10, per P4)**, not whatever pandoc happens to be
      installed locally. Drop any workaround the 3.10 floor already fixes rather than
      porting dead code.
- [ ] Full workspace verification per this repo's standing rules:
      `cargo xtask verify` (full, since this touches `quarto-core`) before any push.
