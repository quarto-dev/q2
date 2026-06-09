# Reveal.js docs page + per-feature example projects

**Strand:** bd-ixdktocp (discovered-from the revealjs epic bd-bea550b0)
**Date:** 2026-06-09
**Writing skill:** apply `reader-expectations-prose` (Gopen structure) when drafting prose.

## Overview

Document the reveal.js authoring features shipped in the revealjs epic
(Phases 1–2) on the Quarto 2 docs site, in the style Quarto 1's
quarto-web uses for `docs/presentations/revealjs/index.qmd`: a short
prose explanation of each feature followed by a runnable example.

Two things ship together:

1. **Example projects** — a *larger set of minimal, self-contained*
   Quarto 2 projects under `examples/presentations/`, one per feature.
   Each is a `type: default` project (a `_quarto.yml` + a single
   `format: revealjs` `.qmd` + a `README.md` + `.gitignore`).
2. **A docs page** — `docs/presentations/revealjs/index.qmd` —
   describing each feature, with each section ending in an **embed
   placeholder** that references the matching example project.

The **live-iframe embed mechanism is explicitly out of scope** here.
This plan only establishes (a) the example projects and (b) a
*placeholder* convention the docs use now and a future Lua filter /
`format: html` feature rewrites into real iframes. That mechanism is
tracked separately.

### Why a placeholder, and which one

Quarto 1 embeds live decks with `<iframe class="slide-deck" src="demo/">`
and links code examples with a `code-preview="examples/foo.qmd"`
attribute on fenced code blocks. We want the same *reading experience*
(prose → example you can see and open) but the embedding machinery
doesn't exist in Q2 yet.

**Convention (decided 2026-06-09, with user):** a generic fenced div
carrying the class **`.q2-website-example-iframe`** and the example
path. Generic — *not* revealjs-specific — because the same embed idea
will serve every Q2 example category (websites, presentations, books,
…), and the eventual rendered artifact is an
`<iframe class="q2-website-example-iframe" …>`.

```markdown
::: {.q2-website-example-iframe example="presentations/03-fragments"}
[View example source](https://github.com/quarto-dev/q2/tree/main/examples/presentations/03-fragments)
:::
```

- `example="…"` is the machine-readable path (relative to `examples/`).
- The body is human fallback (a source link) shown until the embed
  mechanism lands — so the page is useful *today*.
- A future Lua `Div` filter (or `format: html` feature) targets
  `.q2-website-example-iframe`, reads `example=`, and replaces the div
  with a live `<iframe class="q2-website-example-iframe" src="…">`.
- Greppable: `grep -rn 'q2-website-example-iframe' docs/` finds every
  embed site when the mechanism is wired.

**As rendered (verified):** the HTML writer emits
`<div class="q2-website-example-iframe" data-example="presentations/NN-...">`
with the fallback `<a>` inside — i.e. a Lua filter reading the **AST** sees the
`example` Div attribute; JS/CSS reading the **DOM** sees `data-example` (a valid
HTML5 data attribute). Both target the same payload.

> Open point to confirm with the user during review: exact attribute
> name (`example=` vs `path=`) and whether the fallback link should
> point at GitHub or a future rendered-example URL.

## Example projects

Location: `examples/presentations/`. New category alongside
`examples/websites/`. Each project:

```
examples/presentations/NN-feature/
├── _quarto.yml      # project: { type: default }
├── slides.qmd       # format: revealjs, minimal content for ONE feature
├── README.md        # what it demonstrates / how to run / what to look for
└── .gitignore       # _site/ .quarto/ *_files/   (mirror websites/)
```

A category-level `examples/presentations/README.md` carries the
linking table (mirror `examples/websites/README.md`).

### The set (one feature each)

- [ ] `01-creating-slides` — `title`/`author` → auto title slide;
  two `##` slides with bullet content. (Baseline deck.)
- [ ] `02-sections` — level-1 `#` section dividers building vertical
  stacks; also a horizontal-rule (`---`) untitled-slide break.
- [ ] `03-fragments` — `::: {.fragment}`, a couple of variant classes
  (`.fade-up`, `.highlight-red`), and `fragment-index`.
- [ ] `04-incremental-lists` — global `incremental: true`,
  `::: {.incremental}`, `::: {.nonincremental}`.
- [ ] `05-columns` — `:::: {.columns}` with `::: {.column width="…"}`.
- [ ] `06-speaker-notes` — `::: {.notes}` (note the S-key speaker view
  is a later phase; doc says so).
- [ ] `07-asides` — `::: {.aside}`.
- [ ] `08-footnotes` — per-slide coalescing: a slide with a footnote
  (`^[…]`) and an `::: {.aside}` together; prose covers the
  `reference-location: document` opt-out.

