# Website Projects (Epic)

**Date:** 2026-04-23
**Beads:** `bd-0tr6` (epic); phases 0–9 as sub-issues; docs spun out as `bd-tr81`.
**Status:** Design approved; phase sub-plans to be written in separate sessions.

## Overview

Design and implement **multi-page website projects** for Quarto 2. A website
project is a directory with a `_quarto.yml` declaring `project.type: website`,
containing multiple `.qmd` files that render to a coherent website with
shared navigation, cross-document references, and deduplicated resources.

This epic sits at the intersection of four features that interlock:

1. **A "static document snapshot" contract** — a typed, serializable summary of
   everything knowable about a document without running its engines or user
   filters. Sidebars, navbars, cross-references, incremental rebuild cache,
   and (eventually) `freeze` all depend on this.
2. **Sidebars** — new navigational affordance, following the `generate/render`
   pattern established by TOC, navbar, footer.
3. **Project-scoped shared resources** — `site_libs/` style dedup of CSS, JS,
   themes, fonts across pages. A reshape of `ArtifactStore` so stores know how
   to relocate themselves based on project scope.
4. **Hub-client project rendering** — hub-client caches a project's navigation
   state and re-renders only the active page on edit. This foreshadows the
   bigger architectural move: **`quarto preview` in Quarto 2 will be a local
   hub-client instance** (ephemeral sync server + file watcher), not a
   standalone preview server.

## Reference material

This plan was written after reviewing:

- **Q2 pipeline design:** `claude-notes/plans/2026-01-06-pipeline-stage-design.md`,
  `2025-12-27-unified-render-pipeline.md`, `2025-12-29-config-integration-pipeline.md`,
  `2026-03-09-metadata-merge-stage.md`, `2026-03-16-user-filters-pipeline.md`,
  `2026-04-01-lua-api-pipeline-wiring.md`, `2026-04-13-pipeline-tracing.md`.
- **Q2 navigation model:** `claude-notes/plans/2026-04-18-navbar-footer-design.md`,
  `2026-01-28-phase6-toc-rendering.md`, `2026-01-10-hub-client-nav-refactor.md`.
- **Q2 dependencies/resources:** `claude-notes/plans/2026-04-18-html-js-deps-design.md`,
  `2026-03-09-css-in-pipeline*.md`, `2025-12-20-minimal-website-render-resources.md`.
- **Hub-client:** `claude-notes/plans/2025-12-22-quarto-hub-web-frontend-and-wasm.md`,
  `2026-01-28-unify-hub-client-pipeline.md`.
- **Q1 website analysis (stale but useful):** `claude-notes/website-project-rendering.md`,
  `claude-notes/book-project-rendering.md`.
- **Source code** in `crates/quarto-core/src/pipeline.rs`,
  `crates/quarto-core/src/stage/`, `crates/quarto-navigation/src/`,
  `crates/quarto-core/src/transforms/`, `crates/wasm-quarto-hub-client/src/`,
  and Q1 reference in `external-sources/quarto-cli/src/project/types/website/`.

## Key decisions (from design conversation)

These are settled as of this draft; sub-plans will execute on them.

1. **Static snapshot is a typed, serializable value.** New type (working name
   `DocumentProfile` — see naming TBD below) produced at a fixed pipeline
   checkpoint. Downstream code that needs cross-document information reads
   `Vec<DocumentProfile>`. User filters at later stages may read profiles but
   **cannot mutate them**.
2. **Pipeline stages up to the snapshot are resumable.** Pass 1 advances each
   file to the snapshot checkpoint and stores the intermediate pipeline state.
   Pass 2 **resumes from the checkpoint** rather than re-running. Avoids
   redundant execution. Also the substrate for future `freeze`.
3. **Project orchestration uses a `ProjectType` trait** with `pre_render` /
   per-file contributions / `post_render` hooks. Websites, books,
   manuscripts, and default all implement it. Single-document renders
   continue to work as a degenerate "default" project.
4. **MVP scope explicitly excludes:** search, listings+RSS, aliases,
   announcements, analytics, 404 page, reader-mode, repo-actions,
   breadcrumbs. These are follow-up epics.
