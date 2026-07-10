# Epic: All output formats via a Pandoc-writer hybrid (Cut A)

**Date:** 2026-07-10
**Status:** Proposed (research plan — not yet started)
**Companion research:** [`claude-notes/research/2026-07-09-q1-filter-catalog.md`](../research/2026-07-09-q1-filter-catalog.md)

---

## The problem, in plain terms

Quarto 2 today can only produce two output formats: HTML and revealjs. Every other
`--to` target — LaTeX/PDF, Typst, Word (`docx`), PowerPoint (`pptx`), EPUB, JATS,
AsciiDoc, GitHub Markdown, Jupyter notebooks, dashboards, and the rest — is rejected
outright. The check lives in one place, `crates/quarto/src/commands/render.rs:626-633`,
which hard-fails anything that isn't `FormatIdentifier::is_native()` (i.e. not
`Html | Revealjs`, per `crates/quarto-core/src/format.rs:61-63`).

Quarto 1 supports all of those formats. It does so by leaning on **Pandoc's own
writers** for the heavy lifting, and layering Quarto's format-specific behavior on top
as **Lua filters** that run inside Pandoc just before the writer emits bytes. Turning a
callout into a LaTeX `tcolorbox`, a multi-figure panel into a Word table, a cross-reference
into a Typst `#ref` — all of that is Lua, sitting in front of a Pandoc writer that already
knows how to serialize a Pandoc AST to LaTeX/OOXML/Typst/etc.

The naive way to bring these formats to Q2 would be to **reimplement every Pandoc writer
in Rust** inside `pampa`. That is an enormous undertaking — Pandoc's writers represent
many person-years of accumulated format lore — and it is almost entirely undifferentiated
work: we would be re-deriving output Pandoc already produces correctly.

## The idea

We don't have to rewrite the writers. We can **use Pandoc itself as our writer** for the
formats it already supports, and reuse Quarto 1's Lua filters — largely unchanged — for the
format-specific rendering on top. Q2 keeps doing what it is uniquely good at (parsing qmd,
executing engines, computing Quarto's semantic model: crossrefs, callouts, theorems,
floats), and then, instead of running its own HTML writer, it **hands the document to
Pandoc as a JSON AST** and lets Pandoc + the imported Lua filters finish the job.

The research doc that accompanies this plan
([`2026-07-09-q1-filter-catalog.md`](../research/2026-07-09-q1-filter-catalog.md))
catalogued all 138 built-in Q1 Lua filters against Q2's pipeline and found that this maps
astonishingly cleanly onto structure Q2 **already has**:

- Q2's transform pipeline runs in four phases: **Normalization → Crossref → Navigation →
  Finalization**. The boundary at the **end of the Crossref phase** is a natural fault line
  between *format-agnostic semantics* (what the document *means*) and *format-specific
  presentation* (how it looks in one output).
