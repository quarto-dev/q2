# `format: revealjs` — Presentation Support for Quarto 2

**Status:** Phase 1 (render-side Tier-1 vertical slice) **complete** — `q2 render`
produces standalone reveal.js 6 decks. Next: Phase 1P (preview parity, GA gate).
**Created:** 2026-06-08
**Branch:** `feature/revealjs` (integration line; not yet pushed)
**Epic strand:** bd-67yja58s
**Phase 1 strand:** bd-2m4wanyd — sub-tasks:
bd-n8mopfqc (1.1+1.8 tests/e2e),
bd-amms9jvg (1.2 vendor assets),
bd-30mk70ld (1.3 slide-split stage),
bd-izay61p8 (1.4 format options),
bd-hr6orq7q (1.5+1.6+1.7 template/fork/gate/title — blocked by 1.2/1.3/1.4)

## Overview

Stand up real `format: revealjs` presentation support for Quarto 2, working
through `q2 render` (standalone single-file decks) and inside larger website
projects. Today:

- The Rust render path has only **scaffolding**: `FormatIdentifier::Revealjs`
  exists (`crates/quarto-core/src/format.rs:35`, marked `is_native` /
  `is_html_based`), but there is **no writer and no template**. `q2 render`
  either bails at the CLI gate (`crates/quarto/src/commands/render.rs:608`,
  "Only HTML is available") or, for documents the gate lets through, falls
  through to the plain HTML template — which is the symptom the user observed.
- The **hub-client preview** has a separate, functional-but-minimal renderer:
  `RevealjsReactAstSlideRenderer.tsx` using `@revealjs/react@0.2.0` over
  `reveal.js@6.0.0`, with AST→slide splitting hand-written in TypeScript
  (`ReactAstSlideRenderer.tsx`). It supports basic slide splitting, a title
  slide, KaTeX, images, and loads Notes/Search/Zoom/Menu plugins — but lacks
  fragments, incremental lists, columns, backgrounds, speaker-notes UI, theme
  selection, and per-slide attributes.

The goal is full-as-possible **Quarto 1 parity** for presentations, delivered
in phases.

## Key decisions (confirmed with user 2026-06-08)