5. **`quarto preview` is out of scope for this epic** but the design must not
   paint us into a corner — it will be rebuilt as a local hub-client instance
   in a separate project.
6. **Artifact stores are scope-aware and relocatable.** A single-document
   project resolves relative to the document; a multi-document project
   resolves relative to the project's output dir (`_site/site_libs/`).
7. **One new user-filter position:** after the snapshot checkpoint, where
   filters can read `Vec<DocumentProfile>` but cannot mutate the snapshot.
   Pre-snapshot mutation is reserved for future Quarto-1-style pre-render
   scripts.

## Naming decisions

- **Static-document snapshot type**: **`DocumentProfile`**. Final. Used
  throughout the rest of this plan.
- **Trait naming**: `ProjectType` (matches Q1 terminology users already know).
- **Snapshot stage name**: finalize in phase 0 when the code shape is
  concrete. Candidates: `ProfileStage`, `DocumentProfileStage`,
  `ProjectSnapshotStage`.

## Scope

### In scope

**Core machinery:**
- `DocumentProfile` type + stage + checkpoint in the pipeline.
- Pipeline resumability at the checkpoint boundary.
- `ProjectType` trait and `WebsiteProjectType` implementation.
- Project orchestration: two-pass render across all files in a project.
- Cross-document index available to per-file rendering.

**Website-specific features (MVP):**
- Project-level **navbar** (extends existing single-doc navbar).
- Project-level **page-footer** (extends existing single-doc footer).
- **Sidebar** (new): manual + auto + nested sections, multiple sidebars
  keyed by `id` or path-prefix, sidebar-for-page resolution.
- **Page navigation** (prev/next from sidebar position).
- **Cross-document link rewriting** (`[link](other.qmd)` → `other.html`).
- **Project output directory** (`_site/` by default).
- **Shared `site_libs/`** for deduplicated CSS/JS/theme resources.
- **`sitemap.xml`** (when `site-url` is set).
- **Favicon** (`website.favicon`).
- **Site title / title prefix** (`website.title`).

**Incremental rebuilds (v1):**
- Cache serialized `DocumentProfile`s keyed by source hash/mtime.
- Reuse cached profiles for unchanged files in the project sweep.
- Update sitemap in place.

**Hub-client integration:**
- Project-level nav state cached in hub-client.
- "Render single page in project context" entry point in the WASM API.
- Invalidation protocol when a sibling's profile changes.

### Out of scope (explicit defers to separate epics)

- **Breadcrumbs.** Non-trivial because the same document can be reachable from
  multiple sidebar paths; design separately.
- **Search / `search.json` / client search UI.** Significant client-side
  infrastructure with its own data model.
- **Listings / RSS feeds.** Its own design with schemas, sorts, filters,
  per-category pages.
- **Aliases / redirects.**
- **Announcements.**
- **Google Analytics / other analytics.**
- **Custom 404.**
- **Reader mode.**
- **Repo actions** (`edit`, `source`, `issue` links).
- **`quarto preview` in Q2** (separate epic; this plan must not block it).
- **Book project type.** Will reuse the project machinery but has its own
  structure (chapters, parts, appendices, cross-ref numbering).
- **`freeze` (cached engine outputs).** Shares the serializable-checkpoint
  substrate but is its own epic.

## Architecture sketch

### Pipeline with snapshot checkpoint

Current pipeline (simplified): `Parse → Merge → Sugar → Engine → ThemeCSS →
UserFilters(pre) → AstTransforms → UserFilters(post) → CodeHighlight →
RenderBody → ApplyTemplate`.

Proposed shape:

```
┌──────────────────────────────┐
│ Parse                        │
│ Merge                        │
│ ── [Profile checkpoint] ──── │  ← DocumentProfile extracted here
│ Sugar                        │
│ Engine                       │
│ ThemeCSS                     │
│ UserFilters(pre)             │  ← first filter position that reads
│ AstTransforms                │    the project-wide Vec<DocumentProfile>
│ UserFilters(post)            │
│ CodeHighlight                │
│ RenderBody                   │
│ ApplyTemplate                │
└──────────────────────────────┘
```