`slide-level` customization is documented in prose on the page
(under creating-slides), not as its own example.

> Note: named `[^id]` footnotes are NOT yet resolvable (bd-po3gn41h),
> so `08-footnotes` uses the inline `^[…]` form, which works.

## Docs page: `docs/presentations/revealjs/index.qmd`

Structure (Gopen: each section opens with the feature in topic
position, old→new flow, the example placeholder as the section's
stress payoff):

1. **Overview** — what `format: revealjs` is; that a deck is markdown
   with headings as slides; pointer to the format reference (if one
   exists) and to the examples.
2. **Creating slides** — `##` slides, `#` sections, `---` breaks, the
   automatic title slide, `slide-level`. Placeholder → `01`, `02`.
3. **Fragments** (incremental reveal of elements). Placeholder → `03`.
4. **Incremental lists**. Placeholder → `04`.
5. **Multiple columns**. Placeholder → `05`.
6. **Speaker notes**. Placeholder → `06`.
7. **Asides**. Placeholder → `07`.
8. **Footnotes** — per-slide coalescing + `reference-location: document`.
   Placeholder → `08`.

Each feature section: 1–2 sentences of *what it is / when to use it*,
a fenced ```markdown code block of the minimal source, then the
`.q2-website-example-iframe` placeholder. Document **only implemented
features** — no backgrounds, code-line highlighting, transitions,
footer/logo, theme authoring (later phases). Where a feature has an
obvious next-phase neighbor (e.g. speaker view), say "not yet" rather
than implying it works.

Frontmatter: `--- title: "Reveal.js" ---` (match local docs style).

## Navigation (`docs/_quarto.yml`)

- [ ] Add a **Presentations** navbar entry (left) →
  `docs/presentations/revealjs/index.qmd`.
- [ ] Add a sidebar section (`id: presentations`) listing the reveal.js
  page, parallel to the existing `guide` / `errors` sidebars.

## Verification (end-to-end, per CLAUDE.md)

- [ ] Each example renders: `cargo run --bin q2 -- render
  examples/presentations/<dir>` produces a reveal deck; spot-check the
  feature markup is present (e.g. `class="fragment"`, `aside-footnotes`).
- [ ] The docs site renders: `cargo run --bin q2 -- render docs/`
  (NEVER `quarto` — Q2 only) succeeds and emits
  `docs/_site/presentations/revealjs/index.html`.
- [ ] Inspect the rendered page: prose + code blocks present, each
  placeholder div renders (it's a plain classed `<div>` until the embed
  mechanism lands — confirm it doesn't error and the fallback link
  works).
- [ ] No YAML-schema errors from the docs build (Q2 validates
  frontmatter; the placeholder div uses only standard attrs).

## Out of scope (tracked elsewhere / future)

- The live-iframe embed mechanism (Lua filter on `docs/` or a q2
  `format: html` website feature) — the whole point of the generic
  `.q2-website-example-iframe` placeholder is to defer this cleanly.
- Named `[^id]` footnote resolution — bd-po3gn41h.
- Speaker-view / notes plugin (S key) — bd-0qaarvzx.
- Later-phase reveal features (backgrounds, code-line highlighting,
  transitions, footer/logo, theme authoring).

## Phase 0 — per-document `format: revealjs` in projects (prereq)

**Discovered 2026-06-09:** a `type: default` project with a single
`format: revealjs` doc renders as **plain Bootstrap HTML, not a deck** —
the project pipeline threads one project-wide format and never reads a
per-file front-matter `format:` override. This is the minimal slice of
**bd-l6itt34u** (Phase 8). The user chose to fix it now (a bare-`.qmd`
example without `_quarto.yml` is a footgun: a stray parent `_quarto.yml`
would absorb it). **Scope here = make a single-deck `type: default`
project render as a real reveal deck.** Website-deck *integration*
(nav/sidebar for decks, listing talks, cross-doc links, project-level
revealjs defaults) stays in bd-l6itt34u.

**Root cause (investigation):** both the single-file and project CLI
paths run through `ProjectPipeline`. The single-file path pre-detects the
doc's front-matter format (`detect_single_input_format`) and passes it as
the project format — which is why a bare deck works. A *directory* render
passes `"html"` (dir detection returns `None`) and never consults the
per-file `format:`.

**Design — prefer-merge model (decided 2026-06-09, with user; supersedes an
initial boolean draft).** The effective per-document format **key** is a
prefer-merge of the `format:` declarations, lowest → highest:

```text
  project config (_quarto.yml)  →  document front matter  →  `--to` (synthesized `format: !prefer <to>`)
