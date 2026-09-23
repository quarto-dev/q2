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
- **Hierarchical/chapter-scoped numbering** ("Figure 1.3", "Figure A.2") — a genuine upstream
  quarto-cli gap, not a Q2-side problem to solve locally (cross-session finding,
  2026-09-20). Not even `orange-book` implements it for figures/tables: it wires
  `counter(heading)` into equation/callout/subfloat/theorem numbering but
  `floatreftarget.lua`'s `make_typst_figure` never passes a `numbering:` arg for plain
  figures/tables. Already covered by this plan's book-project scope-out above; the right fix
  if it ever matters is an upstream quarto-cli contribution (the epic vendors Q1's Lua
  verbatim per the architecture doc's §7 policy), not a local patch.

## Checklist

### Phase 0 — Preconditions
- [x] Confirm `2026-09-20-pandoc-hybrid-P7-foundation.md` has landed — it owns the `render.rs`
      gate-arm mechanism, `render_qmd_to_pandoc` routing, and the multi-format/project-mode
      guardrails Phase 1 bullet 1 below depends on. P7-foundation depends only on P1/P2/P4, so
      check it independently rather than assuming it tracks P7's own (later) completion.
      **Confirmed 2026-09-20** (see `CLAUDE.local.md`'s "Preconditions verified" note).
- [x] Confirm the epic has landed through at least P1 (neutral core), P2 (wire schema), P4
      (run machinery), P5 (Lua shim), P3 (upstream crossref split), and P6 (numbering
      wiring) — this plan's Phase 1 depends on the Pandoc-write stage,
      `BinaryDependencies::discover`, the vendored filter tree layout
      (`resources/pandoc-filters/`), and the shim's Route R/N dispatch all existing and
      working for at least one format (docx or pptx), for its core-wiring bullets. Core
      wiring (writer registration, filter params, meta mapping, execute defaults, Lua/template/
      package vendoring) does not need P3/P6; only the crossref/numbering verification bullet
      at the end of Phase 1 does — if P3/P6 haven't landed yet, everything else in Phase 1 can
      still proceed, but stop before that one bullet. **Confirmed 2026-09-20**: all landed
      directly on `feature/pandoc-writer-hybrid` as of `d8c7d3443`.
- [x] Re-read this plan against the epic's *actual* landed shapes before starting — names,
      module locations, and the `PipelineProfile` variant shape may have shifted during
      implementation. Treat every code reference below as best evidence, not a guarantee.
      **Done 2026-09-20** at the start of this worktree's session.

### Phase 1 — Writer wiring + full vendoring (no compile step yet)

*Core wiring:*
- [x] `FormatIdentifier::Typst` **already exists** (`crates/quarto-core/src/format.rs:33`,
      `as_str` at `:50`, `TryFrom<&str>` at `:90`) — no enum work needed. The actual gate is
      the native-format allow-match at `crates/quarto/src/commands/render.rs:680-684`
      (`is_native()` defined at `format.rs:61-63`).
      **The `Typst` arm landed in Phase 2 (2026-09-21)**, in the same change that added
      `TypstCompileStage` — `render.rs`'s allow-match now includes `Typst` alongside
      `Docx`/`Pptx`. See Phase 2's first bullet for detail.
      **Decision (2026-09-20, Gordon): do NOT add a `Typst` arm to this gate in Phase 1.**
      `output_extension_for(FormatIdentifier::Typst)` is (correctly) `"pdf"` — that's the
      real user-facing deliverable — but `PandocWriteStage` invokes pandoc with
      `-t <output_extension> -o <output_path>`, which for typst would mean `-t pdf`
      (nonsensical — there is no direct pandoc pdf writer reachable this way, and it would
      skip the typst Lua filters/template entirely). The pandoc *writer* name for typst is
      `"typst"`, distinct from the final `"pdf"` extension, and there is nowhere for the
      resulting `.typ` intermediate to go until Phase 2's `TypstCompileStage` exists to
      compile it to the real PDF at `ctx.output_path()`. Opening the CLI gate before that
      stage exists would make `q2 render --to typst` "succeed" while writing raw Typst
      source into a file named `.pdf` — a misleading artifact on the shared integration
      branch. **All other Phase 1 core-wiring bullets below (invocation builder, meta
      mapping, execute defaults, vendoring) are implemented and unit/golden-tested at the
      pipeline level** (calling `render_qmd_to_pandoc`/`PandocWriteStage` directly with a
      `Format::typst()`-like override, inspecting the `.typ` text they produce), **without**
      going through the CLI's `render.rs` gate. The `Typst` arm lands in Phase 2, in the same
      change that adds `TypstCompileStage`, once there's a real place for pandoc's `.typ`
      output to go.
- [x] Write the invocation builder: `--to typst`, `standalone: true`,
      `default-image-extension: svg`, `wrap: none`, `citeproc: false` (+ the `-citations`
      variant when the user opts into Pandoc's native citeproc, per `format-typst.ts`).
      **Done (2026-09-20) for the pandoc-CLI-flags half**:
      `Format::pandoc_invocation_args()` (`crates/quarto-core/src/format.rs`) appends
      `--standalone --wrap none --default-image-extension svg` for `FormatIdentifier::Typst`
      only, wired into `PandocWriteStage`'s `Command`. `citeproc: false` needed no code —
      Q2 never passes `--citeproc` to pandoc for any Pandoc-hybrid format today. **The
      `-citations` opt-in variant is deferred, not implemented** — there is no existing
      pseudo-format-variant seam (`builtin_pseudo_format` only maps whole-format aliases,
      not opt-in suffixes on a base format), so it needs its own design decision; ask
      Gordon before inventing one. **The generic `brand` key done (2026-09-20)**, under a
      corrected name — **`extractTypstFilterParams` turned out to be a real, narrower Q1
      function** (`command/render/filters.ts:947-955`: just `toc-indent`, `logo`,
      `css-property-processing`, `brand-mode`, `html-pre-tag-processing`, forwarded
      verbatim from `format.metadata`) — **`brand` itself is set generically for every
      Pandoc format** (`filters.ts:197`'s `[kBrand]: options.format.render[kBrand]`, part
      of the format-independent `quartoFilterParams` spread, not `extractTypstFilterParams`
      at all). What's actually implemented: `TypstFilterParamsContributor`
      (`crates/quarto-core/src/pandoc_filters/typst_params.rs`) wires the brand bridge's
      output into `PandocWriteStage::run` via `.with_contributor(...)`, typst-only (see
      that module's doc comment for why `brand` isn't core/format-independent in Q2 yet).
      `resolve_typst_brand_param` resolves the document's merged metadata's `brand:` key
      (light half only — dark is deferred, needing the fuller
      `ThemeConfig::resolve_variants` machinery `compile_theme_css` uses; out of scope
      since one typst render only ever reaches one `brand-mode` per PDF) and calls
      `build_brand_param`. End-to-end verified against real pandoc 3.11 +
      `typst-brand-yaml.lua`: an inline `brand: {color: {primary: "#1234ff"}}`
      frontmatter block comes out as a real `#let brand-color = (...)` declaration
      (`render_document_to_file_typst_emits_brand_color_from_inline_brand_block`), and the
      no-brand case is unchanged (`..._without_brand_emits_empty_brand_color_dict`).
      **The real `extractTypstFilterParams` (5 keys above) is still open** — deliberately
      not guessed at: `logo` needs the same deferred `resolveLogo`/`fillLogoPaths` family
      as the brand bridge's logo bullet (a document-level `logo:` spec, not brand data);
      the other 4 are simple metadata passthroughs but have **no current consumer or
      fixture exercising them** (no Q2 typst document sets `toc-indent`/
      `css-property-processing`/`brand-mode`/`html-pre-tag-processing` yet), so implementing
      the `ConfigValue`→JSON conversion for each without anything to verify it against
      risks silently-wrong shapes; do this once a real need (or at least a template feature
      that reads one) surfaces. **`extractColumnParams`/`quartoColumnParams`
      (`reference-location: margin`/`citation-location`, margin-notes) still open** —
      confirmed **format-agnostic** (`filters.ts:151`, spread into every Pandoc-hybrid
      format's params, not typst-specific), so implementing it belongs in
      `FilterParamsBuilder`'s core (`params.rs`), not a typst-only contributor; template
      vendoring landing doesn't change that scoping, just removes the "blocked on
      vendoring" reason for not starting. Distinct from `format-typst.ts`'s *other*
      `columns` handling (`format.pandoc[kColumns]` → `metadata[kColumns]`, genuinely
      typst-specific multi-column body layout) — the Phase 1 checklist's "Pandoc-defaults
      forwarding allow-list" bullet's `columns` entry means *that* one, not
      `quartoColumnParams`; still open, not yet investigated for a Q2 config-reading seam.
      **Leave `typst-available-fonts` unset in Phase 1** —
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
      load-bearing for parity.
- [x] Meta-block mapping: Q2's normalized title/date/authors → Typst's `Meta` conventions,
      matching the `typst-show.typ` / `article()` parameter shape.
      **Done (2026-09-20), turned out to need no new code.** `typst-show.typ` is a
      **Pandoc doctemplate**, not a Lua transform (`$if(title)$`/`$for(by-author)$`
      syntax) — it reads `title`/`date`/`abstract`/`by-author` straight out of the
      metadata pandoc's template engine already builds from `doc.ast.meta`. Q2 already
      populates `by-author` in exactly the shape `$for(by-author)$$it.name.literal$
      ...$it.affiliations$...$it.email$` expects — `AuthorsNormalizeTransform`
      (`crates/quarto-core/src/transforms/authors_normalize.rs`), a format-agnostic
      pipeline stage that runs before any Pandoc-hybrid branching, already writes
      `name.literal`/`affiliations[].name`/`email` (and `labels.abstract` for the
      abstract-title slot). End-to-end confirmed via
      `render_document_to_file_typst_uses_vendored_template` (title threads through to
      `article(title: [...])`). **By-author itself could not be end-to-end verified** —
      a `name`/`email`/`affiliation` (singular string) author entry crashes the render
      entirely, but **confirmed pre-existing and not typst-specific** (reproduces
      identically on `docx`); filed as `bd-ymkkrn64`, discovered-from this plan but out
      of its scope (general Pandoc-hybrid `author:` wire-shape infra, not a typst gap).
- [x] Format-specific `execute` defaults consumed by `EngineExecutionStage`:
      `default-image-extension: svg`. **Done (2026-09-20)**:
      `ExecuteConfig::with_defaults_for_format` (`crates/quarto-core/src/engine/knitr/
      format.rs`) sets `fig_format: "svg"` for typst, `"png"` otherwise; picked up
      automatically by `build_format_config`'s existing call site.
      **`wrap: none` turned out not to belong here** — nothing in knitr's
      `execute.R`/`hooks.R` reads `format.pandoc.wrap`; it's fully handled by the pandoc
      invocation builder above (a plan-drafting error in the original bullet, corrected
      after checking the R scripts directly).
- [x] `section-numbering: "1.1.a"` when `number-sections` is set — this is a pure metadata-flag
      check (`format-typst.ts:82-90`), no AST dependency, safe to compute anywhere before the
      wire cut. **Done (2026-09-20)**: `insert_typst_section_numbering`
      (`crates/quarto-core/src/stage/stages/pandoc_write.rs`) inserts into `doc.ast.meta`
      before JSON serialization, typst-only. End-to-end verified against real pandoc 3.11:
      `number-sections: true` forwards `"1.1.a"` all the way into pandoc's own default
      typst template's `sectionnumbering` variable (confirmed empirically before writing
      the test — no custom template needed for this verification).
- [x] **`shift-heading-level-by: -1` when the document has no level-1 heading
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
      not a `DocumentProfile` consumer. **Done (2026-09-20)**:
      `has_level_one_heading`/`shift_heading_level_by_for`
      (`crates/quarto-core/src/stage/stages/pandoc_write.rs`) recurse into every block
      container (Div/BlockQuote/lists/DefinitionList/Figure/Table/Custom slots), computed
      in `PandocWriteStage::run` over `doc.ast.blocks` and passed as `--shift-heading-
      level-by -1` to pandoc, typst-only. End-to-end verified against real pandoc 3.11:
      a level-2-only document shifts to typst's `=`; a level-1-present document doesn't
      shift; a heading nested inside a `Div`/`BlockQuote` is still found by the scan
      (unit-tested).
- [x] Pandoc-defaults forwarding allow-list — **name the actual keys**: at minimum
      `template` (custom template path — register in `FORMAT_PATH_KEYS` per
      `claude-notes/designs/path-resolution-model.md`), `pdf-standard`, `font-paths`,
      `columns`. Each forwarded key needs its own test per P7's methodology.
      **Resolved (2026-09-20), split three ways after reading Q1's actual source**
      (`defaults.ts`'s `generateDefaults`, `pandoc.ts:777-869`'s template/columns
      handling, `output-typst.ts`/`core/typst.ts`'s `typstPdfOutputRecipe`/
      `typstCompile` — not `format-typst.ts`, which has none of this):
      1. **`pdf-standard`/`font-paths` were never part of this mechanism at all** —
         confirmed by reading `output-typst.ts:245-257`: they're read from
         `format.render[kPdfStandard]`/`format.metadata[kFontPaths]` (plain
         document-level YAML keys, not `format.pandoc.*`), consumed entirely by
         Phase 2's compile step (`typstCompile`'s `--pdf-standard`/`fontPathsArgs`'
         `--font-path`). Phase 2's compile-flags bullet already lists both; nothing
         to do here.
      2. **`columns` needed zero new code.** Q1's real mechanism
         (`defaults.ts`'s `generateDefaults`) dumps the *entire* `format.pandoc`
         dict verbatim to a pandoc `--defaults` file — there is no per-key
         allow-list on Q1's side, and `format-typst.ts:124-129`'s `columns` handling
         is just: move it out of that dict into `metadata.columns` so pandoc's own
         CLI never sees it, landing it where the *template* reads it
         (`page.typ:11`'s `$columns$`). Q2 has no equivalent `format.pandoc` CLI-defaults
         dict distinct from the document's own Meta block (`doc.ast.meta`) — the
         generic merge-time flattening (`resolve_format_config`, already run for
         every format by `MetadataMergeStage`) puts `format: typst: columns: 2`
         directly into `doc.ast.meta.columns`, which is *already* exactly where
         `page.typ`'s `$columns$` reads from. Verified with
         `render_document_to_file_typst_forwards_columns_metadata` — passed with no
         implementation changes, confirming the claim empirically rather than by
         inspection alone.
      3. **`template` needed two small, concrete additions** — mirroring Q1's
         `userTemplate` (`pandoc.ts:784-810`): `format_paths.rs`'s `FORMAT_PATH_KEYS`
         gained a `("template", MarkPolicy::Always, KeyForms::Entries)` row (closing
         the residual-key note bd-hjv5o flagged), and `pandoc_write.rs` gained
         `resolve_user_template_path` (reads the merge-flattened, path-marked
         `template` key, joins it against the document's directory) plus a
         copy-over-the-vendored-file step in the typst template-staging block —
         the vendored 7 partials stay staged alongside the user's file unchanged,
         so a custom template can still `$numbering.typ()$`-include them, exactly
         as Q1's staging unions `templateContext.partials` with any user partials.
         Marked generically (not typst-gated) in `FORMAT_PATH_KEYS` so a future
         Pandoc-hybrid format can read the same resolved value with no merge-time
         changes; only typst's `PandocWriteStage` branch actually consumes it today
         (no P7 precedent existed for docx/pptx to extend). Verified with
         `render_document_to_file_typst_user_template_overrides_vendored_template`
         (RED before the `pandoc_write.rs` change, GREEN after — `columns` was
         GREEN on the first run, confirming point 2 needed no fix).
      `cargo clippy -p quarto-core --all-targets -- -D warnings` clean;
      `cargo nextest run -p quarto-core`: 4712/4712 passed, 31 skipped (was
      4710/4710 at `2a0ecbb96` — the 2 new tests). Phase-boundary
      `cargo nextest run --workspace`: 14201/14201 passed, 199 skipped —
      exactly +2 over the `2a0ecbb96` baseline (14199/14199, 199 skipped),
      matching the two new tests, identical skip count, no regressions.
- [x] **Math delivery — reopened (2026-09-20, cross-session finding, `explore/latex-typst-numbering-influence`), narrowed and resolved same day.**
      **Status: `route_crossref_resolved_ref` fixed and tested; Theorem/FloatRefTarget/Callout
      confirmed safe as-is; `route_equation` verified empirically against a real typst
      pipeline (below), once template vendoring landed.**
      **Closed out (2026-09-20)**: the last open piece — empirical verification of
      `route_equation` — is exactly what
      `render_document_to_file_typst_crossref_figure_and_equation_use_native_numbering`
      (Phase 1's crossref/numbering verification bullets, below) confirms: a labeled
      equation comes out as `#math.equation(numbering: equation-numbering, ...)`, using
      `route_equation`'s existing, unmodified call into Q1's real `renderEquation` — no
      code change was needed, exactly as this bullet's analysis predicted.
      This bullet previously read "the epic's design doc freezes `Equation` as Route N...
      this is a one-line confirmation against P1's landed code, not open design work." That
      is **wrong for a crossref-labeled equation** (one with an `@eq-` id). The design doc's
      Route L/R/N table (`pandoc-hybrid-architecture.md` §3, "Decision 4 — frozen") is
      docx/pptx-shaped throughout: Route R means "reconstruct into the Q1 node **bypassing
      Q1's numbering**," paired with `crossref-numbering: external`, which **skips the
      entire `quarto_crossref_filters` group** — not just number-assignment.
      **Typst (and eventually LaTeX) computes its own crossref numbers natively at compile
      time** (confirmed per-construct: figure/table/equation/listing/theorem/section all
      compiler-native for typst; **callout is the one exception** — LaTeX has no native
      counted callout environment and bakes a literal number docx-style, but Typst callouts
      *are* compiler-native, same `#figure(kind:)` machinery as everything else). External
      mode is therefore actively wrong for typst, not just unhelpful: skip
      `quarto_crossref_filters` and `resolveRefs` (part of that group) never runs, so
      `crossref/refs.lua:89-91`'s `#ref(<label>, supplement: [...])` emission — Typst's
      *only* crossref-rendering path — never fires. Refs don't render at all, numbered or
      not. (This is exactly why `insert_crossref_numbering_mode` was fixed to gate on
      `Docx | Pptx` rather than every `Pandoc(_)` profile — see the Phase 1 commit; that fix
      is necessary but not sufficient by itself.)

      **The corrected model for typst crossref-labeled constructs: run Q1's *unmodified*
      crossref filter group over real, native-shaped nodes — closer to the design doc's
      Route **L** (defined, but currently unpopulated: "reconstruct the raw classed
      Div/native shape and let Q1's unmodified parse+render run") than Route R as currently
      implemented for docx/pptx.** For a labeled equation specifically: the wire node needs
      to reach `crossref/equations.lua`'s `isTypstOutput()` branch (`:115-131`) so it gets
      wrapped as `#math.equation(numbering: equation-numbering, ...)` with a label
      `#ref()` can resolve against — falling through to Pandoc's plain `texmath` writer as
      bare unlabeled `Math` drops both numbering and referenceability. (A generic,
      *non*-crossref'd inline `Math` node with no `@eq-` id legitimately still falls through
      to plain `texmath` unlabeled — that part of the original bullet was fine; the error was
      treating every `Equation` wire node as that case.)

      **What Q2 still needs to supply for typst is the prefix/shape, not the number**:
      "Figure", "Table", localized names, custom category titles — computed via
      `refPrefix`/`titleString` exactly as for every other format — delivered as the
      `supplement:` argument of `#ref()`/`#figure()`. That's `crossref.*` metadata
      pass-through (already read by the vendored `crossref/format.lua`), not new
      number-computation work.

      **Scope impact — narrowed after reading the actual shim code (2026-09-20, same
      session, `resources/pandoc-filters/filters/quarto2-shim.lua`). Gordon's call: design
      the format-conditional dispatch now, in progress — status below, handed off
      mid-investigation.**

      The shim's `route_handlers` table (`:590-597`) + `ROUTE_N_TYPES` (`:611-614`) is
      per-node-type only, no format branching, confirming the peer's core claim. But reading
      each handler body shows the *actual* gap is narrower than "redesign Theorem/
      FloatRefTarget/Callout/Equation wholesale":

      - **`route_equation` (`:576-583`, Route N) already calls Q1's real
        `renderEquation(eq, label, alt, order)` directly** (not a reimplementation) — and
        `crossref/equations.lua`'s `isTypstOutput()` branch (`:115-131`) **never reads
        `order` at all**; it emits `#math.equation(numbering: equation-numbering, [...])
        <label>` and lets Typst's own counter number it. This path was **never reachable
        before this session's `crossref-numbering` fix** (the fail-fast guard aborted the
        whole render first), so it has never actually been exercised for typst — **but
        nothing in it looks wrong for typst on inspection**. Needs the Phase-1 template
        staging (for `numbering.typ`'s `equation-numbering` symbol) to verify empirically,
        exactly as this plan's pre-existing deferred verification bullet already says — this
        may turn out to need *zero* code changes, only verification.
      - **`route_crossref_resolved_ref` (`:468-548`, Route N) is the one handler confirmed
        to need a real typst-specific code path.** It bakes a citation's number as **static
        text** — `add_ref_prefix(prefix)` + `refNumberOption(entry)` computed from Q2's own
        `data.order` — never emitting Typst's `#ref(<label>, supplement: [...])`
        (`crossref/refs.lua:89-91`, the only real Typst crossref-citation mechanism). This is
        wrong two ways: (a) it's static text where Typst expects a live compiler-resolved
        reference, and (b) now that `quarto_crossref_filters` runs unsuppressed for typst
        (this session's fix), the referenced element's *own* Typst-native counter may assign
        a different number than Q2's independently-computed `data.order` — so the baked
        number in the citation could disagree with the number that actually prints at the
        referenced float. **Concrete fix (not yet implemented): add an
        `_quarto.format.isTypstOutput()` branch inside `route_crossref_resolved_ref` that
        returns `pandoc.RawInline("typst", "#ref(<data.identifier>, supplement: [<prefix
        inlines>])")` instead of the prefix+number text**, mirroring `refs.lua:89-91`
        directly (same pattern already used one line away at `:500` for the nbsp-before-
        number tweak — this file already has one typst-conditional branch, just not this
        one).
      - **Theorem / Proof / FloatRefTarget / Callout (Route R, `:174-448`) — checked
        2026-09-20, all three confirmed safe, no code change needed.** These inject Q2's
        `plain_data.order` into a real `quarto.Theorem{...}`/`quarto.FloatRefTarget{...}`/
        `quarto.Callout{...}` constructor, which Q1's own `theorem.lua`/`floatreftarget.lua`/
        `callout.lua` renderers consume later (in `quarto_layout_filters`/
        `quarto_post_filters`, downstream of the shim).
        - `theorem.lua`'s `isTypstOutput()` branch (`:258-270`) reads `thm.order` into a local
          (`:222`) but **never uses it** in the typst branch — it emits `#theorem_type.env(title:
          [...])[...] <label>` and lets `ensure_typst_theorems`'s injected `numbering:
          theorem-numbering` do the counting. Same shape as `equations.lua`.
        - `floatreftarget.lua`'s `isTypstOutput()` renderer (`:1010-1189`) never reads
          `float.order` at all — no call to `decorate_caption_with_crossref` (the function that
          bakes `float.order`/`subfloat.order` as static text via `prependSubrefNumber` /
          `float_title_prefix`, used only by the default/ipynb renderers). Numbering is native:
          `#figure(kind:, numbering: info.numbering)` for plain floats, `quarto_super(...,
          subcapnumbering: "(a)")` for subfloats.
        - `callout.lua`'s `isTypstOutput()` branch (`:231-365`) never calls
          `decorate_callout_title_with_crossref` (the function that bakes `callout.order` via
          `callout_title_prefix`/`titlePrefix`, used only by the default renderer) and never
          reads `callout.order`. Both crossref-numbered code paths (`:325-364`, `:356-364`) pass
          `numbering = nil` to `make_typst_figure` with the comment "handled by
          callout-numbering in template" — native counter, same `#figure(kind:)` machinery as
          everything else typst.

      **Bottom line, confirmed: no Route L redesign, no Route R changes.** Exactly one fix was
      needed: `route_crossref_resolved_ref`'s typst branch (implemented below). Theorem,
      FloatRefTarget, and Callout's existing Route R bodies are correct as landed — Q1's own
      typst renderers already ignore the shim-injected `order` and defer entirely to Typst's
      compiler-native counters.

      **Fix implemented (2026-09-20):** `route_crossref_resolved_ref`
      (`quarto2-shim.lua:468-548`, now `:468-561` after the fix) gained an
      `_quarto.format.isTypstOutput()` branch, inserted between the existing prefix-computation
      block and the (now typst-skipped) number-computation block. It reuses the already-computed
      prefix inlines (nbsp-free for typst per the pre-existing `add_ref_prefix` guard) and wraps
      them as `#ref(<data.identifier>, supplement: [<prefix>])`, mirroring `crossref/refs.lua`'s
      own `elseif _quarto.format.isTypstOutput()` sibling branch (`:89-91`) exactly — including
      returning before the hyperlink-wrap step, since Typst's `#ref()` is already the link.
      TDD: `crates/quarto-core/tests/integration/pandoc_shim_typst_crossref.rs` (`L`-TIER,
      `test_crossref_resolved_ref_emits_typst_ref_call`) renders `See @fig-x.` against a
      labeled figure through the real shim with `to_format = "typst"`, and asserts the captured
      AST contains a `RawInline("typst", "#ref(<fig-x>...")`. Verified RED (no such RawInline;
      shim still emitted a static `Figure\u{a0}1`-shaped `Inlines` list) before the fix, GREEN
      after. `pandoc_shim.rs` gained a format-parametrized sibling of
      `build_ast_and_params_from_content` (`..._for_format`) so the test could build filter
      params against a typst-identified `Format` — a docx-shaped `Format` would have wrongly
      set `crossref-numbering: external` via `insert_crossref_numbering_mode` and masked the
      typst-only branch. `pandoc_transport::L_TIER_TEST_COUNT` bumped 69 → 70 for the new
      `/// L-TIER` marker.

*Lua and template vendoring (do this as one coherent unit — see Vendoring approach above):*
- [x] **The 7 typst Lua files are already vendored — nothing to copy.** Verified
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
      the vendored tree has moved. **Confirmed (2026-09-20)**: all 7 files present at the
      cited paths, unconditionally `import`ed by `filters/main.lua` (`:76-78,103,200`) and
      listed in the filter pipeline (`:517,559-568`); each `render_typst*`/`render_typst_css_
      property_processing`/`render_typst_brand_yaml` function self-gates on
      `_quarto.format.isTypstOutput()` (confirmed at `typst.lua:25-26,207-208`), which reads
      pandoc's own `FORMAT` global — set to `"typst"` by the real `-t typst` invocation
      (`pandoc/datadir/_format.lua:216-217`). No code change needed; reachability holds as-is.
- [x] **Investigate the brand bridge correctly**: the parser to serialize from is the
      `crates/quarto-brand` crate (`ResolvedBrand`, `BrandFont`) plus `crates/quarto-sass`'s
      `brand_to_layers` — **not** `crates/quarto-core/src/brand_fonts.rs` (that's narrowly
      about publishing `source: file` fonts, a different concern). The task is
      schema-matching `ResolvedBrand` into the `brand` filter-param shape
      `typst-brand-yaml.lua`/`typst-css-property-processing.lua` expect — brand parsing
      itself already exists, this is a bridge, not a new parser.
      **Done (2026-09-20)**: `crates/quarto-core/src/pandoc_filters/typst_brand.rs`'s
      `build_brand_param(light, dark, project_dir)` builds the exact `{ light: {
      processedData: { color, typography, logo } }, dark: {...} }` shape
      `filters/modules/brand/brand.lua`'s `get_color`/`get_typography`/`get_logo`
      (`param("brand")[brandMode].processedData.*`) and `typst-brand-yaml.lua`'s direct
      `brand.processedData.color`/`.logo` reads expect, ported from Q1's
      `Brand.processData` (`core/brand/brand.ts:72-162`). Colors are pre-resolved to final
      CSS via `Brand::resolve_color` (Q1 resolves eagerly in `processData`); typography
      stays **raw** (color fields keep the brand-color *name*, not a resolved value)
      because `brand.lua`'s `get_typography` resolves those itself per read — verified by
      a dedicated test asserting `color` stays `"primary"`, not a hex value.
      `monospace-inline`/`monospace-block` use the existing
      `Brand::effective_monospace_inline`/`_block` merge helpers (already ported from Q1's
      `{ ...monospace, ...monospaceInline }` spread). One real gap this bridge had to close
      itself: Q2's `SplitBrand` deliberately leaves logo entries **unsplit** (a
      `{light:,dark:}` logo pair survives into both split halves as the same
      `LogoEntry::LightDark` — see `quarto_brand::split`'s module docs, bd-v5z8w), so
      picking the mode-correct side for `logo.small`/`medium`/`large`/`images.*` is new
      logic in `resource_for_mode`, not something reused from the split. 5 unit tests
      (`cargo nextest run -p quarto-core`, all passing): palette+named-slot color
      resolution, raw-color-passthrough + monospace merge, light/dark logo-path
      resolution (including the project-relative rewrite Q1's `resolvePath` does), and the
      no-brand/both-modes wrapper shape. **Not yet wired into `PandocWriteStage` /
      `FilterParamsContributor`** — that's the next checklist item
      (`extractTypstFilterParams`), which also needs the document's own `logo:` config key
      (`param('logo')`, Q1's separate `resolveLogo` family) that this bridge deliberately
      left out of scope: it normalizes a *document-level* logo spec, not brand data, and
      isn't covered by "schema-match `ResolvedBrand`".
- [x] Vendor all 8 template files from
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
      **Done (2026-09-20)**: all 8 files copied *from the `v1.11.3` tag exactly* (not the
      working-tree `HEAD` of the `external-sources/quarto-cli` checkout, which had already
      drifted one line ahead in `typst-template.typ` — a spurious-blank-page fix landed
      after the pin) into `resources/pandoc-filters/typst-template/` — flat, matching
      Pandoc's own partial-resolution rule (`$partial.typ()$` resolves relative to the
      *including* template's directory, so nesting would break the chain — this also
      answers the "no `--partial` flag" note: Pandoc doesn't need one, partials are plain
      template-syntax inclusion, not a CLI mechanism). New `TYPST_TEMPLATE_DIR` static
      (`pandoc_filters/mod.rs`) + `bundle::extract_typst_template` materialize it;
      `PandocWriteStage::run` stages it into its own `pandoc-typst-template/` subdirectory
      of the per-render temp dir (confirmed independent of the `<share>/filters/` +
      `<share>/pandoc/datadir/` layout — `--template` and `--data-dir`/`-L` are unrelated
      pandoc mechanisms with no positional constraint) and passes
      `--template <dir>/template.typ`, typst-only. **Lint-rule extension scoped out,
      documented rather than skipped silently**: `vendored-pandoc-filters` protects against
      losing Q2 *customizations* on re-vendor, and this subtree has none (vendored
      verbatim) — extending the rule would add bookkeeping with nothing to protect yet.
      Documented in `resources/pandoc-filters/README.md`'s new `typst-template/` bullet
      instead, with an explicit "add a check if that changes" note. End-to-end verified
      against real pandoc 3.11 (`render_document_to_file_typst_uses_vendored_template`):
      the vendored `typst-show.typ` partial's real `#show: doc => article(title: [...],
      ...)` wrapper appears in the output — pandoc's own bundled default typst template
      has no `article()` function at all, so this discriminates the vendored template from
      the fallback. All 5 pre-existing typst writer tests still pass unchanged (the
      template's unconditional `$var$` slots for absent metadata like `toc-depth` render
      as pandoc's own default/empty, not an error — confirmed empirically, no Q2-side
      default-filling needed).
- [x] Vendor the 5 Typst packages Q1 bundles (fontawesome, marginalia, octique, showybox,
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
      **Done (2026-09-20)**: extracted directly from the `v1.11.3` tag (`git archive v1.11.3
      -- src/resources/formats/typst/packages src/resources/formats/typst/fonts | tar -x
      --strip-components=4`, documented in the new `resources/typst-packages/README.md`) —
      2.5 MB, 41 files, landing in the exact `packages/preview/<name>/<version>/` layout a
      real Typst package cache uses (so Phase 2 can copy the subtree in directly, no
      restructuring). `README.md` also carries each package's own license file name (4
      `LICENSE`, marginalia's `UNLICENSE`) and the fonts' `LICENSE.txt`, and repeats the
      `Config.local` trap inline so Phase 2 doesn't have to rediscover it from this plan.
      **Deliberately not embedded via `include_dir!` yet** — Phase 1's scope is vendoring
      the bytes; *how* they reach the compiler's package-path (a fixed resource dir? staged
      per-render like the templates? something `typst-gather` itself expects?) is a Phase 2
      design decision this bullet shouldn't preempt by guessing at a materialization shape.
- [x] Vendor the font-availability filtering workaround (issue #12556) — this needs the
      `typst-available-fonts` filter param (see Phase 2's `getAvailableTypstFonts`
      equivalent); confirm against whatever Typst version this plan targets, since the
      workaround is Typst-version-sensitive and may already be obsolete.
      **Confirmed already vendored, no action needed (2026-09-20)**: the workaround itself
      is Lua code inside `filters/modules/typst_css.lua` (`ensure_available_fonts`/
      `init_available_fonts`), one of the 7 typst Lua files already confirmed reachable
      earlier in this checklist — there is no separate "workaround" artifact to vendor.
      What remains is wiring the real `typst-available-fonts` filter param (the actual
      `typst fonts` discovery), already explicitly scoped to Phase 2 by this plan's own
      invocation-builder bullet ("leave `typst-available-fonts` unset in Phase 1").

*Verification (deliberately deferred here — see next item):*
- [x] Confirm the crossref/numbering machinery (P3/P6) produces correct Typst output. The
      real mechanism is Q1's own Lua — `crossref/refs.lua:89-91`
      emits `RawInline('typst', '#ref(<label>, supplement: [...')` directly; Pandoc's own
      `reference-type` handling is fed only by its LaTeX *reader* and never applies to
      Q2's input. The shim's Route-R reconstruction must produce this `RawInline` path.
      This check is **only reachable once the full template (including `numbering.typ`) is
      staged** — don't attempt it before the vendoring above is complete.
      **Confirmed (2026-09-20), now that vendoring has landed**:
      `render_document_to_file_typst_crossref_figure_and_equation_use_native_numbering`
      renders `See @fig-x and @eq-einstein.` against a labeled figure and equation through
      the real pipeline (real pandoc 3.11 + the vendored template) and asserts the exact
      output `See #ref(<fig-x>, supplement: [Figure]) and #ref(<eq-einstein>, supplement:
      [Equation]).` — confirming both the shim's Route-N `route_crossref_resolved_ref` fix
      (earlier in this plan) and, separately, that the *figure* itself resolved through
      Route N's `#ref()` path too (the reference text, not the figure's own numbering).
- [x] Confirm `crossref/equations.lua`'s `isTypstOutput()` branch (`:115`) actually engages
      — equation numbering should use Typst's native `#math.equation(numbering:
      equation-numbering)`, consuming the symbol `numbering.typ` defines, not a generic
      `(N)` text fallback. Same staging precondition as above.
      **Confirmed (2026-09-20)**, same test: the labeled equation comes out as
      `#math.equation(block: true, numbering: equation-numbering, [ $ e = m c^2 $
      ])<eq-einstein>` — `equation-numbering` is `numbering.typ`'s vendored symbol
      (`#let equation-numbering = "(1)"`), confirming `route_equation` (Route N) was
      correct as landed and needed no code change, exactly as this bullet's pre-existing
      note predicted. The figure came out equally native:
      `#figure(..., kind: "quarto-float-fig", supplement: "Figure", ...)<fig-x>` — no baked
      order anywhere, confirmed by the test's explicit `!text.contains("Figure\u{a0}1")`
      assertion (the static-text shape the shim used to emit before its fix).
- [x] Golden-test methodology: typst emits plain text (`.typ`), not binary/XML like
      docx/pptx — P7's unzip-and-walk-XML approach does not apply. Use direct
      text-diff/insta-snapshot on the `.typ` source for this phase (compile-to-PDF checks
      are Phase 2's problem).
      **Confirmed as the methodology actually used**: every Phase 1 typst test in
      `crates/quarto-core/tests/integration/pandoc_typst_writer.rs` (11 tests total) reads
      the rendered `.typ` file as a plain string and asserts on substrings/exact lines —
      no insta snapshot was needed given how targeted each assertion is; revisit if a
      future test wants to pin the *entire* file (a real insta snapshot would then make
      sense, matching P7's docx/pptx golden tests' spirit even though the mechanism
      differs).

### Phase 2 — Compile step

This is substantially more than "shell out to `typst compile`" — see the research note's
Part 5 table in full before starting.

- [x] Add a new pipeline stage (name TBD, e.g. `TypstCompileStage`) that runs after the
      Pandoc-write stage produces `.typ` + staged resources (from Phase 1), and invokes
      `typst compile` to produce the final PDF. Follow the established subprocess pattern
      (`crates/quarto-core/src/engine/knitr/subprocess.rs`: cached binary discovery,
      `Command::new` + `Stdio::piped()` + `.spawn()` + `.wait_with_output()`).
      **Done (2026-09-21)**: `TypstCompileStage`
      (`crates/quarto-core/src/stage/stages/typst_compile.rs`), appended after
      `PandocWriteStage` only for `FormatIdentifier::Typst`
      (`pipeline::build_pandoc_pipeline_stages`, now format-parametrized). Uses
      `Command::new(...).output()` directly (`PandocWriteStage`'s own shape), not
      knitr's stdin/JSON request pattern — simpler and already proven in this exact
      pipeline. `PandocWriteStage` was changed to write typst's intermediate `.typ` to
      `ctx.output_path().with_extension("typ")` (not the real `.pdf` path) and mark
      `is_intermediate: true`; `TypstCompileStage` compiles that into the real PDF at
      `ctx.output_path()`. **Passthrough guard**: if the two paths coincide (Phase 1's
      tests deliberately override `RenderToFileOptions.output_path` to a `.typ` file to
      inspect pandoc's raw text, sidestepping compilation on purpose — see
      `pandoc_typst_writer.rs`'s module docs), `TypstCompileStage` is a no-op passthrough
      rather than trying to compile a file into itself; all 12 Phase 1 tests still pass
      unchanged. The CLI gate (`crates/quarto/src/commands/render.rs`) now allow-matches
      `FormatIdentifier::Typst` alongside `Docx`/`Pptx`, per the decision recorded in
      Phase 1's first bullet above.
- [x] Consume the existing `BinaryDependencies.typst` field
      (`runtime.find_binary("typst", "QUARTO_TYPST")`) — already implemented, just unread
      today.
      **Done (2026-09-21)**: the actual `StageContext`-based pipeline never threads
      `BinaryDependencies` through at all — `PandocWriteStage`'s own pandoc gate
      (`resolve_and_gate_pandoc`) bypasses it too, calling `ctx.runtime.find_binary(...)`
      directly. `TypstCompileStage::resolve_and_gate_typst` mirrors that exact pattern
      (the underlying primitive `BinaryDependencies::discover()` itself wraps), not the
      older `RenderContext.binaries` field, which nothing in this pipeline reads.
- [x] Pass the flags Q1's `typstCompile` passes: `--root` (**exactly Q2's leading-`/`-means-
      project-root convention** — omitting this breaks every project-root-relative image or
      brand logo), `--package-path`/`--package-cache-path` (pointed at wherever
      `typst-gather`'s staged output lands — see below), `--pdf-standard`, `--font-path`
      (ordering matters: Quarto's own font paths must come first, per `fontPathsArgs`).
      **Done (2026-09-21)**: `--root <ctx.project.dir>`, `--font-path <staged>/fonts`
      (Quarto's vendored Font Awesome fonts, always first — no user-configured extra
      font-paths wired yet, tracked below), `--package-cache-path <staged>` (guarded on
      the staged `packages/preview/` subdirectory actually existing, mirroring Q1's own
      guard — `--package-path`/`local/` never fires since Quarto vendors no `@local`
      packages), `--pdf-standard <comma-joined>` read from the document's `pdf-standard`
      metadata key (array or scalar), normalized via a direct port of
      `normalizePdfStandardForTypst` (lowercase, strip a `pdf`/`pdf-`/`pdf/` prefix,
      filter against Typst's actual supported-standards set — an unsupported value is
      dropped with a codeless warning, not forwarded verbatim). Unit-tested
      (`test_normalize_pdf_standards_strips_prefix_and_filters`).
- [x] **Run unconditional typst version validation** (min `>=0.8`, matching
      `validateRequiredTypstVersion`) — unconditional because, unlike Q1, Q2 has no bundled
      known-good binary to skip validation for.
      **Done (2026-09-21)**: `resolve_and_gate_typst` always parses `typst --version`'s
      stdout and compares numerically (never lexicographically) against `(0, 8)`,
      mirroring `pandoc_filters::version`'s `at_least` (duplicated rather than shared —
      distinct, unrelated binaries/floors). Unit-tested
      (`test_at_least_is_numeric_not_lexicographic`). Missing/too-old binary emit
      `Q-19-1`/`Q-19-2` (new `typst` error-catalog subsystem — catalog entries +
      `docs/errors/typst/Q-19-{1,2,3}.qmd` + sidebar section added in the same commit,
      mirroring `pandoc`'s `Q-18-1`/`Q-18-2`/`Q-18-3` trio exactly).
- [x] **Package staging via `typst-gather` as a linked crate, not a subprocess.** Add
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
      **Done (2026-09-21, this session), and the plan's own "point discovery at the
      whole staged template directory" note turned out to be based on a mistaken
      premise — corrected below.** `typst-gather` is now a real Cargo dependency
      (`typst-gather = { git = "https://github.com/quarto-dev/typst-gather", tag =
      "v0.2.3" }`, pinned in `[workspace.dependencies]`, consumed via
      `typst-gather.workspace = true` in `quarto-core`'s native-only deps table — not
      published on crates.io, confirmed 404, per Gordon's 2026-09-20 decision).
      `TypstCompileStage::run` calls `typst_gather::gather_packages` after staging the
      vendored 5 (`extract_typst_packages`, unchanged), pointed at **`typ_input` alone
      — not the 8-file template directory this bullet originally called for.**
      Re-reading the actual vendored `definitions.typ` (from the `v1.11.3` tag) showed
      its marginalia import is **not** unconditional as this plan's "Vendoring
      approach" section claimed — it's gated behind `$if(margin-geometry)$`, itself
      driven by a `.column-margin` div, `reference-location: margin`, or
      `citation-location: margin`. Since Pandoc's own `$if(...)$` partial resolution
      already decides which imports survive into the *final* compiled `.typ` output,
      that file — not the raw template sources — is both the necessary and sufficient
      thing to scan: it's the actual, complete set of imports the compile is about to
      need, covering the vendored 5 (conditionally) and any arbitrary
      document-body-level `@preview` import identically, with no need to carry the
      ephemeral `--template` staging directory's lifetime into the compile stage.
      Verified empirically before implementing (not guessed): a plain document's
      compiled `.typ` contains no `#import` at all; a `.column-margin` div's does,
      verbatim, `#import "@preview/marginalia:0.3.1"`.
      **A real, previously-undiscovered bug was found and fixed in the same change**:
      the already-landed `--package-cache-path` argument pointed at `packages_dir`
      (the extraction root) instead of `packages_dir.join("packages")` (where
      `extract_typst_packages` actually puts `preview/<name>/<version>/` —
      confirmed by inspecting `resources/typst-packages/packages/preview/`'s on-disk
      layout). Every prior Phase 2 test happened to render a document that never
      triggers `margin-geometry`, so the mismatch was silently masked by (a) no test
      ever needing a real vendored package, and (b) this developer's machine already
      having `marginalia` in its ambient `~/Library/Caches/typst` from unrelated prior
      `typst` use, so even a manual "does it work" check would have silently fallen
      back to a live network fetch without failing. Reproduced with `HOME`/
      `XDG_CACHE_HOME` isolated to a fresh temp dir (no ambient cache) — the render
      fell back to a real network download of marginalia, proving the explicit
      `--package-cache-path` was not actually being consulted. Fixed by passing
      `packages_dir.join("packages")` to both the compile flag and `typst-gather`'s
      `dest`. Regression test
      (`render_document_to_file_typst_margin_note_compiles_without_network` in
      `pandoc_typst_compile.rs`) isolates `HOME`/`XDG_CACHE_HOME` to a fresh temp dir
      **and** points `HTTP(S)_PROXY` at an unroutable address, so a wrong cache path
      fails fast and loud (`Connection refused`) instead of silently succeeding via
      the network — verified RED against the pre-fix code, GREEN after. A second,
      pure-unit test
      (`test_gather_packages_recognizes_already_staged_package_without_network` in
      `typst_compile.rs`) pre-seeds a fake `@preview` package directly in the package
      cache and asserts `gather_packages` recognizes it (`stats.skipped == 1`,
      `stats.downloaded == 0`) without touching the network — proving the "arbitrary
      package beyond the vendored 5" discovery path itself, independent of any real
      package registry. `gather_packages`'s `unconfigured_local` result (a document
      referencing an unsupported `@local` package) surfaces as a codeless warning
      diagnostic rather than a hard error — Quarto doesn't support user-authored
      `@local` typst packages, but the eventual `typst compile` failure will say so
      clearly enough; this warning just gets there first.
      Known rough edge, not fixed: `typst-gather`'s internal `eprintln!` progress
      messages ("Downloading...", "Skipping (cached)...") write directly to the
      process's real stderr on every typst render, mixed in with Quarto's own
      output — cosmetic, not a correctness issue, but worth knowing before someone
      is confused by it. Fixing it means forking or patching `typst-gather` upstream,
      out of scope here.
- [x] **WASM-gate the new compile-stage module.** Gate the compile stage's module with
      `#![cfg(not(target_arch = "wasm32"))]`, mirroring
      `crates/quarto-core/src/engine/knitr/mod.rs:29`.
      **Done (2026-09-21)**: `typst_compile` is declared/exported behind
      `#[cfg(not(target_arch = "wasm32"))]` in `stage/stages/mod.rs`, exactly mirroring
      `pandoc_write`'s existing gate. **`typst-gather` itself is added to
      `quarto-core`'s `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` table
      (2026-09-21, once the dependency was actually wired in — see the package-staging
      bullet above)**, so both halves of this bullet now hold.
- [x] Implement `getAvailableTypstFonts`-equivalent font discovery: a `typst fonts`
      subprocess invocation, cached, feeding the `typst-available-fonts` filter param
      (needed by the #12556 font-fallback workaround vendored in Phase 1).
      **Done (2026-09-21)**: `discover_available_typst_fonts`/`parse_typst_fonts_output`
      (`typst_compile.rs`) port `core/typst.ts`'s `getAvailableTypstFonts`/
      `parseTypstFontsOutput` directly — a `typst fonts --font-path <staged fonts dir>`
      subprocess, stdout split into lowercased trimmed lines. Called from a new
      `resolve_typst_available_fonts` in `pandoc_write.rs` (not `typst_compile.rs`,
      since the filter param must exist when Pandoc's Lua filters run, i.e. *before*
      `TypstCompileStage` — the package/font tree is staged a second time there,
      idempotently, into the same `ctx.temp_dir()`-scoped directory
      `TypstCompileStage` will stage again later in the same render, so the reported
      list matches the font-path the real compile will actually use). Wired into
      `PandocWriteStage`'s existing `TypstFilterParamsContributor` (now also carrying
      an `available_fonts: Option<Vec<String>>` field, emitted as a JSON string array
      under the `typst-available-fonts` key `typst_css.lua`'s `param(...)` call reads).
      Fails open (`None`), not an error, on a missing binary or failed subprocess —
      the Lua consumer already treats an absent param as fully permissive, and a
      genuinely missing/broken `typst` is `TypstCompileStage`'s clearer `Q-19-*` error
      to report later in the same render.
      **Not ported: Q1's cross-render in-memory + on-disk cache.** Each render's
      `--font-path` points at that render's own fresh temp directory, so a
      literal-path cache key would never hit across separate renders anyway — the
      cache would only help within a single render, where `typst fonts` already runs
      at most once. Revisit if project-mode multi-document builds show repeated
      invocations are a measurable cost (deliberate scope-down, not silently
      dropped).
      Tests: two pure unit tests for `parse_typst_fonts_output` (lowercase/trim/drop-
      blanks, empty input); a fail-open unit test for a missing binary; an
      end-to-end unit test against the real vendored Font Awesome fonts + a real
      `typst` binary asserting a real family name comes back
      (`test_discover_available_typst_fonts_finds_vendored_font_awesome`). The full
      chain's real-world effect was verified twice — first manually
      (`cargo run --bin q2 -- render` a `::: {style="font-family: 'Font Awesome 6
      Free', 'Some Totally Fake Font'"}` div to `.typ`; the fake font was dropped,
      confirmed by re-running the identical command against the pre-fix binary first,
      where both fonts survived unfiltered), then committed as a regression test
      (`render_document_to_file_typst_filters_unavailable_font_family` in
      `pandoc_typst_writer.rs`).
- [x] Decide and implement the missing-binary / compile-failure diagnostic. The epic's own
      P4 plan commits to a new `pandoc` error-catalog subsystem for pandoc binary-missing/
      nonzero-exit. Decide explicitly whether typst compile failures get their own
      `Q-<n>-*` subsystem or fold into the `pandoc` one as a distinct case. Whichever is
      chosen, follow the epic's process: `docs/errors/<subsystem>/` page(s) in the same
      commit (`error-docs-page-missing`) and a sidebar entry
      (`error-docs-sidebar-unlisted`).
      **Decided and done (2026-09-21): a new `typst` subsystem, `Q-19-*`** — typst is a
      genuinely distinct external tool/failure domain from pandoc, and Q1's own
      `core/typst.ts` is a separate module from anything pandoc-related; folding into
      `Q-18-*` would conflate two unrelated tools under one docs folder. `Q-19-1` (Not
      Found), `Q-19-2` (Version Too Old), `Q-19-3` (Compile Failed) mirror `Q-18-1/2/3`
      exactly. Catalog entries (`error_catalog.json`), three docs pages
      (`docs/errors/typst/Q-19-{1,2,3}.qmd`), and a new sidebar section
      (`docs/_quarto.yml`) landed in the same commit; `cargo xtask lint` passes
      (`error-docs-page-missing`/`error-docs-sidebar-unlisted`). Verified the new page
      renders correctly with `cargo run --bin q2 -- render docs/errors/typst/Q-19-1.qmd`
      (per this repo's docs/ convention — Q2 renders its own docs, never Q1's `quarto`).
- [x] Capture `typst` compiler stderr unconditionally (mirroring the epic's decision for
      `pandoc`'s stderr).
      **Done (2026-09-21)**: `TypstCompileStage` reads `output.stderr` regardless of
      exit status — embedded verbatim in the `Q-19-3` error on failure, surfaced as a
      codeless warning diagnostic on success when non-empty (mirroring
      `classify_pandoc_completion`'s unconditional-capture policy, simplified since
      typst's stderr has no `[WARNING]`-shaped-line classification to do).
- [x] Decide and document: does Q2 keep the intermediate `.typ` file by default? Q1 already
      ships `keep-typ` (`kKeepTyp`, `config/constants.ts:88`), forced on in debug mode
      alongside `keep-tex`, discarded otherwise. Port that default directly.
      **Done (2026-09-21)**: ported the direct default — `keep-typ: true` in document
      metadata retains the intermediate `.typ` alongside the compiled PDF; unset/false
      removes it after a successful compile. Q1's debug-mode auto-enable half is **not**
      ported (no equivalent debug-mode concept wired to this stage yet) — noted as a
      scope call, not silently dropped. Tested:
      `render_document_to_file_typst_keep_typ_retains_intermediate`.
- [x] End-to-end verification per the repo's standing rule: render a real `.qmd` fixture
      (including a callout, a figure crossref, and an equation crossref — reachable now
      that Phase 1 staged everything) to `--to typst`, inspect the actual PDF, and record
      the invocation + a description of what was inspected in this plan or the session
      transcript.
      **Done (2026-09-21).** Real invocation:
      `cargo run --bin q2 -- render doc.qmd --to typst` against a fixture with a
      crossref'd figure (`# Introduction` heading, `See @fig-example.`, `![A
      caption](image.svg){#fig-example}`) — exit code 0, real subprocess chain (pandoc
      writer -> typst compile), no CLI overrides. Inspected the actual output file
      (`xxd`/`strings`, not just exit-code success): `doc.pdf` is 16022 bytes, begins
      with a real `%PDF-1.7` header, and its embedded XMP metadata shows
      `CreatorTool: Typst 0.14.2` and `dc:title: <the document's own title>` — confirming
      a genuine typst-produced PDF, not a stub. Three more end-to-end pipeline-level
      tests (`crates/quarto-core/tests/integration/pandoc_typst_compile.rs`) cover: a
      plain document compiling to a real PDF (checks the `%PDF-` header and that
      `keep-typ`'s default removes the intermediate `.typ`), `keep-typ: true` retaining
      the intermediate, and a crossref'd figure (with a real, valid SVG image — not a
      hand-typed PNG, which turned out to be fragile: an initial attempt at hardcoding
      PNG bytes by hand produced CRC-corrupt files that real `typst compile` correctly
      rejected; reused an existing verified-valid `TINY_PNG`-shaped fixture pattern
      before switching to SVG for simplicity) compiling end-to-end. **Callout crossref
      not separately covered** — Phase 1's crossref verification already covers
      callout/theorem/floatreftarget at the `.typ`-text level (Route R, confirmed safe
      as-is); not re-verified at the compiled-PDF level in this pass.

### Phase 3 — Docs & polish
- [x] `docs/` page for the typst format (usage-focused, per the repo's docs/ convention —
      not technical internals).
      **Done (2026-09-21)**: `docs/guides/formats/typst/index.qmd` (replacing the
      long-standing `TBD.` stub — `git log` showed it untouched since the original
      site-scaffolding commit) covers: requirements (`typst` on `PATH` or
      `QUARTO_TYPST`, min version 0.8), basic usage (`--to typst` / `format: typst`),
      `format.typst.{columns,template,pdf-standard,keep-typ}`, native crossref
      numbering, margin notes (`.column-margin` → vendored `marginalia`), brand.yml
      support, and the `typst-gather`-backed automatic fetch of arbitrary
      `@preview` packages beyond Quarto's vendored 5. Added to the "Guides" sidebar
      section in `docs/_quarto.yml` (the page was otherwise unreachable from site
      navigation — confirmed the same is true of the pre-existing, equally-orphaned
      `guides/formats/{latex,html}` pages; leaving those as-is is a deliberate scope
      call, not an oversight — fixing that is a separate, pre-existing gap unrelated
      to this plan). Rendered with `cargo run --bin q2 -- render
      docs/guides/formats/typst/index.qmd` per this repo's docs/ convention (Q2
      renders its own docs, never Q1's `quarto`); inspected the real output HTML —
      correct `<title>`, all code/option blocks present, external links to
      typst.app resolve.
- [x] Audit the Q1-cited upstream Pandoc bugs (image alt-text #11394, width/height units
      #9945, table-in-figure #10438 — all in `filters/quarto-post/typst.lua`; Skylighting
      block styling #14126 in `format-typst.ts`'s postprocessor) against **the epic's
      pinned minimum pandoc version (3.10, per P4)**, not whatever pandoc happens to be
      installed locally. Drop any workaround the 3.10 floor already fixes rather than
      porting dead code.
      **Done (2026-09-21) — audited against real upstream history via `gh`, not
      guessed. Conclusion: keep all four unchanged; none are dead code at the 3.10
      floor.**
      - **#9945** (width/height unitless→inches): the pandoc issue itself was never
        fixed on pandoc's side (closed 2025-01-02 without a merged pandoc PR) — the
        actual fix was Quarto's *own* downstream PR
        (`quarto-dev/quarto-cli#11676`, merged 2025-01-17), i.e. exactly the Lua
        workaround already vendored here. Still required at any pandoc version.
      - **#11394** (image alt text): closed *without merging* — jgm closed it
        "in favor of a fresh change" that did land upstream
        (`jgm/pandoc@cc72b8054c`, "Typst writer: include alt attributes on
        images", 2026-02-14, with escaping/data-URI follow-ups
        `04848401d5`/`a0448c7168`), which predates the 3.10 release
        (2026-06-04) — so pandoc *does* now pass `alt` through natively. **Kept
        anyway**: this vendored code does substantially more than the pandoc bug
        it's named after — it also implements Quarto's own `fig-alt` > `alt` >
        caption-as-alt precedence and a caption-suppression flag, neither of
        which pandoc's native fix knows about, so *some* version of this logic
        stays necessary regardless of pandoc version. Decisive check: **Q1's own
        current upstream `typst.lua`** (`quarto-dev/quarto-cli@18634c0ac`, "Update
        Pandoc, Typst, Dart Sass, esbuild, and veraPDF") is **already pinned to
        pandoc 3.10** (`configuration`'s `PANDOC=3.10`) and still carries this
        exact code, byte-for-byte, unchanged. Per this repo's "vendor Q1's Lua
        verbatim" policy, matching Q1's own current judgment at the identical
        floor is the right call, not a unilateral simplification based on reading
        pandoc's changelog in isolation.
      - **#10438** (table-in-figure): a `quarto-cli` issue, not a pandoc bug —
        closed as `COMPLETED` by the very workaround being audited
        (`typst:no-figure` class insertion). No pandoc version could "fix" this;
        it's a permanent Quarto-side layout choice. Confirmed unchanged in Q1's
        current `typst.lua` too.
      - **#14126** (Skylighting block styling): also a `quarto-cli` issue (not
        pandoc or skylighting), closed `COMPLETED` in 2026-03 by Q1's own
        postprocessor — "temporary until upstreamed to the Skylighting library"
        never happened; Q1's current `format-typst.ts`, pinned to the same
        pandoc 3.10 floor, still carries the regex-based postprocessor unchanged.
      **New finding, deliberately not acted on — flagged for Gordon rather than
      unilaterally designed:** Q2 has **no equivalent of the #14126 Skylighting
      postprocessor at all** — it was never in this plan's Phase 1/2 vendoring
      scope (only the 7 Lua files were), so Q2's typst code blocks likely carry
      the same missing-width/inset/radius visual bug Q1 patches around. Porting
      it verbatim would mean regex-patching pandoc's own generated `.typ` text
      after `PandocWriteStage` writes it — precisely the "DOM/text postprocessor"
      shape this repo's architecture policy (`CLAUDE.md`'s "No DOM postprocessor"
      section) says to re-express as an AST transform instead, and flags as "do
      not introduce without an extremely strong, explicitly-discussed reason."
      Unlike HTML, typst genuinely has no earlier AST-level seam to move this
      into: the Skylighting function is pandoc's *own* writer-generated
      boilerplate, injected into the `.typ` text only after Pandoc has already
      written it — there is no Quarto-owned AST node representing it to
      transform instead. `TypstCompileStage` (which already reads/mutates the
      intermediate `.typ` before compiling, for `keep-typ` etc.) is the one
      existing seam that could host a text patch without inventing a new
      pipeline stage, but doing so is still the same architectural shape the
      policy warns about, just reusing an existing stage rather than adding one.
      This needs a design decision, not a unilateral implementation — out of
      this bullet's audit scope, and per this plan's own checklist convention it
      belongs as new discovered work *in this plan* if picked up (not a beads
      strand — it's squarely inside "full Q1 feature parity," this plan's stated
      end state), not attempted here.
- [x] Full workspace verification per this repo's standing rules:
      `cargo xtask verify` (full, since this touches `quarto-core`) before any push.
      **Done (2026-09-21), with an honest caveat — full `cargo xtask verify` does
      not currently pass on this branch, for a reason unrelated to this plan.**
      `cargo xtask verify` (unabridged) failed at step 8/14 (hub-client tests):
      the `smoke-all preview <-> render DOM parity` suite
      (`hub-client/src/services/smokeAllParity.wasm.test.tsx`) fails on nearly
      every fixture. Diffing an artifact directly
      (`hub-client/test-results/parity/drafts__draft-banner.qmd/*.norm.txt`)
      showed the real cause: the **WASM preview pipeline omits the entire
      `<header class="quarto-title-block">` block** that the real `q2 render`
      path emits — a severe, systemic, pre-existing bug, filed as `bd-sv5dqgbd`
      (priority 1) rather than fixed here, since it's title-block rendering in
      the WASM preview path, unrelated to typst output and squarely outside
      this plan's scope (the "Beads vs. plans" rule: this is *not* part of
      completing this plan, so it's a strand, not a checklist item here).
      Confirmed not caused by this session: zero of this session's changed
      files compile into the `wasm32` target (`pandoc_write.rs`/
      `typst_compile.rs` are both already native-only-gated modules), and the
      `Cargo.lock` diff is purely additive (no existing package version
      changed). Re-ran `cargo xtask verify --skip-hub-tests` to confirm every
      *other* leg is clean: lint/clippy, Rust formatting, full workspace build,
      `cargo nextest run --workspace` (**14215/14215 passed, 199 skipped** —
      matching the per-crate delta reported above), tree-sitter grammar tests,
      ts-packages builds, hub-client build (including a real WASM rebuild),
      trace-viewer/preview-*/hub-MCP package tests, and the q2-preview-spa
      build all passed — `✓ All verification steps passed!`. The only red leg
      is the one this plan should not silently paper over by skipping it
      without saying so.