The checkpoint sits **after `Merge`** (metadata fully resolved) and **before
`Sugar`** (no AST mutation yet). This means the profile sees parsed-but-raw
content and fully merged metadata, but not engine output — which is exactly
the static contract we want.

`DocumentProfile` contents (first cut; sub-plan will finalize):

- `source_path: PathBuf` (relative to project root)
- `output_href: String` (the URL other pages should use to link to this doc)
- `title: Option<ConfigValue>` (markdown-rich, following Q2 meta
  interpretation)
- `subtitle`, `description`, `author(s)`, `date`, `categories`, `keywords`,
  `image` (each `Option<…>`)
- `draft: bool`
- `outline: Vec<HeadingEntry>` (id/text/level hierarchy used by sidebar auto,
  page-navigation, and in-page TOC)
- `format_id` (which format-variant this page outputs as)
- `profile_version` (bumped if we ever change the serialized shape)
- Room to grow: the type is `serde`-serializable from day one.

### Pipeline resumability

`PipelineData` today is an enum threading between stages. Two changes:

1. **Checkpoint variant** (e.g. `PipelineData::AtProfile { ... }`) is a
   dedicated halting point.
2. **Clone at checkpoint.** Pass 1 produces `PipelineData::AtProfile` for each
   file and keeps a clone; the extracted `DocumentProfile` is stored in the
   project's cross-doc index. Pass 2 takes the stored `AtProfile`, injects the
   cross-doc index into the per-file runtime context, and resumes into `Sugar`
   onwards.

For the CLI on a cold project: the full pass 1 happens in memory; pass 2
happens in memory immediately after. For incremental rebuilds: pass 1 may be
satisfied from disk cache (see §Incremental).

This pattern also lets hub-client serialize checkpoint state into the VFS,
and eventually serves as the mechanism for `freeze`.

### `ProjectType` trait

Rough shape (sub-plan will finalize names, async shape, error type):

```rust
pub trait ProjectType {
    fn kind(&self) -> &'static str; // "website", "book", "default"
    fn output_dir(&self) -> &'static str; // "_site" for website, "." for default
    fn lib_dir(&self) -> &'static str; // "site_libs" for website, "{stem}_files" for default

    // Called after config is loaded, before any files are processed.
    fn pre_render(
        &self,
        ctx: &mut ProjectContext,
    ) -> Result<()>;

    // Transforms/stages to add to the per-file pipeline for this project type.
    fn per_file_transforms(&self) -> Vec<Box<dyn AstTransform>>;

    // Called after all files have rendered.
    fn post_render(
        &self,
        ctx: &ProjectContext,
        outputs: &[RenderedPage],
        incremental: bool,
    ) -> Result<()>;
}
```

The "default" project type (single-doc or directory-without-`_quarto.yml`)
implements no-op hooks. Website adds the per-file navigation transforms plus
sitemap/favicon emission in post-render.

### Cross-document index

Shape:

```rust
pub struct ProjectIndex {
    pub profiles: Vec<DocumentProfile>,
    pub by_source_path: HashMap<PathBuf, usize>,
    pub by_output_href: HashMap<String, usize>,
    // Helpers for sidebar/link rewriting:
    pub fn lookup_by_source(&self, path: &Path) -> Option<&DocumentProfile>;
    pub fn lookup_by_href(&self, href: &str) -> Option<&DocumentProfile>;
}
```

Available to per-file pipeline via the runtime context, shared read-only in
pass 2.

### Artifact store scoping and relocation

Today: `ArtifactStore` holds keyed blobs (CSS, JS, intermediate docs).
Problem: nothing tells it whether it's writing to a per-page directory or a
project-shared directory.

Proposed reshape:

- Every artifact entry has a **scope**: `Page` (per-file) or `Project`
  (shared).
- An artifact writer (part of project orchestration) resolves scopes into
  concrete paths:
  - Default project (single doc): both scopes resolve under
    `{stem}_files/...` alongside the output.
  - Website project: `Page` scope → `{stem}_files/` per-page; `Project` scope
    → `_site/site_libs/{name}/...` shared.
