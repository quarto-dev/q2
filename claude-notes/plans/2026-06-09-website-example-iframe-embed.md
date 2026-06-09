# Example-iframe embed feature (`.embed-example-iframe`)

**Strand:** bd-z1smhvuo (discovered-from bd-ixdktocp, the revealjs docs page)
**Date:** 2026-06-09
**Status:** DESIGN — core decisions settled with user 2026-06-09; ready to turn
into an execution plan on go-ahead. No code yet.
**Parent context:** Quarto 2 website infrastructure
(`claude-notes/plans/2026-04-23-website-project-epic.md`); the placeholder
convention this consumes was set in
`claude-notes/plans/2026-06-09-revealjs-docs-and-examples.md`.

## Goal

Quarto 1's docs embed live demos as `<iframe class="slide-deck" src="demo/">`
beside a short, illustrative code snippet (see
https://quarto.org/docs/presentations/revealjs/#incremental-lists — the snippet
shows *only* the relevant metadata; the iframe shows a *full* deck). Quarto 2
wants the same reading experience for **most** doc pages, plus a third element
Q1 lacks: **a link to a minimal, self-contained example project on GitHub**.

So each documentation example reads as a triple:

1. **An iframe** embedding the rendered example (HTML page *or* reveal deck),
   sized to sit comfortably in the page.
2. **A short code snippet** illustrating just the feature (hand-authored in the
   doc — *not* the whole example; already present in the revealjs page).
3. **A link to the GitHub repo** holding the full minimal Quarto 2 project.

This strand builds the **embed mechanism** — the thing that turns the authored
placeholder into the live iframe + link. It must work:

- in **both** `q2 render` and `q2 preview`,
- inside **both** HTML pages and **revealjs** presentations,
- as a **general Quarto 2 feature**, named independently of the website project
  and of the "example" docs use case — the iframe target can be *any* static
  asset, not only an example project.

The code snippet is out of scope (hand-authored). What this feature owns is
**(iframe + link)** and resolving the iframe `src` like a normal Quarto link.

## Decisions settled with the user (2026-06-09)

1. **Mechanism = built-in Rust transform** on `Div.embed-example-iframe`
   (Axis 1, option 1a below). Runs in the shared transform pipeline, so render +
   preview + HTML + revealjs all get it for free.
2. **Class = `.embed-example-iframe`** (renamed from the interim
   `.q2-website-example-iframe`). Deliberately specific/verbose: because a
   built-in filter silently activates on this class, the name must be one a user
   would never type by accident.
3. **Attribute = `file`** — a project-relative path to a **static asset**,
   resolved through the **normal Quarto link-rewriting machinery** (exactly like
   any other `href`/`src` in a project).
4. **Static-asset-only constraint.** The `file` target MUST be a pre-existing
   static asset (e.g. rendered `…/slides.html`). Pointing `file=` at a `.qmd`
   that would need dynamic rendering is **disallowed**. This single rule:
   - makes the iframe behave like a regular project link (copied to `_site/` on
     render, served from the VFS on preview),
   - makes **preview parity in scope** for this static case (no dynamic
     sub-render needed in the preview server),
   - dodges infinite-recursion footguns (an iframe pointing at the page that
     contains it, or at a doc that re-embeds itself).
5. **Axis 2 = stage example output as in-project static assets.** A future
   pre-render *script* (Quarto-external for now — "remember to run it") renders
   every example project into a separate location and copies the static output
   into an in-project `examples/` directory. The feature itself is oblivious to
   "examples": it only ever sees a project-relative path to a static file. The
   guiding principle: from a *user's* perspective this must feel like a **regular
   Quarto feature** — `render`/`preview` alone resolve the iframe; no extra
   manual step is required at render/preview time. (The pre-render script is an
   authoring-time content-preparation step, not a render-time dependency.)

## What already exists (do not rebuild)

- **Placeholder convention** (landed, bd-ixdktocp) — *to be renamed* by this
  work. Docs currently author:
  ```markdown
  ::: {.q2-website-example-iframe example="presentations/03-fragments"}
  [View example source](https://github.com/quarto-dev/q2/tree/main/examples/presentations/03-fragments)
  :::
  ```
  Renders today to a plain
  `<div class="q2-website-example-iframe" data-example="…">` with the fallback
  `<a>` inside. 8 live placeholders in `docs/presentations/revealjs/index.qmd`.
  **This strand migrates them** to the settled spelling:
  ```markdown
  ::: {.embed-example-iframe file="examples/presentations/03-fragments/slides.html"}
  [View source](https://github.com/quarto-dev/q2/tree/main/examples/presentations/03-fragments)
  :::
  ```