- Q1's format-specific renderers — essentially all of the `layout/`, `quarto-post/`, and
  `quarto-finalize/` filter groups, plus the per-format render handlers in `customnodes/`
  (the 44 files the catalog labelled `format-not-in-q2`) — all run **after** that same
  boundary (Q1's `pre-render … post-finalize` region). Nothing format-specific runs before it.
- Q2's **Navigation** phase (navbar, sidebar, page-nav, listings, feeds, TOC widget) is
  HTML/website chrome with no meaning for single-file formats like LaTeX or Word, so it is
  simply *skipped* for a Pandoc handoff.

So the plan is: **fork the pipeline at the end of the Crossref phase.** For HTML/revealjs,
continue exactly as today (Navigation → Finalization → pampa HTML writer). For every other
format, serialize the Pandoc JSON AST at that fork and pipe it to
`pandoc -f json -t <format> --template … --lua-filter <imported Q1 filters>`. We call this
fork point the **`PostCrossref` seam**, and the overall architecture **Cut A**.

## Why this shape is right

- **Maximal reuse, minimal reinvention.** Pandoc's writers + Q1's proven Lua filters are
  reused wholesale. We port almost no rendering logic.
- **One source of semantic truth.** Crossref numbering, callouts, theorems, and floats are
  computed **once**, by Q2's front end, and shared across *every* output format — including
  HTML. Formats can't drift apart on "what figure number is this."
- **Non-invasive to what already works.** HTML/revealjs rendering is untouched; new formats
  opt into the fork.
- **It matches the seam the catalog independently flagged as most valuable.** The
  `PostCrossref` seam ("resolved numbers, before presentation") is the same one the catalog
  recommended exposing to *user* filters, and the same boundary whose internal absence once
  let a revealjs transform corrupt crossref numbering. Building it pays off twice.

There is one genuinely hard, **untried** part, and the plan treats it as the central risk:
Pandoc's JSON AST has no notion of Quarto's custom nodes. Q2 represents callouts/floats/etc.
as real Rust `CustomNode` values; Q1 fakes them as classed-`Div` "scaffolds" that its Lua
runtime decodes. To hand Q2's document to Q1's imported filters, we must translate one
representation into the other. How we do that is what separates the easy on-ramp from the
mature architecture (see the proposal).

---

## What still needs researching first: the TypeScript layer

This session researched only the **Lua** half of Quarto 1's format machinery. But Q1 also
has a substantial body of **TypeScript** that orchestrates each format, and none of it has
been surveyed yet. It is **in scope for this epic** but **not yet researched** — and it
gates the design, so it must be studied before (or in parallel with) the earliest
implementation phases.

The TypeScript layer is what decides *how Pandoc is actually invoked* for a given format.
Roughly, it owns:

- **Pandoc invocation:** which `--to`, which `--template`, which `--defaults`, which
  `--lua-filter`s and in what order, extra CLI flags, `-M`/`-V` variables, standalone vs.
  fragment, `--reference-doc` for Word/PowerPoint.
- **Format definitions & defaults:** `src/format/<fmt>/format-*.ts` — per-format option
  defaults, pandoc-argument construction, feature toggles (e.g. `output-divs: false` for
  pptx/gfm).
- **Pre/post-processing outside Pandoc:** LaTeX → PDF via latexmk/tectonic, Typst compile,
  EPUB packaging, `docx`/`pptx` reference-doc handling, image conversion (svg→pdf), and
  project-level assembly (book chapter merge, manuscript notebook embedding).
- **Resource & dependency plumbing:** how format-specific resources, mediabag entries, and
  template partials are staged.

Open research questions to answer before committing to per-format work (do **not** answer
these now — this is a forward pointer, and likely its own research session + doc):

- [ ] For each target format, what is the exact Pandoc command line Q1 constructs
      (to/template/defaults/filters/flags/variables)?
- [ ] Which formats are "pure Pandoc writer + Lua" vs. which need a **custom Lua writer**
      (dashboard, email, confluence, hugo, llms-txt) that Pandoc invokes via `--to <script>.lua`?
- [ ] What non-Pandoc post-processing does each format require, and where must it run
      relative to the Pandoc call (latexmk, typst compile, epub zip, reference-doc merge)?
- [ ] Which TS behaviors are genuinely format-specific rendering vs. project/website
      orchestration already owned (or planned) elsewhere in Q2?
- [ ] What is the minimum TS→Rust port needed to drive Pandoc correctly per format, and what
      can be deferred?

The output of that research should be a companion doc
(`claude-notes/research/YYYY-MM-DD-q1-format-typescript.md`) and a per-format tiering table.
This plan's proposal is structured so that the TS research is **Phase 0** and blocks the
format-rollout phases, but not the seam/infrastructure phases.

---

<!-- =================================================================== -->
<!--  TONE SHIFT: everything below is LLM-oriented. Terse, technical,     -->
<!--  file:line-anchored, TDD-first, checklist-driven.                    -->
<!-- =================================================================== -->

## Proposal

Target end-state: **Cut A** for all non-HTML formats. Q2 owns Normalization + Crossref;
Pandoc + vendored Q1 `layout`/`post`/`finalize` Lua own presentation + writing. On-ramp via
**Cut B** (existing `pre` seam, full Q1 chain, no new seam, no CustomNode bridge) to
de-risk the JSON→Pandoc→filters→writer chain before building Cut A infrastructure.

Authoritative anchors (verify before editing; may drift):
- Native-format gate: `crates/quarto/src/commands/render.rs:626-633`; `FormatIdentifier::is_native()` `crates/quarto-core/src/format.rs:61-63`.
- Pipeline builders: `crates/quarto-core/src/pipeline.rs` (`build_html_pipeline_stages_with_options` ~:269; `build_transform_pipeline` :1159).
- Transform phases: `crates/quarto-core/src/transform.rs:71-93` (`TransformPhase`); ordering test `pipeline.rs:3122`.
- User-filter seam: `crates/quarto-core/src/stage/stages/user_filters.rs:17-23` (`FilterPosition`); resolution `crates/quarto-core/src/filter_resolve.rs:33-40` (`ENTRY_POINTS`).
- JSON writer (serialization mechanism): `crates/pampa/src/writers/json.rs`.
- Extension→format placeholders + external-tool precedent: `crates/quarto-core/src/render_to_file.rs:503-511`; typst binary lookup `crates/quarto-core/src/render.rs:155`.
- Pandoc-compatible template engine already in-tree: `crates/quarto-doctemplate`.

### Hard constraints

- **External-sources policy (CLAUDE.md).** Q1 Lua filters live in `external-sources/quarto-cli/…`,
  which is NOT version-controlled and NOT present in CI. They MUST be **vendored** into an
  in-repo directory (mirror the `resources/scss/` precedent, e.g. `resources/pandoc-filters/`)
  and embedded via `include_dir!` from *that* path. The `external-sources-in-macro` lint
  (`cargo xtask lint`) enforces this. No build/runtime path may reference `external-sources/`.
- **TDD (CLAUDE.md).** Every phase writes tests first. Golden-parity tests (below) precede
  implementation. Do not close any phase whose tests do not pass.
- **`?Send` async traits** for any new `PipelineStage`/`AstTransform` (`.claude/rules/wasm.md`).
- **Phase-ordering invariant** must keep passing; any new transform declares its `phase()`
  and any pipeline split preserves non-decreasing phase rank (`pipeline.rs:3122`).
- Pandoc becomes a **runtime dependency** for non-HTML formats. Detect/locate it the way the
  `typst` binary is already located; fail with a catalog error if absent.

### Testing strategy (write before Phase 1 implementation)

- [ ] **Golden-parity harness.** For a fixture set, render with Q1 (`quarto-cli` in
      `external-sources/`, dev-only, NOT a build dep) and with the Q2 hybrid; normalize
      (line endings, tmp paths, timestamps) and diff. Parity target is *semantic* equality
      of the Pandoc AST fed to the writer and byte-similarity of final output, per format.
- [ ] **AST-at-the-cut snapshots.** Snapshot the Pandoc JSON emitted at the `PostCrossref`
      fork for representative fixtures; this is the contract between Q2 and Pandoc+Lua.
- [ ] **Per-format smoke fixtures** covering callouts, crossrefs (fig/tbl/sec/eq/thm),
      panels/layout, code cells, footnotes, includes, and metadata/title-block.
- [ ] Route tests through the real binary (`cargo run --bin q2 -- render fixture.qmd --to <fmt>`),
      not just library calls (CLAUDE.md end-to-end rule). Inspect emitted files.

### Phase 0 — TypeScript research (BLOCKING for rollout; not for infra)

- [ ] Produce `claude-notes/research/YYYY-MM-DD-q1-format-typescript.md` answering the open
      questions in the section above.
- [ ] Deliver a **format tiering table**: Tier 1 = pure Pandoc-writer + Lua (latex, docx,
      odt, epub, jats, asciidoc, commonmark/gfm, ipynb, beamer, context, pptx, typst);
      Tier 2 = needs external post-process (pdf/latexmk, typst-compile, epub-zip,
      reference-doc); Tier 3 = custom Lua writer (dashboard, email, confluence, hugo, llms-txt).
- [ ] Per Tier-1 format: the exact intended Pandoc invocation (to/template/defaults/filters/vars).

### Phase 1 — JSON→Pandoc bring-up via Cut B (throwaway scaffold; pilot = `latex` → `.tex`)

Goal: prove the chain end-to-end with **zero new seam** and **zero CustomNode bridge**.
Pilot format is `latex` emitting `.tex` (richest Q1 filter exercise; no latexmk yet).

- [ ] Golden-parity + AST-at-cut tests for the `latex` pilot (RED first).
- [ ] Vendor the **full** Q1 Lua filter chain + deps (`ast/`, `common/`, `modules/`,
      `customnodes/`, `normalize/`, `quarto-init/`, `quarto-pre/`, `crossref/`, `layout/`,
      `quarto-post/`, `quarto-finalize/`, `main.lua` et al.) into `resources/pandoc-filters/`;
      wire `include_dir!`; pass `cargo xtask lint`.
- [ ] Add a `PandocWriteStage` (native-only) that: takes `PipelineData::DocumentAst`,
      serializes via `pampa::writers::json`, shells to `pandoc -f json -t latex
      --lua-filter <vendored main.lua>` (+ minimal defaults), and writes the result.
- [ ] Add a non-HTML pipeline builder that cuts at the **existing `pre` seam** (i.e. after
      engine execution / before Q2 transforms) → `PandocWriteStage`. Q2 runs no transforms;
      Q1's full chain runs inside Pandoc.
- [ ] Relax `render.rs:626-633` to admit piloted formats through the new path.
- [ ] Locate/validate the `pandoc` binary; catalog error if missing.
- [ ] Verify end-to-end: `cargo run --bin q2 -- render fixture.qmd --to latex`; inspect `.tex`.

Acceptance: pilot `.tex` reaches parity with Q1 for the smoke fixtures. Divergence between
Q2-HTML crossref numbers and Q1-latex numbers is **expected here** (Cut B lets Q1 own
semantics) and is the motivation for Phase 3–5.

### Phase 2 — Filter-chain assembly & format plumbing

- [ ] Per-format Pandoc invocation builder (from Phase 0 data): template, defaults,
      filter list + order, variables. Decide `--template` (Pandoc) vs. `quarto-doctemplate`
      pre-render (default to Pandoc `--template` for Tier 1; revisit for Tier 3).
- [ ] Resource/mediabag/dependency staging for the Pandoc call (see `mediabag.lua` gap in
      the catalog — drain mediabag to disk before/at the handoff).
- [ ] Extend the format registry (`format.rs`) so target formats resolve config, extension
      (`render_to_file.rs:503`), and a "writer backend = pandoc(fmt)" marker.

### Phase 3 — Expose the `PostCrossref` seam (Cut A infrastructure)

- [ ] Tests: phase-ordering test updated for the split; new `FilterPosition` round-trips
      through `filter_resolve` (RED first).
- [ ] Split `AstTransformsStage` into two sub-stages at the Normalization/Crossref |
      Navigation/Finalization boundary: `AstTransformsStage(Semantic)` = Normalization+Crossref,
      `AstTransformsStage(Presentation)` = Navigation+Finalization.
- [ ] Insert `UserFiltersStage(PostCrossref)` between them; add `FilterPosition::PostCrossref`
      (`user_filters.rs`).
- [ ] Extend `ENTRY_POINTS` (`filter_resolve.rs:33-40`) to the **8→3** projection:
      `pre-ast|post-ast|pre-quarto → Pre`; `post-quarto|pre-render → PostCrossref`;
      `post-render|pre-finalize|post-finalize → Post`. Update the sentinel-default docs.
- [ ] Confirm HTML/revealjs output byte-identical after the split (pure refactor for the
      native path).

### Phase 4 — CustomNode → Q1-scaffold bridge (the crux)

- [ ] Tests: for each Q2 CustomNode (Callout, Float/FloatRefTarget, Theorem, Proof, Tabset,
      DecoratedCodeBlock, ConditionalBlock, Shortcode-resolved), snapshot the emitted
      Q1-scaffold `Div`/`Span` and confirm Q1's vendored parse handler reconstructs the
      matching Q1 CustomNode (RED first).
- [ ] Implement a serializer that lowers Q2 CustomNodes (carrying resolved crossref
      `plain_data`) into Q1's scaffold encoding (`_quarto.ast.make_scaffold` shape / classed
      Divs) that the vendored `customnodes/` **parse** handlers recognize. This is the inverse
      of Q2's Normalization sugar.