1. **Render architecture — static HTML now, architected to converge later.**
   `q2 render` emits a Quarto-1-style **static reveal.js HTML scaffold**
   (`<div class="reveal"><div class="slides"><section>…</section></div></div>`
   + `Reveal.initialize(...)`), using **reveal.js 6 core** (the
   framework-agnostic library — no React). The preview keeps `@revealjs/react`
   for now.

   **Why not "unify on React":** `@revealjs/react` is a thin lifecycle wrapper,
   **not** an HTML→React parser. Every reveal.js feature (fragments,
   backgrounds, auto-animate, transitions, plugins) is driven by `<section>` +
   `data-*` attributes read by the **core** library; the React wrapper just maps
   friendly props onto those same `data-*` attributes and manages
   init/destroy. A pure-Rust static-file render needs only the core + a
   `Reveal.initialize()` call. Making `q2 render` emit *React* output would
   require running a JS/React runtime in the render path (Rust can't) or
   shipping the whole SPA per deck (breaks single-file standalone). React buys
   nothing the core doesn't already provide.

   **Convergence point:** because the static path (`<section data-…>`) and the
   React path (`<Slide backgroundColor=…>`) both bottom out on the *same*
   reveal.js 6 core and the *same* `data-*` vocabulary, both renderers can
   share **one slide-splitting + attribute-mapping contract**. We build the
   splitter as a reusable AST transform (Rust, WASM-compatible) emitting the
   canonical slide structure. The preview drops its bespoke TS splitter
   (`ReactAstSlideRenderer.tsx` `parseSlides`) and renders the shared sections,
   with golden tests keeping render↔preview honest.

4. **`q2 preview` parity is a landing gate (React mirror), confirmed
   2026-06-08.** `q2 preview` on `format: revealjs` must feel like a live
   preview of the `q2 render` output — a real reveal.js slideshow. This mirrors
   exactly how `format: html` preview works today (verified this session):

   - `q2 preview` runs the **real Rust pipeline in WASM** (`render_page_for_preview`
     → `render_qmd_to_preview_ast`, `crates/wasm-quarto-hub-client/src/lib.rs:1166`),
     mapping `html` → the `q2-preview` pseudo-format. It stops at the **AST**.
   - A **TS/React renderer** (`ts-packages/preview-renderer/src/q2-preview/…`)
     turns that AST into DOM and is hand-kept in parity with the Rust HTML
     writer (`crates/pampa/src/writers/html.rs`) — the job of the
     `/preview-render-parity` skill.

   So "two implementations kept in sync" really means **only the AST→DOM step
   is duplicated** (Rust writer for render, React for preview); everything
   upstream of the AST is shared Rust-in-WASM. revealjs adopts the *same* shape:

   - **Slide-split is shared, single-implementation** (the Phase-1.3 Rust/WASM
     stage). Both `q2 render` (Rust reveal writer → static HTML) and `q2 preview`
     (React reveal renderer → DOM) consume the *same* split AST. This removes
     the worst drift source — today the preview's `ReactAstSlideRenderer.tsx`
     does its own TS `parseSlides`, divergent from Rust.
   - **Preview render is the React-mirror path (option B1):** `q2 preview` uses
     `@revealjs/react` to build `<Slide>`s from the shared-split AST, kept in
     parity with the Rust static reveal writer via golden tests. Chosen for
     consistency with the html model and to reuse `useCursorToSlide`,
     `useSlideThumbnails`, and live-incremental updates. (The rejected
     alternative B2 — iframe the actual Rust static HTML for byte-exact parity —
     would be a new mechanism inconsistent with html and would force
     re-plumbing cursor-sync/thumbnails/live-edit across the iframe boundary.)
   - **GA gate:** render-path increments may merge to the integration branch
     render-only, but the **epic cannot land in `main` until `q2 preview` shows
     an in-parity Tier-1 revealjs slideshow.** Tracked as a blocking child of
     the epic (see Phase 1.5p / preview-parity strands).

5. **`format: revealjs` must work both standalone and inside a website — the
   first heterogeneous-format project (confirmed 2026-06-08).** Two contexts:

   - **No `_quarto.yml` (implicit single-file project):** `format: revealjs` is
     resolved from the document's own front-matter. This is the Phase-1 path and
     must work from the first slice — no project context required.
   - **Inside a Q2 website project:** a `revealjs` deck lives alongside `html`
     pages. **This is likely the project pipeline's first encounter with
     multiple output formats in one project.** Current state (verified this
     session): `ProjectPipeline` threads a *single* `self.format` through both
     passes (`crates/quarto-core/src/project/orchestrator.rs:674,723,1034,1190`);
     a per-file `format: revealjs` override is **not** honored today — every file
     gets the project's one format. The render *cache* already keys on
     `format_id` (`crates/quarto-core/src/project/cache_key.rs:111`), so it is
     format-aware; the gap is **format resolution + threading per document**, not
     caching.

   No resource embedding / self-containment requirement is implied here — the
   ask is purely that per-file format be respected in a project, and that the
   no-project case work. The website-project work is **Phase 8**; the
   single-file case is **Phase 1**.

2. **reveal.js version: 6.0.0 (matches preview), minimal themes first.**
   Vendor reveal.js 6 `dist/` into `resources/revealjs/` (External Sources
   Policy — must be local, never reference `external-sources/`). Start with the
   stock reveal themes (white/black/…) plus a *minimal* Quarto layer (logo,
   footer, sizing, centering). Full Quarto-1 `quarto.scss` + 12-theme + brand
   parity is deferred to a later phase. (Note: Quarto 1 bundles reveal.js
   **5.1.0**; porting its SCSS forward to 6.x is the deferred cost.)

3. **First implementation slice: Tier-1 end-to-end vertical slice.**
   `q2 render talk.qmd` → an openable, navigable standalone deck. Prove the
   whole pipeline path (slide split → template → assets → initialize) before
   layering authoring features. End-to-end verification through the real binary
   is mandatory (see CLAUDE.md "End-to-end verification before declaring
   success").

## Architecture: where revealjs slots into the pipeline

Reference (from investigation 2026-06-08):

- Format infra: `crates/quarto-core/src/format.rs` (`FormatIdentifier::Revealjs`
  already present; output extension already `"html"`).
- CLI gate to lift: `crates/quarto/src/commands/render.rs:608`.
- Writer-side options container: `crates/quarto-core/src/render.rs`
  (`FormatOptions { html, json }` → add `revealjs`).
- Pipeline stage order: `crates/quarto-core/src/pipeline.rs:251-325`. The
  pipeline is format-agnostic through the transform stages; the format-specific
  fork is at `RenderHtmlBodyStage` (`:325`) and `ApplyTemplateStage` (`:328`).
- HTML writer: `crates/pampa/src/writers/html.rs`.
- Templates: `crates/quarto-core/src/template.rs`
  (`MINIMAL_HTML_TEMPLATE`, `FULL_HTML_TEMPLATE`) and template selection in
  `crates/quarto-core/src/stage/stages/apply_template.rs`.
- Metadata/format-config merge: `crates/quarto-core/src/stage/stages/metadata_merge.rs`
  (`resolve_format_config()` flattens `format.revealjs.*`).

**Plan:** add a `RevealSlidesStage` (slide-splitting AST transform) before the
HTML-body render, a reveal-specific branch in body render / template
application, a `RevealjsFormatOptions` struct + metadata plumbing, and a
vendored asset + template story. The slide-split transform is the shared
contract from decision (1).

## Quarto 1 feature surface (parity target)

Catalogued from `external-sources/quarto-cli/` (2026-06-08). Tiers drive phasing.

**Tier 1 — core slide construction / chrome / theming (MVP):**
slide-level splitting (default H2 horizontal, H3 vertical), H1 section title
slides, explicit `---`/HR breaks, auto title slide (title/subtitle/author/
institute/date), TOC slide; navigation/controls/progress/slide-number/hash;
1 theme + minimal quarto layer (logo, footer, width/height/center).

**Tier 2 — authoring / animation (high value):**
incremental lists (`.incremental`/`.nonincremental`), fragments (`.fragment`
+ variants), columns (`::: columns` / `::: {.column width=…}`), speaker notes
(`::: notes`), asides/footnote coalescing, transitions (per-deck + per-slide),
auto-animate (`{auto-animate=true}` + `data-id`), code-line highlighting
(`code-line-numbers="4-5|7"`), `output-location`.

**Tier 3 — media/backgrounds, plugins, advanced:**
slide backgrounds (color/image/video/iframe/size/position/opacity/parallax),
`.absolute`, `.r-stretch`/`auto-stretch`, `.r-fit-text`, `.scrollable`,
`.smaller`; plugins (menu, chalkboard, multiplex, pdf-export, line-highlight,
tone); scroll-view; `revealjs-config`/`revealjs-url`/`disable-layout`.

**Theming parity (deferred):** port Q1 `quarto.scss` + 12 themes to reveal 6,
brand integration (`crates/quarto-sass/src/brand_layer.rs` already references
revealjs).

## Phasing

Each phase is its own braid strand under the epic. Detailed checklist below is
**Phase 1 only**; later phases are sketched and will be expanded into their own
plan sections / strands when we reach them.

- **Phase 1 — Tier-1 vertical slice (MVP, `q2 render`).** ← first implementation
  session.
- **Phase 1P — `q2 preview` revealjs path (Tier-1 parity). 🔒 GA landing gate.**
  Wire `render_page_for_preview` / `q2-preview-spa` to route `format: revealjs`
  to a slides preview that consumes the **shared Phase-1.3 split AST**; replace
  the preview's TS `parseSlides` with the shared split; render via
  `@revealjs/react` (React-mirror / B1); add render↔preview golden parity tests
  for the Tier-1 feature set. The epic **cannot land in `main`** until this is in
  parity (render increments may merge to the integration branch render-only).
  Each later authoring phase extends preview parity + its parity tests in step.
- **Phase 2 — Authoring features** (incremental, fragments, columns, notes,
  asides) — render *and* preview, with parity tests.
- **Phase 3 — Media & backgrounds** (`.absolute`, stretch, fit-text, slide
  backgrounds).
- **Phase 4 — Code features** (code-line-numbers/highlight steps,
  output-location, code-block-height).
- **Phase 5 — Transitions & auto-animate.**
- **Phase 6 — Plugins & chrome** (menu, chalkboard, multiplex, pdf-export,
  slide-number formats).
- **Phase 7 — Theming parity** (port `quarto.scss` + 12 themes, brand).
- **Phase 8 — Website integration** (revealjs decks inside website projects:
  nav, listings of talks, cross-doc links, project-level format defaults).

**Standing obligation (every phase from Phase 2 on):** each authoring feature
ships in *both* the Rust reveal writer and the React-mirror preview, with a
render↔preview parity test, before that phase is done. Preview parity is not a
trailing phase — Phase 1P establishes the path and every subsequent phase keeps
it in lockstep.

---

## Phase 1 — Tier-1 vertical slice (MVP)

**Goal:** `cargo run --bin q2 -- render talk.qmd` produces a standalone,
openable, navigable reveal.js 6 deck from a real `.qmd`, with the title slide,
one theme, working navigation/controls, and the core format options plumbed
end-to-end.

Per CLAUDE.md TDD: **tests/specifications first**, then implementation, then
**end-to-end verification through the binary** with the invocation + inspected
output recorded here.

> **Root cause of the observed symptom (found 2026-06-08).** `q2 render
> talk.qmd` produces plain HTML because the CLI never reads the document's
> front-matter `format:`. `crates/quarto/src/commands/render.rs:605` sets
> `format_str = args.to.as_deref().unwrap_or("html")` and threads that single
> format through `ProjectPipeline`; front-matter `format: revealjs` is ignored
> unless `--to revealjs` is passed. So Phase 1 must add **front-matter format
> resolution** (read the document/`_quarto.yml` `format:` when `--to` is
> absent), not just lift the `is_native()` gate. This is also the seed of the
> Phase-8 multi-format work (decision 5) — but Phase 1 only needs the
> single-format-per-render case.

**Test layers (decided 2026-06-08):**
- **Pipeline-level (fast TDD red/green):** `crates/quarto-core/tests/integration/revealjs_format.rs`,
  calling `render_to_file(path, "revealjs", &RenderToFileOptions{quiet:true,..}, NativeRuntime)`
  (`crates/quarto-core/src/render_to_file.rs:161`). Asserts on the written HTML.
  This passes `"revealjs"` explicitly, so it tests the *pipeline*, bypassing the
  CLI format-defaulting bug.
- **CLI-level (real binary, real user path):** `crates/quarto/tests/integration/revealjs_cli.rs`,
  spawning `CARGO_BIN_EXE_q2` (pattern from `render_cli_e2e.rs`). At least one
  test renders a deck via **front-matter `format: revealjs` with no `--to`** to
  pin the root-cause fix.

### Phase 1 work items

**1.1 — Test fixtures & specification (TDD first).** ✅ tests written & confirmed RED
- [x] Fixture decks inline in the tests (`FLAT_DECK` = title + 3 H2;
      `RICH_DECK` = section header + vertical subslide + code + inline math).
- [x] Golden-output strategy: whitespace-insensitive structural string
      assertions on the `.reveal/.slides/section` tree + `Reveal.initialize`
      config; integration-test layout used (no top-level `tests/<name>.rs`).
- [x] Pipeline-level failing tests in
      `crates/quarto-core/tests/integration/revealjs_format.rs` (6 tests, all
      RED): scaffold, title slide, exact section count, init options, theme
      stylesheet, rich-deck structure. Route through `render_to_file(_,
      "revealjs", _, _)`.
- [x] CLI-level failing tests in
      `crates/quarto/tests/integration/revealjs_cli.rs` (2 tests, RED, via
      `CARGO_BIN_EXE_q2`): front-matter `format: revealjs` with no `--to`
      (root-cause regression) + explicit `--to revealjs`. Confirmed the
      no-`--to` case currently emits 974 bytes of plain HTML.

**1.2 — Vendor reveal.js 6 assets.** ✅ done
- [x] Vendored reveal.js 6.0.0 (MIT) into `resources/revealjs/`
      (`reset.css`, `reveal.css`, `reveal.js`, `theme/white.css`, `LICENSE`) +
      `README.md` documenting source/version. Copied from `node_modules/`.
- [x] Embedded via `include_str!` (binary stays single; output is a
      self-contained file). `cargo xtask lint` green.

**1.3 — Slide-construction transform (`RevealSlidesTransform`).** ✅ done
- [x] `crates/quarto-core/src/revealjs/slides.rs` — `build_reveal_slides(blocks,
      slide_level)` implements Pandoc's slide-level algorithm: `< N` →
      section-divider stack, `== N` → slide (vertical if in a stack), `> N` →
      in-slide heading, `HorizontalRule` → break. Emits `Div(.section)` (writer
      serializes `<section>`); header id/classes/attrs hoisted onto the
      section. WASM-safe (pure AST, no native deps). Implemented as an
      `AstTransform` (not a separate stage) replacing `TitleBlockTransform` +
      `SectionizeTransform` for revealjs in `build_transform_pipeline`.
- [x] `transform.rs` also synthesizes the `<section id="title-slide">` from
      metadata (title/subtitle/author/date).
- [x] 11 unit tests covering every split boundary (flat, divider/vertical,
      deep-heading-in-slide, HR, preamble, no-headers, empty, class hoisting,
      slide-level 3).

**1.4 — Reveal config from metadata.** ✅ done (revised approach)
- [x] `crates/quarto-core/src/revealjs/assemble.rs` `reveal_config_json()`
      reads the merged/flattened metadata (`format.revealjs.*` → top level via
      `resolve_format_config`, keyed on `identifier.as_str()`) and maps to
      `Reveal.initialize` camelCase keys: `controls`/`progress`/`center`/`hash`
      (default true), `transition` (default "slide"), `transitionSpeed`,
      `slideNumber`, `width`, `height`. **Note:** read via `as_plain_text()`
      (YAML scalars are `PandocInlines`; `as_str()` misses them).
- [x] **Deviation from sketch:** no typed `RevealjsFormatOptions` in
      `FormatOptions` yet — reading from flattened metadata at assembly time is
      simpler and sound for Tier-1. Promote to a typed struct when the preview
      (Phase 1P) needs the shared contract in code. _Tracked as a follow-up._
- [x] Unit tests: defaults + YAML→reveal-key mapping.

**1.5 — Reveal document assembly + template fork.** ✅ done (revised approach)
- [x] `assemble.rs` `render_revealjs_document(body, meta)` wraps the slide body
      in the reveal scaffold (`.reveal > .slides`) with **inlined** reset/
      reveal/theme CSS + reveal.js + `Reveal.initialize({…})`. Self-contained
      single file. **Deviation:** direct string assembly, not the
      `quarto-doctemplate` engine (avoids `$`-collision with rendered body and
      keeps reveal asset inlining self-contained; revisit if partials needed).
- [x] Forked `apply_template.rs` on `FormatIdentifier::Revealjs` (new `None if
      …Revealjs` arm) → calls the reveal assembler, bypassing Bootstrap
      templates.
- [x] Body serializes as `<section>` via the existing HTML writer (no writer
      change needed — `RevealSlidesTransform` produces `.section` Divs).
- [x] **Also fixed:** `CompileThemeCssStage` now skips revealjs (reveal `theme:`
      is not a Bootswatch name; the stage would mis-validate `theme: white`).
      Caught by end-to-end verification, not unit tests.
- [x] Asset default: **inlined/self-contained** for Tier-1 (binary stays single
      via `include_str!`). Linked-assets + `embed-resources` is a later phase.

**1.6 — CLI gate + front-matter format resolution.** ✅ done
- [x] revealjs already passes the `is_native()` gate; widened the
      not-yet-supported message to mention revealjs.
- [x] **Resolved the target format from front-matter when `--to` is absent**
      (root-cause fix): `detect_single_input_format()` /
      `format_key_from_frontmatter()` in `render.rs` read the document's
      front-matter `format:` (scalar, or first key of a `format:` map) for a
      single `.qmd` input. `--to` stays an explicit override. Best-effort
      (parse failure → `"html"`). Single-format-per-render; project/per-file is
      Phase 8 (decision 5).

**1.7 — Title slide.** ✅ done (folded into 1.3)
- [x] `build_title_slide()` in `transform.rs` emits `<section id="title-slide">`
      with `<h1 class="title">` + classed subtitle/author/date paras from
      metadata. (Plain-text extraction for Tier-1; rich-inline titles a later
      refinement.)

**1.8 — End-to-end verification (mandatory).** ✅ done
- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` green — **9565 passed, 0 failed** (no
      regressions from the pipeline/apply_template/compile_theme_css changes).
- [x] `cargo xtask lint` green; clippy clean on touched files.
- [ ] Full `cargo xtask verify` (WASM leg) — deferred to just before requesting
      push (expensive; reveal module is WASM-safe by construction). _Pending._
- [x] **E2E through the binary** (recorded below).

#### Phase 1 end-to-end record (2026-06-08)

Invocation: `cargo run --bin q2 -- render /tmp/revealjs-e2e/talk.qmd`
(fixture: title/subtitle/author/date + 2 H2 slides + a `# Section` divider +
`## Under the Hood` + an H3, code block, inline math; `format.revealjs` =
`theme: white, transition: fade, slide-number: true`).

Output: a **749 KB self-contained `talk.html`**. Inspected:

```
<section id="title-slide" class="section title-slide"><h1 class="title">…
<section id="why-quarto-2" class="section"><h2>…
<section id="some-code" class="section"><h2>… (class="sourceCode" highlighting)
<section id="section-details" class="section">        ← H1 stack
  <section class="section"><h1>…                      ← divider slide
  <section id="under-the-hood" class="section"><h2>…  ← vertical sub-slide
    <h3 id="a-vertical-subslide">…                    ← H3 as in-slide content
Reveal.initialize({ "controls":true,…, "transition":"fade", "slideNumber":true })
```

Confirmed: reveal scaffold, 2-level nesting (H1 stack + vertical H2, H3 kept
in-slide per slide-level 2), title slide with subtitle/author/date, inline math
(`<span class="math">`/`\(E…`), syntax-highlighted code, and the reveal.js
library inlined (`I.Reveal=b()`). Output inspected via grep of the rendered
HTML; not yet opened in a live browser (a manual browser pass is still worth
doing before the epic lands).

### Phase 1 open questions / risks

- **Slide split vs. `sectionize` — RESOLVED 2026-06-08: Option A (reveal-specific
  slide-construction stage; skip generic sectionize for revealjs).** Decided by
  the "land similar to Q1" criterion. Evidence: Q1 delegates reveal slide-tree
  construction entirely to **Pandoc's reveal.js writer** (driven by
  `slide-level`, hardcoded default 2 at `format-reveal.ts:149`); the generic
  HTML5 section-div machinery is *not* what builds reveal slides, and the Lua
  `reveal.lua` filter is purely decorative. Pandoc's writer also hoists header
  attributes onto the enclosing `<section>` (`format-reveal.ts:607`). Since Q2
  has no Pandoc, the faithful port is a Rust `RevealSlidesStage` implementing
  Pandoc's slide-level algorithm, with generic `sectionize` **skipped** for
  revealjs (Pandoc keeps reveal slide-construction separate from
  `--section-divs`). The existing HTML writer already serializes
  `Div(.section)` → `<section>` (`html.rs:1467`) and passes key-value attrs
  through as `data-*`, so the stage emits section Divs and needs **no writer
  change**.

  **Pandoc slide-level semantics to replicate (N = slide-level, default 2):**
  header level `< N` → section-divider slide wrapping the following slides as a
  vertical stack; `== N` → a slide; `> N` → ordinary heading content *within*
  the current slide; `HorizontalRule` → slide break; title slide auto-generated
  from metadata; header attributes hoisted onto the `<section>`.