```

This reuses the **existing** `ConfigValue` `MergeOp::Prefer` machinery (it is
*not* new infrastructure). Properties:
- Uniform: `--to` behaves exactly like a document that wrote
  `format: !prefer <to>` itself — it wins over every in-document declaration.
- Honors a **project-level** `format: revealjs` in `_quarto.yml` (a
  document-front-matter-only peek would miss this — the early boolean draft
  did).
- Resolved **pre-pipeline** (in `render_document_to_file`, after the project
  is known) because the key selects both the transform pipeline (reveal vs.
  generic) and the output extension, both chosen before any stage runs.
  `MetadataMergeStage` even force-overwrites `meta.format` to `ctx.format`,
  confirming `ctx.format` is the single source of truth — so the key merge
  belongs here, while full format-specific *config* flattening stays in
  `MetadataMergeStage`.

**Implementation (landed):**
- `quarto-core::format`: `format_key_from_frontmatter` / `extract_yaml_frontmatter`
  (shared; the CLI's `detect_single_input_format` delegates to it),
  `format_key_from_config_value`, and `resolve_format_key(project, document,
  cli_to, default)` building synthesized `{format: !prefer <key>}` layers and
  `MergedConfig::materialize()`.
- `render_document_to_file` gains `format_override: Option<&str>` (the `--to`
  value); resolves project-config + document-front-matter + `--to`. The passed
  `format` is the fallback default. All non-project callers pass `None`.
- `RenderToFileRenderer.format_override: Option<String>` threaded to its 3
  `render_document_to_file` sites; builder
  `ProjectPipeline::with_format_override(Option<String>)` on the concrete
  `RenderToFileRenderer` impl (no `Pass2Renderer` trait / other-renderer
  changes). CLI sets `with_format_override(args.to.clone())`.

**Tests (TDD) — all landed & green:**
- [x] `default_project_honors_per_file_revealjs_format` — per-file `format:
  revealjs`, override `None` → `class="reveal"` + `Reveal.initialize`.
- [x] `project_level_revealjs_format_renders_deck` — project-level
  `format: revealjs` in `_quarto.yml`, doc declares none, override `None` →
  deck. (The boolean draft would have failed this.)
- [x] `explicit_to_overrides_per_file_format` — override `Some("html")`
  (= `--to html`) → plain HTML.
- [x] full workspace green (9593), no regressions in the shared project path.

**Verify (done):** `q2 render examples/presentations/01-creating-slides` (as a
directory) emits a 750 KB reveal deck (`class="reveal"` ×3); `… slides.qmd
--to html` forces plain HTML (0 reveal). ✅

## Checklist

### Phase 0 — per-document format (bd-l6itt34u slice) ✅
- [x] Shared front-matter + format-key helpers in `quarto-core::format`
  (incl. `resolve_format_key` prefer-merge); CLI delegates (DRY)
- [x] `render_document_to_file(format_override: Option<&str>)` + prefer-merge
- [x] `RenderToFileRenderer.format_override` + 3 call sites threaded
- [x] `ProjectPipeline::with_format_override` (concrete impl)
- [x] CLI sets `with_format_override(args.to.clone())` (single-doc + project)
- [x] TDD tests (per-file + project-level + `--to` regression) + workspace green

### Phase A — example projects ✅
- [x] `examples/presentations/.gitignore` (+ ignore in-place `slides.html`)
- [x] `examples/presentations/README.md` (linking table)
- [x] 01-creating-slides … 08-footnotes (project + qmd + README each)
- [x] Rendered every example; confirmed feature markup (fragments,
  `data-fragment-index`, `<li class="fragment">` incl. `.nonincremental`
  opt-out, `flex-basis`, `aside.notes`, `aside.aside`, per-slide
  `aside-footnotes` + `<sup>1</sup>`)

### Phase B — docs page ✅
- [x] `docs/presentations/revealjs/index.qmd` (reader-expectations prose;
  Gopen & Swan 1990)
- [x] One section per implemented feature, each with a
  `.q2-website-example-iframe` placeholder + not-yet callouts
- [x] Rendered `docs/` (150/150; only pre-existing warnings); page emits 8
  placeholders w/ `data-example`, all sections, working fallback links

### Phase C — navigation ✅
- [x] Navbar "Presentations" + `id: presentations` sidebar in `docs/_quarto.yml`
- [x] Nav link verified from home page (`presentations/revealjs/index.html`)

### Phase D — wrap ✅
- [x] `examples/README.md` registers the `presentations/` category
- [x] Commit (Phase 0 already committed separately); report; push on request