- [ ] Configure the vendored chain to run **only** the presentation groups for the fork:
      import `customnodes/` (parse+render), `layout/`, `quarto-post/`, `quarto-finalize/`;
      **disable** `quarto-init`, `normalize`, `quarto-pre`, `crossref` groups (Q2 already did
      that semantic work — re-running risks double numbering / conflicts).
- [ ] Skip the **Navigation** phase for non-HTML (HTML/website chrome; no analog).

### Phase 5 — Migrate the pilot format Cut B → Cut A; establish parity

- [ ] Repoint the `latex` path: cut at **`PostCrossref`** (not `pre`), emit the JSON AST via
      the Phase-4 bridge, invoke Pandoc with only the presentation filter groups.
- [ ] Verify crossref numbers, callouts, theorems now come from **Q2's** front end and match
      the HTML path (the divergence Phase 1 tolerated is now closed).
- [ ] Golden-parity vs Q1 for the full smoke set; snapshot AST-at-cut.
- [ ] Retire the Cut B scaffold pipeline (or keep behind a debug flag for A/B diffing).

### Phase 6 — Roll out remaining Tier-1 / Tier-2 formats

- [ ] For each Tier-1 format (docx, odt, epub, jats, asciidoc, gfm/commonmark, ipynb,
      beamer, context, pptx, typst): golden-parity fixtures → invocation builder entry →
      enable in `render.rs` → verify end-to-end. One format per sub-task.