- Theme CSS, navbar/sidebar JS, bootstrap, and similar shared assets are
  tagged `Project`. Per-page figures, cached plots, and engine outputs stay
  `Page`.
- The rendered HTML for a page rewrites artifact URLs through a relocator
  that knows the project's layout, so that `index.html` and `docs/api.html`
  each produce the correct relative or absolute reference to the same shared
  file.

This reshape is the one piece of this epic that touches code outside of
website-specific modules. The sub-plan must carefully sequence the change so
single-document rendering remains a pure refactor (no behavior change).

### Template slot changes

Following navbar/footer/TOC, sidebar and page-navigation get their own slots:

- `$rendered.navigation.sidebar$` (left column, when applicable)
- `$rendered.navigation.page_navigation$` (prev/next block, bottom of page)

Existing slots (`$rendered.navigation.navbar$`, `$rendered.navigation.toc$`,
`$rendered.navigation.footer$`) already exist. The template remains a
substitution target; project context flows through `ast.meta.navigation.*`
and the pre-rendered HTML strings.

### Hub-client integration shape

Two new WASM API surfaces (or extensions of existing ones):

1. **`build_project_nav(project_dir)`** — runs pass 1 for all files in the
   project, returns a serializable `ProjectNavState` (profiles + resolved
   sidebar/navbar/footer). Called once when the user opens a project.
2. **`render_page_in_project(file_path, project_nav_state)`** — resumes
   from the cached `AtProfile` (if we serialize it, or re-runs pass-1 for
   that one file cheaply), then runs pass 2 with `project_nav_state` injected
   into the context.

On edits:
- Profile-affecting changes (title, draft flag, frontmatter, sidebar YAML in
  `_quarto.yml`) → rebuild `ProjectNavState`, re-render active page.
- Body-only changes → re-render active page only; nav state unchanged.

**Preview coupling:** this is the same API shape the future `quarto preview`
CLI wants, backed by an ephemeral hub server instead of a browser session.
Designing these entry points well is how we avoid painting ourselves into a
corner.

### Incremental rebuilds

For the CLI and for hub-client:

- Compute a content hash of each source file (plus the merged metadata that
  affects it — project config files, parent `_metadata.yml`).
- Key the cached `DocumentProfile` on that hash; key cached pass-2 output on
  the same hash plus the project's nav-state hash.
- On re-run, recompute hashes; reuse cached profile if unchanged; rebuild
  nav state (cheap); re-render only pages whose body hash or whose
  nav-relevant context changed.
- Sitemap updates in place: parse existing `sitemap.xml`, update/add entries
  for changed files, write back.

v1 can be conservative (hash-based, single cache dir per project). More
aggressive strategies (dependency tracking across files) are follow-ups.

## Phases

### Phase 0 — Foundations (snapshot contract, resumability, naming)

Sub-plan: `claude-notes/plans/2026-04-23-websites-phase-0.md`
(beads `bd-f3jc`). Contract doc (to be written during
implementation): `claude-notes/designs/document-profile-contract.md`.

Deliverables:
- Final names for `DocumentProfile`, `ProjectType`, and the snapshot stage.
- `DocumentProfile` type (in `quarto-core` or a new `quarto-project` crate —
  to be decided in phase 0), `serde`-serializable.
- Pipeline checkpoint: `PipelineData::AtProfile { … }` (or equivalent),
  `Clone` at this boundary.
- Tests: round-trip serialization, stage advances to checkpoint from a
  fixture and clones cleanly.
- Documentation in `CLAUDE.md` or `claude-notes/` describing the contract:
  "what is guaranteed present in a profile and under what conditions".

Cross-cutting invariant from Phase 0 that later phases must respect:
**no code added for the website epic may branch on "is this a
project?"** — a bare file is a single-file project rooted at its
directory, and the project-relative / output-relative math works
uniformly. See Phase-0 sub-plan §"Project root invariant" for detail.
This inverts a recurring Q1 bug source.

### Phase 1 — Project orchestration

Deliverables:
- `ProjectType` trait.
- `DefaultProjectType` (single-doc + loose-directory fallback, no-ops).
- `ProjectPipeline` driver: discovers files, runs pass 1, builds
  `ProjectIndex`, runs pass 2 per file.