- **Single-file project (no `_quarto.yml`) must work in Phase 1.** `format:
  revealjs` resolved from document front-matter, implicit single-file project.
  This is the Phase-1 fixture path — no project context required. _Covered by
  Phase 1; see decision 5._
- **Asset mechanism: `include_dir!` (decided 2026-06-08).** Vendor reveal.js 6
  into `resources/revealjs/` and embed via `include_dir!`/`include_str!` so `q2`
  stays a **single self-contained binary** — no copy-to-output dependency, no
  runtime path to `external-sources/`. Keep `cargo xtask lint`
  (external-sources-in-macro) green. _Not self-containment of the **output
  HTML** — the deck may still link its assets; this is about the binary.

### Resource embedding vs. output self-containment (clarified)

These are two different axes and were briefly conflated in an earlier draft:
- **Binary:** reveal.js assets are embedded in `q2` via `include_dir!` (decided
  above). One binary, no external asset tree at runtime.
- **Output HTML:** whether the rendered deck inlines its assets
  (`embed-resources`) or links a sidecar dir is a *separate* knob with no special
  Tier-1 requirement — follow whatever the html format already does. Single-file
  *standalone* (the project goal) means "one `.qmd`, no `_quarto.yml`," **not**
  "one self-contained `.html`."

---