- [ ] Tier-2 post-processing: pdf (latexmk/tectonic), typst compile (binary already located),
      epub packaging, `--reference-doc` for docx/pptx, svg→pdf image conversion
      (`pdf-images.lua` analog). Model as post-`PandocWriteStage` steps.

### Phase 7 — Tier-3 custom-writer formats (separate sub-epics)

- [ ] dashboard, email, confluence, hugo, llms-txt: each needs its Q1 **custom Lua writer**
      vendored and invoked via `pandoc --to <writer>.lua`, plus its TS orchestration ported.
      Scope as independent sub-epics after Tier-1/2 lands.

### Cross-cutting / watch-list

- **Templates:** Tier-1 uses Pandoc `--template`; evaluate whether `quarto-doctemplate` should
  pre-expand Quarto-specific partials before handing to Pandoc.
- **Book/manuscript assembly:** the catalog flagged `book-cleanup`/`book-numbering`/`cites`
  as blocked on a multi-chapter merge pipeline Q2 lacks. Single-file-book (latex/pdf) formats
  depend on it; treat as a prerequisite for book-in-non-HTML, not for article-in-non-HTML.
- **`content-hidden` / profiles:** absent in Q2 (catalog). `when/unless-format` gating is
  format-aware and will matter once multiple formats exist; may need porting alongside Phase 6.
- **Do not** introduce a DOM postprocessor (CLAUDE.md). All format work is Lua-in-Pandoc or
  AST transforms — never post-hoc string/DOM mutation.

### Definition of done (epic)

- [ ] All Tier-1 + Tier-2 formats render end-to-end through Cut A with golden-parity to Q1.
- [ ] Crossref/callout/theorem semantics are computed once and identical across HTML and all
      Pandoc formats.
- [ ] `PostCrossref` seam exposed to user filters (8→3 projection) and documented.
- [ ] Vendored filters pass `cargo xtask lint`; full `cargo xtask verify` green.
- [ ] Tier-3 tracked as follow-on sub-epics.