- Integration point in the `quarto` binary so `quarto render` in a directory
  with `_quarto.yml` invokes `ProjectPipeline`.
- Regression: existing single-doc renders still pass all tests (they go
  through `DefaultProjectType` now).

### Phase 2 — Sidebar (data model, generate, render, template)

Deliverables:
- Schema: parse `website.sidebar` as `Vec<Sidebar>`. Each sidebar has
  `id`, `title`, `contents`, `style`, `collapse-level`. Contents supports
  string (path), `{href, text, icon}`, `{section, contents}`, `{auto: …}`.
- Data types in `quarto-navigation`: `Sidebar`, `SidebarEntry`,
  `SidebarContents`.
- `SidebarGenerateTransform` — reads `_quarto.yml` sidebar config and the
  `ProjectIndex` to resolve `auto:` and expand entries with real titles/hrefs.
- `SidebarRenderTransform` — emits HTML (Bootstrap 5 compatible, matches
  Q1 classnames where possible so Q1 CSS continues to work).
- Template slot + integration tests with both manual and auto contents.
- Sidebar-for-page selection (which sidebar applies to which href).

### Phase 3 — Navbar / footer project integration

Deliverables:
- Extend existing `NavbarGenerateTransform` and `FooterGenerateTransform` to
  read project-level config and cross-doc hrefs via `ProjectIndex`.
- Active-item highlighting in rendered navbar (current page).
- Navbar *tools* stay stubs for now (search button disabled — search is a
  follow-up epic).
- Tests: navbar entries pointing at `.qmd` files correctly render as
  `.html` links; external URLs pass through.

### Phase 4 — Page navigation (prev / next)

Deliverables:
- New `PageNavGenerateTransform` / `PageNavRenderTransform` pair, pattern
  identical to the other nav transforms.
- Compute prev/next from flattened sidebar entries.
- Opt in/out per page and per project via `page-navigation: false`.

### Phase 5 — Scoped artifact store and `site_libs/`

Deliverables:
- Add `scope: ArtifactScope { Page, Project }` to artifact entries.
- Project-aware artifact writer that emits `Project`-scoped artifacts once
  to `_site/site_libs/{name}/…`.
- Relocator that rewrites per-page HTML to point at the shared path.
- Migrate theme CSS, Bootstrap, quarto-nav JS, etc. to `Project` scope when
  rendering inside a website project.
- Single-doc renders: both scopes still resolve under `{stem}_files/` to
  preserve current behavior.

### Phase 6 — Cross-document link rewriting

Deliverables:
- HTML post-render transform that rewrites `href` attributes pointing at
  project-relative `.qmd` paths to the corresponding output hrefs, using
  `ProjectIndex`.
- Handles query strings, hash fragments, subdirectories.
- Warning (diagnostic) for broken `.qmd` links.

### Phase 7 — Post-render (sitemap, favicon, site-url/title)

Deliverables:
- `WebsiteProjectType::post_render` orchestration.
- Sitemap generation (`_site/sitemap.xml`, gated on `website.site-url`).
  Incremental-aware: read existing, update, write.
- `robots.txt` referencing sitemap, if not present.
- Favicon copied to output dir and referenced in page `<head>`.
- Title prefix: pages render with `<page-title> — <website-title>` in
  `<title>`.

### Phase 8 — Incremental rebuilds

Deliverables:
- Content hashing for source + merged metadata contributions.
- On-disk cache dir (in `.quarto/` per project) for serialized profiles and
  pass-2 output stubs.
- CLI: detect and reuse unchanged pages.
- Tests: edit one page's body, verify only that page re-renders; edit
  `_quarto.yml` sidebar, verify sidebar rebuilds but bodies don't.

### Phase 9 — Hub-client project rendering

Deliverables:
- WASM API surface: `build_project_nav`, `render_page_in_project`.
- Hub-client state: project-scoped nav cache, invalidation on profile-
  affecting edits.
- Live preview: editing a page's title updates siblings' sidebars within one
  render cycle.
- End-to-end smoke test in a real browser session (per CLAUDE.md policy).