## Phase 2 — Authoring features (in progress)

**Goal:** the common presentation authoring constructs work in **both** `q2
render` and `q2 preview`, with a parity check each (standing obligation). Q1
parity target catalogued from `external-sources/quarto-cli/` (2026-06-08).

**Architecture — three classes of feature** (discovered 2026-06-08 by testing
the current pass-through + reading Q1):

1. **Pure class pass-through** — the AST already carries the class and *both*
   paths emit it (render via the HTML writer, preview via `previewRegistry`).
   reveal.js interprets it. **Works for free; needs only tests.**
2. **Element / CSS change** — the DOM element or styling must change
   (`Div(.notes)` → `<aside>`, columns flexbox). The AST→DOM step is
   implemented twice (Rust HTML writer + `previewRegistry`), so **both need the
   rule**, kept in parity by a golden test. CSS must be inlined for render and
   imported for preview.
3. **Structural generation** — markup not expressible as a pass-through class
   (incremental `<li class="fragment">` — Pandoc list items have no attr). Needs
   **writer-level** support.

**Current pass-through baseline (q2 render today):**
- `.fragment` / `.fragment .fade-out` → `<div class="fragment …">` ✅ (class 1)
- `.incremental` → class lands on the section/div but **no `<li class="fragment">`** ❌ (class 3)
- `.columns`/`.column` → classes present, **no flexbox CSS** ⚠️ (class 2)
- `.notes` → `<div class="notes">`, needs `<aside class="notes">` ❌ (class 2)

