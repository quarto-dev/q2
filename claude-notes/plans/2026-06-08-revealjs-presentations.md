# `format: revealjs` — Presentation Support for Quarto 2

**Status:** Planning (not yet approved for implementation)
**Created:** 2026-06-08
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

**1.2 — Vendor reveal.js 6 assets.**
- [ ] Copy reveal.js 6 `dist/` (reveal.js, reveal.css, one theme, the plugins
      we need) into `resources/revealjs/` with a `README.md` documenting source
      + version (mirror `resources/scss/README.md` convention). Never reference
      `external-sources/` or `node_modules/` at compile/runtime.
- [ ] Confirm `cargo xtask lint` (external-sources-in-macro rule) stays green if
      assets are embedded via `include_dir!`/`include_str!`.

**1.3 — Slide-splitting AST transform (`RevealSlidesStage`).**
- [ ] New stage that walks the post-transform Pandoc AST and emits the canonical
      slide structure (the shared contract from decision 1). Rules: split at
      `slide-level` (default H2) for horizontal slides, H3 for vertical/nested,
      H1 → section title slide, explicit `HorizontalRule` → slide break, auto
      title slide from metadata. WASM-compatible (no native-only deps; honor
      `.claude/rules/wasm.md`).
- [ ] Represent the split as structure the body writer can serialize to
      `<section>` (likely Div wrappers with reveal classes, or a typed
      intermediate). Keep the `data-*` attribute mapping centralized so the
      preview can reuse it later.
- [ ] Unit tests for split boundaries (each rule + interaction: HR inside a
      section, H3 before any H2, doc with no headings, etc.).

**1.4 — `RevealjsFormatOptions` + metadata plumbing.**
- [ ] Add `revealjs: RevealjsFormatOptions` to `FormatOptions`
      (`crates/quarto-core/src/render.rs`). Tier-1 fields: `theme`,
      `transition`, `transition_speed`, `slide_number`, `controls`, `progress`,
      `center`, `hash`, `incremental` (global default), `width`, `height`,
      `slide_level`, `logo`, `footer`.
- [ ] Ensure `resolve_format_config()` surfaces `format.revealjs.*` to these
      options. Map option names to `Reveal.initialize` config keys.
- [ ] Tests: option round-trips from YAML → `Reveal.initialize` JSON.

**1.5 — Reveal template + body/template fork.**
- [ ] Add a `REVEALJS_TEMPLATE` (reveal scaffold: head with reveal.css + theme +
      configured CSS, body `.reveal > .slides > $body$`, scripts importing
      reveal.js core + plugins, `Reveal.initialize({…})`). Use the
      `quarto-doctemplate` engine like the existing templates.
- [ ] Fork template selection in `apply_template.rs` on
      `FormatIdentifier::Revealjs`.
- [ ] Fork body rendering so slides serialize as `<section>` (Phase 1.3 output)
      — `RenderHtmlBodyStage` branch or a revealjs body writer in
      `crates/pampa/src/writers/`.
- [ ] Wire reveal assets into the artifact/resource-resolver path so the
      standalone file references (or embeds) them correctly. Decide self-
      contained vs. linked default (Q1 default is *non*-self-contained; honor
      `embed-resources` later).

**1.6 — CLI gate + front-matter format resolution.**
- [ ] Allow `FormatIdentifier::Revealjs` through `render.rs:608` (it already
      reports `is_native()`; confirm the gate change doesn't open non-native
      formats).
- [ ] **Resolve the target format from front-matter when `--to` is absent**
      (root-cause fix): read the document (and, in a project, `_quarto.yml`)
      `format:` key instead of defaulting to `"html"`. Keep `--to` as an
      explicit override. Single-format-per-render is enough for Phase 1; the
      multi-format/project case is Phase 8 (decision 5). Use a sound
      resolution path (parse the merged front-matter `format`), not a string
      hack.

**1.7 — Title slide.**
- [ ] Render title/subtitle/author/institute/date into the first `<section>`
      (reveal-conventional markup). Reuse `DocumentProfile` authors/title where
      possible rather than re-extracting.

**1.8 — End-to-end verification (mandatory, record here).**
- [ ] `cargo build --workspace` clean.
- [ ] `cargo nextest run --workspace` green (monorepo-wide — pampa changes can
      break downstream crates).
- [ ] `cargo xtask verify` (full, since `quarto-core`/`pampa` feed the WASM
      leg).
- [ ] Run `cargo run --bin q2 -- render <fixture>/talk.qmd`; **open the output
      in a browser**, navigate slides, confirm title slide + theme + controls.
      Record the exact invocation + an HTML snippet + an explicit "inspected"
      note in this doc.

### Phase 1 open questions / risks

- **Slide split placement vs. existing transforms.** Must run after Quarto AST
  transforms (callouts/crossref/sectionize) but the split likely interacts with
  `sectionize` (which already wraps headings in `section` Divs — see the
  preview's "section Div" splitting path). Need to confirm whether to consume
  sectionize output or split independently. _Deferred to implementation
  (user, 2026-06-08): decide with the real code in front of us. When it comes
  up in 1.3, surface a **concrete** question to the user (show the actual
  sectionize AST shape + the two options it implies) rather than guessing — the
  user is indifferent a priori but will answer a specific question._
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

## Later phases (sketch — expand when reached)

**Phase 2 — Authoring:** `.incremental`/`.nonincremental` list handling,
`.fragment` (+ fade/grow/highlight variants), columns layout filter, `::: notes`
→ reveal notes, footnote/aside coalescing (`reference-location`).

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

### Phase 1P work items

- [ ] **Shared split, single source.** Make the preview consume the Phase-1.3
      Rust/WASM `RevealSlidesStage` output (the split AST), and **delete the TS
      `parseSlides`** in `ReactAstSlideRenderer.tsx`. Confirm `RevealSlidesStage`
      runs in the WASM preview pipeline, not only native render.
- [ ] **Preview routing.** Wire `render_page_for_preview` /
      `crates/wasm-quarto-hub-client/src/lib.rs` + `q2-preview-spa` to detect
      `format: revealjs` and route to a slides preview (a `q2-slides`-style
      pseudo-format), instead of the `q2-preview` html iframe. Today the reveal
      React renderer is wired into the collaborative editor, *not* the
      `q2-preview-spa` path — this is net-new.
- [ ] **React-mirror renderer (B1).** Render the shared-split AST with
      `@revealjs/react` `<Deck>/<Slide>`, mapping the canonical `data-*`
      attributes from the split. Reuse `useCursorToSlide` + `useSlideThumbnails`;
      enable live-incremental updates. Un-hardcode the theme to follow the
      configured `theme`.
- [ ] **Parity tests.** Golden render↔preview parity tests for the Tier-1 set
      (slide boundaries, title slide, section slides, vertical slides, the
      core `Reveal.initialize` options). Establish the slides analogue of the
      `/preview-render-parity` mechanism so later phases extend it per feature.
- [ ] **E2E both paths.** Verify `q2 render` (browser-open the static deck) and
      `q2 preview` (live slideshow in the running preview) on the same fixture;
      record both invocations + inspected notes here.

## References

- Investigation 2026-06-08 (this session): hub-client renderer map, q2 render
  pipeline map, Quarto 1 feature catalog.
- Existing strands: bd-74qv (Quoted inline in slides renderer), bd-1kor9
  (callout output for revealjs/typst/latex).
- `external-sources/quarto-cli/src/format/reveal/` (Q1 reference; read-only).
- `.claude/rules/integration-tests.md`, `.claude/rules/wasm.md`, External
  Sources Policy in `CLAUDE.md`.