### Documentation — spun out into its own epic (`bd-tr81`)

Originally this epic included a Phase 10 for documentation. We've promoted
it to its own epic (`bd-tr81`) because the motivation for doing websites
*now* is to unblock Quarto 2's own documentation site, and that docs effort
is bigger than a single phase of this epic. The docs epic covers **both**
existing Q2 features and the new website features, all built using Q2
itself (bootstrapping).

The docs epic depends on this one reaching a minimum functional state
(phases 0–2 plus 5–7 — enough to render a navbar + sidebar + shared
resources website). See `bd-tr81` for its own plan.

## Test strategy (cross-cutting, all phases)

- **Fixture projects** under a new `crates/quarto-core/tests/fixtures/websites/`:
  - `minimal/` — two pages, one sidebar, no other features.
  - `auto-sidebar/` — sidebar with `auto: true`.
  - `nested-sections/` — sidebar with nested section groups.
  - `mixed-engines/` — mix of markdown and code-executing pages to exercise
    pass 1 without engine execution.
  - `site-url/` — exercises sitemap and title prefix.
  - `shared-theme/` — exercises scoped artifact relocation.
- **End-to-end CLI verification** per CLAUDE.md §"End-to-end verification"
  for every phase that produces user-visible output (cargo run --bin quarto
  -- render <fixture>; inspect output).
- **Snapshot tests** for rendered HTML fragments (sidebar, page-nav) with
  explicit call-outs when snapshots change.
- **Hub-client smoke test** in phase 9: real browser session showing a
  live-preview update affecting the sidebar.
- **Full-workspace verification** (`cargo xtask verify`) before every commit
  touching `quarto-core` or `quarto-pandoc-types`.

## Open questions to resolve during phase 0

- **Naming** (Document* type, Trait, Stage).
- **Crate placement:** does `DocumentProfile` + `ProjectType` live in
  `quarto-core`, or a new `quarto-project` crate that depends on
  `quarto-core`? Depends on circular-dep analysis.
- **Async vs sync `ProjectType`.** The existing per-file pipeline is sync;
  some Q1 hooks (network, shell out) would benefit from async. Decide in
  phase 1 based on what Website's pre-render needs in MVP.
- **Snapshot location precisely:** is it after metadata merge, or after
  metadata merge + pre-engine sugaring? Sugar mutates the AST (callouts,
  theorems). If sidebars only need title/heading-outline from the raw AST,
  pre-sugar is the cleaner cut. Confirm during phase 0.
- **Profile hashing strategy:** source file only, or include relevant
  `_quarto.yml` / `_metadata.yml` content too? Relevant to phase 8.
- **Cache location:** `{project}/.quarto/cache/`? Gitignored? Shared with
  any existing cache? Relevant to phase 8.

## Explicit non-goals for this epic

- No search, listings/RSS, aliases, announcements, analytics, reader mode,
  repo-actions, breadcrumbs. Each is a follow-up.
- No book project type. (`ProjectType` trait enables it, but book-specific
  features like part/chapter numbering, cross-ref adjustments, appendices
  are out of scope.)
- No `quarto preview` in Quarto 2. (Separate epic; shape of hub-client APIs
  here must support it.)
- No `freeze`. (Shares the serializable-checkpoint substrate; follow-up.)
- No parallel per-file rendering in v1. (The two-pass structure allows it
  cleanly, but v1 ships sequential. A follow-up can add parallelism within
  pass 2.)

## Risks and mitigations

- **Risk:** artifact-store reshape regresses single-doc rendering.
  *Mitigation:* phase 5 must ship a pure refactor first (new scope API,
  identical behavior when all scopes resolve under `{stem}_files/`), then
  switch websites to use `Project` scope as a second step.
- **Risk:** pipeline resumability breaks existing stage invariants.
  *Mitigation:* phase 0 adds a cloneable checkpoint *without* changing the
  rest of the pipeline, and an integration test that clones at the
  checkpoint and resumes produces byte-identical output to running end to
  end.