**Sequence (by value/effort, each = render + preview + parity test):**

- **2a — Fragments** (class 1). ✅ **DONE + browser-verified (2026-06-08).**
  `.fragment` + variant classes (`fade-out/up`, `grow`, `shrink`,
  `highlight-*`, `semi-fade-out`, `current-visible`) + `fragment-index` →
  `data-fragment-index` pass through in both paths. 3 render tests
  (`revealjs_features.rs`); preview inherits class pass-through (verified live:
  2 fragment divs `fragment` / `fragment fade-out`).
- **2b — Speaker notes** (class 2). ✅ **DONE + browser-verified (2026-06-08).**
  `Div(.notes)` → `<aside class="notes">` in the Rust HTML writer (parallel to
  `.section`→`<section>`) **and** `previewRegistry` `Div.tsx` (+ `NOTES` class
  const). reveal.css hides `aside.notes` on the slide — verified live
  (`display:none`, "These are speaker notes."). Render test + preview
  integration test. **Follow-up bd-0qaarvzx:** load the reveal **notes plugin**
  (S-key speaker view) in render scaffold + `RevealDeck` (both paths).
- **2c — Columns** (class 2). ✅ **DONE + browser-verified (2026-06-08).**
  `RevealColumnsTransform` rewrites `.column[width=X%]` → inline
  `style="flex-basis:X%"` (drops the bare `width`); runs in the `is_revealjs`
  branch (render + preview). **Single-source** `resources/revealjs/quarto-reveal.css`
  (`.reveal .columns{display:flex;gap}`, `.reveal .column{flex:auto}`) — inlined
  for render (`assemble.rs`), imported by `RevealDeck.tsx` for preview. `Div.tsx`
  passes the AST inline `style` through (CSS-string→React-object). 4 transform
  unit + 2 render + 1 preview tests. Verified live: both paths emit
  `style="flex-basis:40%/60%"`, `display:flex`, columns on the same row.
  **Gotcha hit:** preview features needing a Rust transform require the *full*
  WASM rebuild chain (`npm run build:wasm` → `build-q2-preview-spa` → `cargo
  build --bin q2`); a SPA-only rebuild leaves the preview on stale WASM.