- **Example projects** (landed). `examples/presentations/NN-feature/` — each a
  `type: default` project (`_quarto.yml` + `slides.qmd` `format: revealjs` +
  `README.md`), rendering `slides.qmd` → `slides.html` **in place** (in-place
  `slides.html` is currently `.gitignore`d). Sibling `examples/websites/`.
  `examples/` sits at the **repo root**, outside the `docs/` tree.
- **Shortcode machinery.** `transforms/shortcode_resolve.rs` — not the chosen
  surface, but confirms the transform pipeline runs natively + on WASM.
- **AST transform machinery.** Div-targeting Rust transforms are the dominant
  pattern (`callout_resolve`, `theorem`, `table_bootstrap_class`, …); they run
  in the shared pipeline → render *and* preview, HTML *and* revealjs.
- **Link-rewriting machinery.** `transforms/link_rewrite.rs` — the existing path
  that resolves project-relative links and drives resource copying into `_site/`.
  The `file=` attribute should ride this same path so the iframe target is
  treated as a project resource.
- **Website project pipeline.** Two-pass orchestrator
  (`project/orchestrator.rs`) with a resource-copy step
  (`project_resources::copy_resources_to_output_dir`).
- **Preview server.** `q2 preview` (`crates/quarto-preview`) + the q2-preview
  format in hub-client serve docs from the VFS; static assets in the VFS are
  directly serveable (this is what makes Decision 4 tractable).

## The two design axes

### Axis 1 — What rewrites placeholder → iframe? (SETTLED: 1a)

| Option | Sketch | Verdict |
| --- | --- | --- |
| **(1a) Built-in Rust transform on `Div.embed-example-iframe`** | New transform in `quarto-core/src/transforms/`, mirrors `callout_resolve`. Reads `file=`, link-resolves it, emits an `<iframe>` + source link. | **Chosen.** Shared pipeline → render + preview + HTML + revealjs. Placeholders are already Divs → minimal authoring churn (just the rename). No Lua runtime. |
| (1b) Built-in shortcode `{{< … >}}` | Register a handler beside `meta`. | Rejected — the Div is the right surface for a *block* with a fallback child link; would also collide conceptually with the existing `{{< embed >}}` notebook shortcode. |
| (1c) Lua filter scoped to `docs/` | `docs/_filters/…lua`. | Rejected — couples to `docs/`; feature must be general + built-in. |
| (1d) `format: html` website feature | Bake into the website format. | Rejected — wrong altitude; too broad. |

**Transform behavior (`ExampleEmbedTransform`, working name):**
- match `Div` with class `embed-example-iframe`;
- read `file=` → run it through the **same link resolution** as project links
  (so render copies it to `_site/` and preview serves it from the VFS);
- **validate** the target is static (reject `.qmd`/dynamic targets with a clear
  diagnostic — Decision 4);
- replace the Div with: the `<iframe class="embed-example-iframe" src="…">`,
  plus the source link (reuse the Div's existing child `<a>` as the link, else
  synthesize one from `file=`);
- sizing: per-placeholder `height=`/`aspect=` opts with sensible defaults
  (a deck target → 16:9 box). Exact sizing config is a v1 detail to refine.

Open Axis-1 sub-questions (non-blocking, decide during execution):
- **Gating / degradation.** Outside a project (single-file `q2 render foo.qmd`)
  there is no link-resolution base. The transform should degrade to "keep the
  fallback link" rather than emit a broken iframe. Lean: run unconditionally,
  emit the iframe only when a project link base resolves; else keep the link.
- **GitHub link.** Keep `tree/main/examples/…` convention (v1) vs a config block.

### Axis 2 — Where does the example output live, and how does the iframe reach it? (SETTLED: in-project static assets)

Settled per Decision 5: example output is materialized as **static assets inside
the project** (an in-project `examples/` dir) by an external pre-render script;
the placeholder's `file=` points at that static asset; the normal link machinery
does the rest. Concretely, the static-asset constraint (Decision 4) makes all
three contexts collapse to "resolve a project-relative static link":

| Context | How the iframe `src` resolves |
| --- | --- |
| `q2 render docs/` | `file=` target is copied to `_site/` as a project resource (link-rewrite path); iframe loads the static copy. Self-contained site + static deploy "just work". |
| `q2 preview docs/` (+ hub-client q2-preview) | the static asset is in the VFS; the preview server serves it directly. No dynamic sub-render; no recursion. |
| Deployed quarto.org | static `_site/` already contains the asset; nothing special. |