- **Risk:** hub-client nav invalidation is easy to get wrong (stale
  sidebars, flicker). *Mitigation:* phase 9 adds an explicit invalidation
  log and a smoke test that covers the tricky cases (rename, draft toggle,
  title edit, new file).
- **Risk:** `ProjectType` trait calcifies too early. *Mitigation:* trait
  starts minimal in phase 1. Book/manuscript additions will grow it, but
  only once we know what they actually need.

## Work items

These will be filed as `br` sub-issues under the epic. They mirror the
phases above.

- [x] **Phase 0:** Foundations (snapshot type, checkpoint, naming).
      Closed `bd-f3jc` (commit `e8674612` on `feature/websites`).
      Sub-plan: `claude-notes/plans/2026-04-23-websites-phase-0.md`.
      Contract: `claude-notes/designs/document-profile-contract.md`.
- [x] **Phase 1:** `ProjectType` trait + orchestration.
      Closed `bd-w5os` (commits `5bd92a4a` rename + `c00ee7eb`
      orchestration on `feature/websites`).
      Sub-plan: `claude-notes/plans/2026-04-23-websites-phase-1.md`.
- [x] **Phase 2:** Sidebar data model, generate, render, template.
      Closed `bd-9svl` on `feature/websites`.
      Sub-plan: `claude-notes/plans/2026-04-24-websites-phase-2.md`.
- [x] **Phase 3:** Navbar / footer project integration.
      Closed `bd-fqyg` on `feature/websites`.
      Sub-plan: `claude-notes/plans/2026-04-24-websites-phase-3.md`.
- [x] **Interphase merge:** `main` → `feature/websites` to thread
      `IncludeExpansionStage` (from main, 2026-04-20) through the
      DocumentProfile checkpoint. Post-merge pipeline order runs
      `IncludeExpansion` immediately before `DocumentProfile`, so
      profiles reflect content spliced in via `{{< include … >}}`.
      Closed `bd-xfwx` (merge commit `c3bcfb76` on
      `feature/websites`). Sub-plan:
      `claude-notes/plans/2026-04-24-include-expansion-merge.md`.
      Follow-up `bd-r82e` tracks the deferred
      `DocumentProfile.includes: Vec<…>` field needed for Phase-8
      cache invalidation (see §Epic-wide follow-ups).
- [ ] **Phase 4:** Page navigation (prev/next).
- [ ] **Phase 5:** Scoped artifact store + `site_libs/`.
- [ ] **Phase 6:** Cross-document link rewriting.
- [ ] **Phase 7:** Post-render (sitemap, favicon, site-url/title).
- [ ] **Phase 8:** Incremental rebuilds.
- [ ] **Phase 9:** Hub-client project rendering.

Documentation is tracked separately as `bd-tr81`.

Each phase will get its own `claude-notes/plans/YYYY-MM-DD-*.md` sub-plan
before implementation begins.

## Epic-wide follow-ups surfaced by sub-plans

Issues that transcend a single phase — surfaced while scoping a phase
but with implications across the epic. These must be tracked here so
the epic's close-out catches them; filing as bd happens when the
relevant design work starts.

- **Nav-config placement is inconsistent across features.** Surfaced
  in Phase 2 scoping (see `2026-04-24-websites-phase-2.md` Decision 1
  & 6). Today: `navbar` reads from top-level document metadata,
  `sidebar` reads from `website.sidebar`, and the per-page
  sidebar-id override reads from top-level `site-sidebar`. Q1 has the
  same split. For Q2 we should pick one placement — either
  "everything top-level" or "everything under a nav namespace" — and
  migrate all of navbar / sidebar / page-footer / page-navigation /
  site-url / title-prefix / `site-sidebar` to it. The decision should
  land before we commit to a docs-facing release, but is not a
  blocker for the website epic to ship a working MVP.
- **Sidebar template placement.** Phase 2 puts the sidebar beside the
  TOC on the right, which is the minimum-churn slot in the existing
  full HTML template. Q1 renders sidebar-left, TOC-right. Moving to
  the Q1 layout is a `FULL_HTML_TEMPLATE` restructuring task — not
  sidebar-feature work — and is tracked as a separate follow-up so
  Phase 2 stays scoped to feature implementation.