- **2d — Incremental lists** (class 3). ✅ **DONE + browser-verified (2026-06-08).**
  Ported Pandoc's `writerIncremental` (confirmed via `external-sources/pandoc`
  HTML.hs:493-496 + quarto-cli that a filter *cannot* do this — list items have
  no `Attr`, in Pandoc and pampa alike). **Render:** `HtmlConfig.incremental_lists`
  (gated to revealjs) + `incremental_default`; `HtmlWriterContext.incremental`
  traversal state flipped by `.incremental`/`.nonincremental` Divs (forced off
  in note asides); list writers emit `<li class="fragment">`. **Preview:**
  `IncrementalContext` (enabled only in `RevealDeck`) flipped by Div classes +
  the section-class on `## Slide {.incremental}` (handled in `RevealDeck`/
  `SlideBody`); `BulletList`/`OrderedList` emit `<li class="fragment">` when
  enabled, html-preview path unchanged (editing preserved). 6 render + 4 preview
  tests. Verified live: the `.incremental` slide shows 3 `<li class="fragment">`.
- **2e-i — Asides** (class 2). ✅ **DONE + browser-verified (2026-06-08).**
  `::: {.aside}` → `<aside class="aside">` in the Rust writer + previewRegistry
  `Div.tsx` (alongside `.notes`); styled small/muted at the slide bottom via the
  single-source `quarto-reveal.css` (`.reveal .slides aside.aside{position:absolute;
  bottom:20px;font-size:0.6em;color:#6c757d}`). Render + preview tests; verified
  live. _(strand bd-0zosmiq8 closed.)_