Mechanics to design in execution:
1. **Link resolution for `file=`.** Make the transform feed `file=` into
   `link_rewrite` (or the equivalent resource-collection path) so the target is
   registered as a project resource and copied/served like any link. This is the
   crux of the implementation and the main thing to get right.
2. **Static-target validation.** Reject `.qmd` (and any dynamic target) with a
   helpful diagnostic pointing the author at the rendered-asset path instead.
3. **The pre-render script (separate deliverable, maybe its own strand).** A
   script that renders `examples/**` projects and stages their static output
   into the in-project `examples/` dir the docs reference. Out of the
   *render-time* path by design; tracked so we "remember to run it". Whether it
   becomes a real `pre-render` project hook later is a future question — for now
   it is explicitly a manual content-prep step.

## Recommended phasing

### Phase 1 — Rewrite transform (Axis 1a) ✅ (2026-06-09)
- [x] TDD: transform tests that `Div.embed-example-iframe[file=…]` becomes an
  `<iframe class=embed-example-iframe src=…>` + source link; degrades to the
  fallback link with a diagnostic on missing `file=`; rejects a `.qmd` target
  with a diagnostic and degrades (no iframe). 9 unit tests, all green.
- [x] `ExampleEmbedTransform` in `quarto-core/src/transforms/example_embed.rs`,
  registered in `build_transform_pipeline` (common normalization phase, after
  shortcode/metadata-normalize). q2-preview reuses that builder, so preview
  gets it too.
- [x] Sizing defaults (deck → `aspect-ratio: 16/9`; `height=` override) inline
  on the iframe; GitHub link is the author-written fallback body.
- [x] Registered structured error codes in `quarto-error-reporting`
  (`project` subsystem): **Q-5-4** (missing `file=`) and **Q-5-5** (non-static
  target). Added catalog entries, `docs/errors/project/Q-5-{4,5}.qmd` pages +
  sidebar entries, and a catalog-registration unit test. Diagnostics now render
  as `[Q-5-5] Example Embed Target Is Not a Static Asset` with a tidyverse
  problem/hint.
- [x] Verified end-to-end through `q2 render` (HTML **and** `--to revealjs`):
  valid placeholder → `<iframe class="embed-example-iframe" src=…>`; `.qmd`
  target → rendered `[Q-5-5]` diagnostic + degraded link, single iframe in
  output. Workspace build green; `quarto-core` + `quarto-error-reporting` suites
  green (2314 tests).

**Note:** migrating the 8 live placeholders in
`docs/presentations/revealjs/index.qmd` moved to **Phase 2** — the `file=` value
must point at a *staged static asset*, which doesn't exist until staging lands.
Migrating now would ship broken iframe `src`s (the fallback link still works,
but the page would render a 404 frame). Coupling the migration to staging keeps
the docs build honest.

### Phase 2 — Stage example output + resource-copy + migrate docs (Axis 2)

**Investigation results (2026-06-09):**
- An example's output is a **directory** (`slides.html` + `slides_files/…`), not
  a single self-contained file — so copying must be **directory-level**.
- Q2 copies static (non-input) files into `_site/` **only** via an explicit
  `project.resources:` declaration (`project_resources.rs`); there is no blanket
  auto-copy. → Phase 2 needs a `resources:` entry.