- **DocumentProfile should record its include set.** Surfaced while
  merging the `IncludeExpansionStage` from `main` ahead of the
  `DocumentProfile` checkpoint (`bd-xfwx`). After the merge, a
  profile can reflect content spliced in from `{{< include … >}}`
  children; the profile therefore *depends on* those child files
  but carries no record of them. For Phase 8 (incremental
  rebuilds) and for eventual `freeze`, the cache-key computation
  needs to invalidate a parent's cached profile when any
  (transitive) include changes. Tracked as `bd-r82e`; not a
  blocker for Phases 4–7.

## Follow-up beads report (running log)

Each phase will accumulate follow-up `bd` issues — deferred work
discovered while scoping or implementing that phase. To keep the
follow-up surface visible in one place, **the final close-out task
of the epic is to produce a single report** listing every `bd` issue
created in service of the website epic and its current status
(open / closed / reassigned). The report should link each issue to
its originating sub-plan so reviewers can trace why each was
deferred.

Running log (update as phases close; cross-link from each sub-plan
when it files an issue):

- **Phase 0 (`bd-f3jc`, closed).** No follow-ups filed at close-out.
- **Phase 1 (`bd-w5os`, closed).** Follow-ups filed at close-out:
  - `bd-ee4z` — Pass-2 resumption from cached `AtProfile`
    (optimization; v1 re-runs the head pipeline).
  - `bd-7tvb` — `.quartoignore` support in file discovery.
  - `bd-k9i1` — `project.resources` support for non-renderable
    site resources.
  - `bd-mlj6` — conditional render lists / `_quarto-<profile>.yml`
    config profiles.
  - `bd-xxul` — non-`.qmd` input extensions (.md / .ipynb / .Rmd)
    in project discovery.
  - `bd-pdwr` — parallel per-file rendering via rayon +
    pollster-per-worker.
- **Phase 2 (`bd-9svl`, closed).** Follow-ups filed at close-out:
  - `bd-6cme` — Sidebar search integration (depends on search epic).
  - `bd-fod3` — Sidebar tools: reader-mode, dark-toggle, etc.
  - `bd-ht0n` — Sidebar logo / subtitle / header / footer rendering.
  - `bd-49ar` — Sidebar collapse/expand JS (rides with Phase 5).
  - `bd-w0o9` — Draft-mode include/visible/exclude option.
  - `bd-l6f0` — Honor explicit `expanded: true` through active
    resolution.
  - `bd-81x4` — Multi-sidebar ambiguity diagnostic.
  - `bd-tfy0` — Deep-directory auto-sidebar grouping (N-level).
  - `bd-2quy` — Audit `StageContext` ↔ `RenderContext` bridge
    completeness. (Phase 2 surfaced a missing `project_index`
    field; a structural guard would prevent recurrence.)
  - `bd-n9dr` — *(epic-wide)* Unify nav config placement across
    features (`navbar` vs `website.sidebar` vs `site-sidebar`).
- **Phase 3 (`bd-fqyg`, closed).** Follow-ups filed at close-out:
  - `bd-jfyl` — Footer `Text`-region project-link rewriting (depends
    on Phase 6's body-link rewriter contract).
  - `bd-jbml` — Navbar index-forgiveness
    (`about/` == `about/index.html`) if a real site hits it.
  - `bd-bwwv` — Navbar sub-row (book-style secondary navbar, epic-
    excluded for MVP).
  - `bd-9m8p` — `navbar.pinned` JS (rides with Phase 5 `site_libs/`).
  - `bd-15dw` — Navbar icon-only item enrichment tie-break.
  - `bd-n9dr` reframed: Phase 3 replaced "unify everything under one
    namespace" with "placement follows feature semantics." The only
    remaining tension is `site-sidebar` at the doc-level override for
    a website-scoped feature. Description updated 2026-04-24.
  - *(no epic-wide follow-up for sidebar template placement; `bd-4g6g`
    remains open from Phase 2 unchanged.)*
  - `bd-4g6g` — *(epic-wide, from Phase 2)* Move sidebar to Q1
    template position (sidebar-left, TOC-right).
- Phases 4–9: TBD.