- **2e-ii — Per-slide footnote coalescing** (class 3, most complex). _strand
  bd-9aknlx1j._ Collect each slide's footnotes into an `<aside><ol
  class="aside-footnotes">` at the slide bottom (noteref → `<sup>N</sup>`,
  per-slide numbering) and **suppress the trailing footnotes slide**; coalesce
  multiple `.aside`s per slide. Q1 ref: `format-reveal.ts:702-793`. **Current
  Q2:** footnotes form a trailing `section[role=doc-endnotes]` that
  `RevealSlidesTransform` turns into a final slide (functional, not Q1-faithful).
  **Approach:** a reveal-aware AST transform — *pure AST*, so it benefits **both**
  render and preview at once (like `RevealColumnsTransform`); runs after
  `FootnotesTransform` (which runs after slide construction, so the slide
  structure exists). _Not yet started._

**Phase 2 strands:** bd-bea550b0 (epic-child) — 2a bd-f8dpxwle (fragments),
2b bd-o5sg45fb (notes), 2c bd-34rd2y86 (columns), 2d bd-fy793w6i (incremental),
2e bd-0zosmiq8 (asides).

---

## Later phases (sketch — expand when reached)

**Phase 3 — Media/backgrounds:** per-slide `background-*` from heading attrs →
`data-background-*`, `.absolute` positioning, `.r-stretch` + `auto-stretch`,
`.r-fit-text`, `.scrollable`, `.smaller`.

**Phase 4 — Code:** `code-line-numbers` highlight steps (`"4-5|7|10"`),
`code-block-height`, `output-location: fragment/slide/column`, integration with
existing `CodeHighlightStage`.

**Phase 5 — Transitions/auto-animate:** per-deck + per-slide transitions,
`auto-animate` with `data-id` matching, animated code-line stepping.

**Phase 6 — Plugins/chrome:** vendor + wire menu, chalkboard, multiplex,
pdf-export, line-highlight, tone; slide-number formats; footer/logo polish.

**Phase 7 — Theming parity:** port `quarto.scss` + 12 themes to reveal 6; brand
layer (`crates/quarto-sass/src/brand_layer.rs`).

**Phase 8 — Website integration & heterogeneous-format projects:** the project
pipeline's first multi-format support — honor a per-file `format: revealjs`
override inside an otherwise-`html` project by threading a **per-document
format** instead of the orchestrator's single `self.format`
(`orchestrator.rs:674,723,1034,1190`; cache already keys on `format_id`). Plus:
nav/sidebar behavior for decks, listing decks as talks, cross-doc links from/to
slides, project-level `format: revealjs` defaults, profile checkpoint
interaction. (The standalone no-`_quarto.yml` case is handled in Phase 1.)

---

## Phase 1P — `q2 preview` revealjs path (Tier-1 parity) 🔒 GA landing gate

**Goal:** `q2 preview talk.qmd` (a `format: revealjs` deck) shows a live
reveal.js slideshow that is in parity with the `q2 render` output for the Tier-1
feature set. Adopts the html preview model (decision 4): shared Rust/WASM
slide-split AST + React-mirror render via `@revealjs/react`.

**This phase is a hard gate on the epic landing in `main`.** Render-path
increments may merge to the integration branch render-only; GA is blocked until
this is in parity.

### Phase 1P work items — core WORKING (verified in Chrome 2026-06-08)

- [x] **WASM foundation.** `q2-slides` pseudo-format (`format.rs` → `("html",
      Some("preview"))`) carries the AST/preview path; `is_revealjs_target()`
      generalizes the reveal transform check to `{revealjs, q2-slides}` (so the
      shared `RevealSlidesTransform` fires in the preview AST pipeline, which is
      `build_transform_pipeline` minus exclusions); `map_format_for_preview`
      maps `revealjs → q2-slides`; `RenderResponse.is_slides` flag added. Native
      test `render_qmd_to_preview_ast_builds_reveal_slides_for_q2_slides`.
- [x] **Shared split, single source.** The q2-preview AST path consumes the
      **same Rust `RevealSlidesTransform`** as `q2 render` — slide construction
      lives once, in Rust. (The hub-client *editor*'s old TS `parseSlides`
      still exists but is no longer used by the `q2 preview` path; fully
      deleting it from the editor is a separable cleanup — see follow-ups.)
- [x] **Preview routing.** `entry.tsx` `PreviewRoot` branches to `RevealDeck`
      when `meta.format` is `q2-slides`/`revealjs` (the target format is written
      into `meta.format` by `MetadataMergeStage`) — so **no cross-package
      `is_slides` postMessage plumbing** was needed for the render branch.
- [x] **Reveal renderer = previewRegistry + reveal shell (B1a).** New
      `ts-packages/preview-renderer/src/q2-preview/RevealDeck.tsx`: maps the
      shared-split `Div.section` AST onto `@revealjs/react`
      `<Deck>/<Slide>/<Stack>` (chosen over raw `Reveal.initialize` for robust
      live-edit lifecycle — the wrapper is a thin layer over reveal.js core),
      and renders slide **content** via the framework `<Node>` dispatcher (the
      shared `previewRegistry` mirror → code highlighting, KaTeX, etc. for
      free). `@revealjs/react` + `reveal.js` added to the package.
- [ ] **Parity tests.** _Follow-up._ Verified manually (Chrome) + the Rust
      foundation test, but no automated TS golden render↔preview parity test
      yet. Establish the slides analogue of `/preview-render-parity`.
- [x] **E2E both paths.** `q2 render` verified (Phase 1 record above). `q2
      preview /tmp/.../talk.qmd` verified live in Chrome (DevTools MCP):
      `.reveal/.slides` present, **4 top-level sections matching render**
      (title, Why Quarto 2?, Some Code, and `Section: Details` as a **stack with
      2 vertical sub-slides**), reveal root `…has-vertical-slides
      has-horizontal-slides ready`, controls+progress, syntax-highlighted code +
      KaTeX in slides, keyboard/control nav advances slides, **no console
      errors**. Screenshot of the title slide inspected.

### Phase 1P follow-ups (tracked, not GA-blocking for the basic experience)

- Section `id`s not yet passed onto `<Slide>` (hash nav is off in preview; ids
  matter for cross-doc anchors — wire when needed).
- Reveal **config-option parity**: preview uses default `Deck` config; render
  reads `format.revealjs.*` (transition/slide-number/…). For `q2-slides` those
  keys aren't flattened (base format is `html`), so the preview would need to
  read `meta.format.revealjs.*` (or flatten for `q2-slides`) to match. Minor;
  defaults align for the common case.
- Automated TS golden render↔preview parity test (above).
- Retire the hub-client *editor*'s `parseSlides`/`RevealjsReactAstSlideRenderer`
  in favor of `RevealDeck` (separable from the `q2 preview` path).

## References

- Investigation 2026-06-08 (this session): hub-client renderer map, q2 render
  pipeline map, Quarto 1 feature catalog.
- Existing strands: bd-74qv (Quoted inline in slides renderer), bd-1kor9
  (callout output for revealjs/typst/latex).
- `external-sources/quarto-cli/src/format/reveal/` (Q1 reference; read-only).
- `.claude/rules/integration-tests.md`, `.claude/rules/wasm.md`, External
  Sources Policy in `CLAUDE.md`.