- Repo convention is **commit source, never commit rendered output**: `docs/_site/`
  is untracked and example projects commit only `slides.qmd`/`_quarto.yml`/`README.md`
  (the rendered `slides.html`/`slides_files/` are `.gitignore`d). → Staged assets
  are **regenerated, not committed** (matches the user's "a script we'll have to
  remember to run").
- Project resources must live **inside the project root** (that's error `Q-5-1`),
  and `examples/` sits at the *repo* root, outside `docs/`. → Staged output must
  land **under `docs/`** (e.g. `docs/examples/<cat>/<name>/`); the root `examples/`
  *sources* stay where they are.

**Settled design:**
- Staging location: `docs/examples/presentations/<name>/` (gitignored).
- `file=` spelling: project-absolute `/examples/presentations/<name>/slides.html`.
- Copy surface: `project.resources: [examples]` in `docs/_quarto.yml`.

**OPEN (user's call): the staging-script form** — a `cargo xtask` subcommand
(cross-platform Rust, discoverable, fits repo tooling; repo has a hard
cross-platform rule) vs. a plain shell script in `scripts/` (simplest, matches
the user's "just a regular script" phrasing, but not Windows-friendly). Leaning
xtask.

**Resolved (user, 2026-06-09):** staging form = `cargo xtask`; gated by an
explicit `examples/manifest.yml` allow-list (no globs) so a stray project is
never rendered/published by accident.

**Relativization (user, 2026-06-09):** the emitted iframe `src` must NOT be the
naked project-absolute `/examples/...` — it must be rewritten to a
**page-relative** URL based on the page's depth, exactly like other Quarto
links, so the site is portable under any deploy subpath. Implemented via a new
`resolve_static_resource_href` (sibling of `resolve_doc_relative_href`, but no
index lookup / no `.qmd` diagnostic — just normalize + `page_url_for`). The
transform threads the page source + resolver and routes `file=` through it.

Tasks:
- [x] `examples/manifest.yml` allow-list (8 presentation projects).
- [x] `cargo xtask stage-doc-examples`: render each manifest project with `q2`
  and copy its `*.html` + `*_files/` output into `docs/examples/<entry>/`.
- [x] `docs/.gitignore`: ignore the generated `/examples/` staging tree.
- [x] `docs/_quarto.yml`: `project.resources: [examples]`.
- [x] Migrate the 8 placeholders in `docs/presentations/revealjs/index.qmd`
  to `.embed-example-iframe`/`file="/examples/presentations/<name>/slides.html"`.
- [x] `resolve_static_resource_href` helper + transform threads source/resolver
  so the iframe `src` is page-relative (`../../examples/...`). Unit tests on
  both the helper and the transform (depth-2 page → `../../`).
- [x] End-to-end (render): staged, `q2 render docs/`, served `_site/`, and
  **browser-verified** all 8 iframes load real reveal decks; iframe `src` is
  `../../examples/...` (depth-2 relative, no host-absolute `/examples`); staged
  decks (+ `slides_files/`) land under `_site/examples/`; source links work.
  2327 workspace tests green.
- [ ] **Preview (Decision 4) — moved to its own strand: bd-kjrpya2d.**
  - A CLI-only disk approach was prototyped (a `vfs_root`-mode resolver branch
    emitting `/examples/…` + a `project_or_spa_handler` disk route in
    `quarto-preview`) and **browser-verified working for `q2 preview docs/`** —
    but then **reverted**: it's disk-bound and can never work in a real
    hub-client project where `/examples/` lives only in Automerge (no disk, no
    native server). See the strand for why.
  - Chosen instead: the **VFS-native** fix — teach the TS iframe post-processor
    to fall back to the VFS **source** path when the artifact path misses (and
    ensure the deck is in the VFS source). Avoids per-render Automerge
    duplication. Tracked in
    `claude-notes/plans/2026-06-09-preview-embed-vfs-resolution.md`.
  - Crossref ("Demo 1" caption + `@demo` xref) already works in preview
    (verified in-browser).

### Phase 3 — Pre-render staging script (separate deliverable)
- [ ] Script that renders `examples/**` and stages static output into the
  in-project staging dir; document the "remember to run it" step. Likely its
  own strand. Not on the render/preview critical path.

## Resolved vs. remaining questions

**Resolved with user:** mechanism (Rust transform), class
(`.embed-example-iframe`), attribute (`file`), static-only constraint, preview
parity in scope for static assets, example output staged as in-project static
assets via an external pre-render script.

**Remaining (decide during execution, non-blocking):**
- Sizing config surface (`height=`/`aspect=` vs category defaults).
- GitHub-link source (convention vs `website:`-style config block).
- Whether the pre-render staging eventually becomes a real project hook.
- Exact diagnostic wording / error code for a rejected dynamic (`.qmd`) target.

## Out of scope

- The hand-authored illustrative code snippet (already in the docs page).
- Authoring new example projects (presentations done; other categories later).
- A general "embed any external/remote site" feature — scoped to project-relative
  **static** assets.
- Dynamic iframe targets (`.qmd` rendered on the fly) — deliberately disallowed.

## Checklist (design phase)

- [x] Survey existing placeholder, examples, transform, project, link-rewrite,
  and preview infrastructure.
- [x] Enumerate Axis 1 (rewrite) and Axis 2 (output location) options.
- [x] Settle core decisions with the user (mechanism, class, attribute,
  static-only constraint, preview-parity scope, staging model).
- [x] Write + revise this plan.
- [ ] Convert into a TDD-first execution plan and repoint `CURRENT.md` on
  the user's go-ahead.
