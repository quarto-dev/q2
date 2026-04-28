# Phase 9 — Hub-client project rendering

**Date:** 2026-04-27
**Beads:** `bd-ayj6` (parent `bd-0tr6`).
**Parent plan:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Previous phase:** `claude-notes/plans/2026-04-27-websites-phase-8.md`
**Status:** Draft v1 — pending user review.

## Goal of this phase

Phase 9 makes the hub-client live preview render a project page
**as it would appear on the deployed website**: with the project's
sidebar in the gutter, the project's navbar at the top, the
prev/next strip at the bottom, cross-document `[link](other.qmd)`
references rewritten to `other.html`, and shared theme CSS
resolved. Today the live preview calls `render_qmd` for the active
file alone — even when `_quarto.yml` declares `project.type:
website`, none of those features render, because nothing in the
WASM path ever runs Pass 1 over the sibling files that produce
the [`ProjectIndex`](crate::project::index::ProjectIndex) the
website transforms consume.

The win is: a `_quarto.yml` website project edited in hub-client
shows the same page-in-context that `quarto render` would emit on
disk, refreshed live as the user edits. The user can navigate
within the preview by clicking sidebar / navbar / cross-doc links
and the hub follows along (already wired through
`MorphIframe.onNavigateToDocument`).

Phase 9 ships a single new WASM API surface — call it
**`render_page_in_project`** — that drives the full project
two-pass orchestration against the VFS. The hub-client switches
its preview from `render_qmd` to this entry point whenever the
active file lives inside a discovered website project.

This phase is **also** the direct rehearsal for the future Q2
`quarto preview` CLI, which will be a local hub-client instance
(per epic §"Hub-client integration shape"). Anything we wire up
here for the WASM path needs to be reusable by an in-process
preview server later. Concretely: orchestrator code paths must
not branch on `cfg(target_arch = "wasm32")` for *behavior* —
gating is for I/O backend choice (VFS vs disk), not for rendering
semantics.

## What this phase explicitly is **not**

- **No `quarto preview` CLI.** That's its own follow-up epic. We
  only build the API surface and prove it works in-browser.
- **No website post-render disk writes.** Sitemap, robots.txt,
  favicon copy, and `flush_site_libs` to disk all stay native-only.
  The favicon `<link rel="icon">` in `<head>` is a per-page
  Pass-2 transform (Phase 7) and already works in WASM.
- **No new project types.** Phase 9 lights up the existing
  `WebsiteProjectType` on WASM. `BookProjectType` is its own future
  epic; `DefaultProjectType` continues to behave as today (single-
  doc render, no nav).
- **No TS-side `ProjectNavState` cache.** The Phase 8 profile
  cache already lives behind `SystemRuntime::cache_get/set`,
  backed by IndexedDB on WASM. Re-running Pass 1 every render
  consults that cache; the warm path is one IndexedDB read per
  project file. JS-level memoization is an optimization to defer
  until measurement justifies it.
- **No selective "only re-render dependents" logic.** Hub-client
  re-renders the *active page* on any edit, full stop. The Phase 8
  dependency graph is for CLI Mode B (subset render); in
  hub-client there's only one page on screen, and we always render
  it. Mode A (full project render) is also out of scope —
  hub-client doesn't write a `_site/`.
- **No file watching.** Edits arrive through Automerge sync;
  Preview already debounces. No new event source.
- **No Pass-2 caching.** Same reasoning as Phase 8 (filters,
  engines, side effects). Pass-2 always runs on the active page.
- **No multi-tab coordination.** Two browser tabs editing the
  same project both compute their own pass-1 and share the
  IndexedDB profile cache transparently. No explicit
  cross-tab invalidation.

## Reference material

- **Parent epic plan** §"Phase 9 — Hub-client project rendering"
  and §"Hub-client integration shape".
- **Phase 1 sub-plan** §"`pass_one` / `pass_two` driver" — the
  orchestrator we lift onto WASM.
- **Phase 2 sub-plan** §"Sidebar resolution" — sidebar Pass-1
  helpers already cross-platform.
- **Phase 5 sub-plan** §"`ResourceResolverContext::vfs_root`" —
  the synthetic-VFS-path resolver hub-client uses today; will be
  extended for project-scoped artifacts.
- **Phase 6 sub-plan** §"`LinkRewriteTransform`" — already runs
  in WASM when `project_index` is set; Phase 9 makes it set.
- **Phase 7 sub-plan** §"Decision 1 — splits cleanly into per-page
  Pass-2 transforms and post-render writes" — the per-page
  transforms (title prefix, favicon link, canonical URL) work on
  WASM today; only the disk-writing post-render hooks are
  native-only.
- **Phase 8 sub-plan** §"Decision 1 — Cache layout" and
  §"Sub-phase 8.6 — WASM/hub-client cache no-op audit" — confirms
  profile cache is wired through `cache_get/set` and works in
  WASM.
- **Q2 current code:**
  - `crates/wasm-quarto-hub-client/src/lib.rs:906-1036` —
    `render_qmd`. Phase 9 either supersedes this or adds a sibling
    entry point.
  - `crates/wasm-quarto-hub-client/src/lib.rs:691-700` —
    `create_wasm_project_context` (single-file pseudo-context).
    Unused on the project-discovery path; kept for `render_qmd_content`.
  - `crates/quarto-core/src/project/orchestrator.rs:316-796` —
    `ProjectPipeline`. Currently `#[cfg(not(target_arch =
    "wasm32"))]` end-to-end. Phase 9 extracts a WASM-compatible
    driver.
  - `crates/quarto-core/src/project/orchestrator.rs:139-219` —
    `WebsiteProjectType::post_render`. The native body uses
    `website_post_render` which is `#![cfg(not(target_arch =
    "wasm32"))]`; the WASM body needs a stub that does nothing
    (or just flushes Project artifacts to VFS via the resolver).
  - `crates/quarto-core/src/project/website_post_render.rs:33,58`
    — `flush_site_libs`. Native uses `runtime.file_write` to
    `<output_dir>/<lib_dir>/...`. Hub-client needs to write to VFS
    paths under `/.quarto/project-artifacts/...` so the existing
    Phase-5 post-processor finds them.
  - `crates/quarto-core/src/render.rs:127-217` —
    `RenderContext::project_index` and `with_project_index`.
    Already cross-platform.
  - `crates/quarto-core/src/render_to_file.rs` —
    `render_document_to_file`. Native-only;
    `ProjectPipeline::pass_two` calls it. Phase 9 needs a
    WASM-compatible Pass-2 entry point that returns HTML+artifacts
    instead of writing to disk.
  - `hub-client/src/services/wasmRenderer.ts:344-365` —
    `renderQmd` / `renderQmdContent` TS wrappers.
  - `hub-client/src/components/render/Preview.tsx:57-115` — the
    only call site of `renderToHtml`.
  - `hub-client/src/components/FileSidebar.tsx` — the *file* tree;
    do **not** confuse with the website *sidebar* (which renders
    inside the preview iframe).
- **Q1 reference:**
  `external-sources/quarto-cli/src/project/types/website/website.ts`
  for the canonical post-render order. Note that Q1's "preview"
  is a separate server pipeline; Phase 9 collapses that into
  in-browser orchestration, which is genuinely Q2-original.

## Key decisions (to confirm with user)

### Decision 1 — One new WASM entry point (`render_page_in_project`), not two

The epic's first-cut sketch named two surfaces:
`build_project_nav(project_dir)` returning a serializable
`ProjectNavState`, and `render_page_in_project(file_path, state)`
consuming it. The two-call shape made sense when we expected to
explicitly cache `ProjectNavState` on the JS side.

After Phase 8 landed the IndexedDB-backed profile cache, the
two-call shape stops earning its keep:

- The cache key for each profile already invalidates on source /
  metadata / include changes (Phase 8 §Decision 2). Pass 1 over
  the project on every render hits the warm cache for unchanged
  siblings, so the per-render Pass-1 cost is one IndexedDB
  `cache_get` per project file plus a re-extract of the active
  file (whose source bytes the user just edited).
- Maintaining a separate `ProjectNavState` in JS would require
  TS-side invalidation logic (when does it stale? on which edits?
  to which siblings?). The Phase 8 cache key answers these
  questions structurally; replicating that logic in TS would
  diverge.

**Resolved:** ship one new entry point.

```rust
#[wasm_bindgen]
pub async fn render_page_in_project(
    path: &str,
    user_grammars: Option<JsUserGrammars>,
) -> String;
```

It does the same project discovery `render_qmd` does today, then:

1. If the discovered project is **single-file** (no `_quarto.yml`
   found in any ancestor directory) — fall through to the
   existing single-file render path. No orchestration.
2. Otherwise, run the orchestrator: Pass 1 over `project.files`
   (cache-backed), build `ProjectIndex`, run pre-render hooks,
   run Pass 2 only for the active page, run a WASM-flavored
   `post_render` that flushes Project-scoped artifacts to VFS.
3. Return the same `RenderResponse` JSON shape `render_qmd`
   returns today. No new TS-side type.

This keeps the hub-client TS layer thin: `renderToHtml` just
switches `wasm.render_qmd(...)` → `wasm.render_page_in_project(...)`.
No new state, no new invalidation paths.

The future `quarto preview` CLI gets the same entry point — it'll
own a hub-client instance and call exactly this WASM function
through the same JS bridge.

### Decision 2 — Lift the orchestrator off the `wasm32` cfg gate, with WASM-only branches for I/O

Today the entire `ProjectPipeline` driver — every method, the
`Default`/`Website` `post_render` impls, even `RenderToFileResult`
itself — is gated behind `#[cfg(not(target_arch = "wasm32"))]`.
Phase 9 needs the driver to compile and run on WASM, but should
not duplicate the orchestration logic.

**Resolved:** un-gate `ProjectPipeline` and make Pass-2 dispatch
go through a small trait that has separate native and WASM
implementations. The trait absorbs the only truly platform-
specific bit: "render this document and produce an output."

```rust
#[async_trait(?Send)]
pub trait Pass2Renderer {
    type Output;
    async fn render(
        &mut self,
        doc_info: &DocumentInfo,
        format: &Format,
        format_str: &str,
        project: &ProjectContext,
        index: Arc<ProjectIndex>,
        runtime: Arc<dyn SystemRuntime>,
        project_artifacts: &mut ArtifactStore,
    ) -> Result<Self::Output>;
}
```

Native-only impl `RenderToFileRenderer { options: &RenderToFileOptions }`
calls `render_document_to_file` (current behavior).

WASM-only impl `RenderToHtmlRenderer { config: HtmlRenderConfig }`
calls `render_qmd_to_html` (existing in-memory entry point), and
returns the HTML + diagnostics + drained Project-scoped
artifacts.

`ProjectPipeline` is parameterized over the renderer:
`ProjectPipeline<'a, R: Pass2Renderer>`. `pass_two` becomes
generic. All other code (Pass-1 cache lookup, `pre_render`
dispatch, `post_render` dispatch, dependency-graph
`augmented_render_set`) is **already cross-platform** — the
checks confirm it: `pass_one` only touches the runtime, profile
cache, and pipeline stages, none of which are gated.

The `RenderMode::Subset` machinery stays available on WASM (the
type isn't gated) but hub-client never sets it: there's only one
"target" — the active page — and it's always rendered, so Mode A
with `pass2_filter` covers the case.

`WebsiteProjectType::post_render` keeps its native body for
disk-writing hooks and gains a parallel WASM body that runs only
the steps relevant to in-browser preview (Decision 4 below).

### Decision 3 — Pass-2 output type for WASM is `RenderOutput` (HTML + diagnostics + per-doc artifacts)

The native Pass-2 returns `RenderToFileResult { input, output_path,
... }` because the file got written to disk. WASM needs HTML
back to return up to JS, plus enough metadata to populate the VFS
with per-page artifacts (figures, etc.) for the post-processor.

The existing `render_qmd_to_html` already returns
`crate::render::RenderResult` (HTML + `Vec<DiagnosticMessage>` +
the `RenderContext` whose `artifacts` and `diagnostics` we drain).
That's our type.

```rust
pub struct WasmPassTwoOutput {
    pub source_path: PathBuf,
    pub html: String,
    pub diagnostics: Vec<DiagnosticMessage>,
    pub source_context: Option<SourceContext>,
    // Per-page (Page-scoped) artifacts to flush into VFS at
    // resolver-determined synthetic paths.
    pub page_artifacts: ArtifactStore,
}
```

`ProjectPipeline::pass_two` collects a `Vec<WasmPassTwoOutput>`;
since hub-client only renders the active page, the vec is always
length 1. The orchestrator's project-scoped `project_artifacts`
accumulator is shared with the renderer (Phase 5 invariant), so
project-scoped artifacts pile up in the orchestrator across the
single render plus whatever was drained from cached Pass-1
context.

### Decision 4 — Single `flush_site_libs` parameterized on destination root; native vs WASM differ only in the root they pass

Native `WebsiteProjectType::post_render` runs four hooks (Phase 7
§"Decision 11"): `flush_site_libs`, `copy_favicon`,
`write_sitemap`, `write_robots_txt`. The last three write to
`<output_dir>` on disk — meaningless in hub-client (no `_site/`
ever materializes). The first one *does* matter: theme CSS,
quarto JS, etc. need to land somewhere the post-processor finds
them.

**Why a single function works for both platforms.** The
`ResourceResolverContext` is the source of truth for *both*
artifact URLs in HTML and on-disk paths
(`crates/quarto-core/src/resource_resolver.rs:97-223`):

- Native renders construct
  `ResourceResolverContext::website(site_root, page_output, lib_dir, ...)`,
  whose `on_disk_path_for(Project, p)` returns
  `{site_root}/{lib_dir}/{p}` and whose `html_url_for(Project, p)`
  returns the page-relative URL pointing at that path.
- WASM renders construct
  `ResourceResolverContext::vfs_root("/.quarto/project-artifacts")`,
  whose `on_disk_path_for(scope, p)` and `html_url_for(scope, p)`
  both collapse onto `/.quarto/project-artifacts/{p}` regardless
  of scope (the deliberate `vfs_root_mode` flag at
  `resource_resolver.rs:111-118` zeroes out `lib_dir`).

The HTML emitted by Pass 2 already references the correct URL on
each platform, because `html_url_for` was called with the right
resolver. `flush_site_libs`'s only job is "write each artifact's
bytes at the on-disk location the resolver promised."

That is: `flush_site_libs` should compute its destination from
the *resolver*, not from `(output_dir, lib_dir)`. The same
function works on both platforms.

**Implementation.** Un-gate `website_post_render::flush_site_libs`
(remove `#![cfg(not(target_arch = "wasm32"))]` from that one
function — keep the gate on `copy_favicon`, `write_sitemap`,
`write_robots_txt`). Change its signature from
`(project, project_artifacts, lib_dir, runtime)` to
`(project_artifacts, resolver, runtime)`:

```rust
pub(super) fn flush_site_libs(
    project_artifacts: &ArtifactStore,
    resolver: &ResourceResolverContext,
    runtime: &dyn SystemRuntime,
) -> Result<()> {
    if project_artifacts.is_empty() { return Ok(()); }
    let mut entries: Vec<_> = project_artifacts.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    for (_, artifact) in entries {
        let Some(path) = &artifact.path else { continue };
        let on_disk = resolver.on_disk_path_for(ArtifactScope::Project, path);
        if let Some(parent) = on_disk.parent() {
            runtime.dir_create(parent, true).map_err(...)?;
        }
        runtime.file_write(&on_disk, &artifact.content).map_err(...)?;
    }
    Ok(())
}
```

`WebsiteProjectType::post_render` callers pass the resolver they
already constructed for Pass 2:

```rust
async fn post_render(...) -> Result<()> {
    flush_site_libs(project_artifacts, &resolver, runtime)?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        copy_favicon(project, runtime, diagnostics)?;
        write_sitemap(project, index, outputs, runtime)?;
        write_robots_txt(project, runtime)?;
    }
    Ok(())
}
```

This requires plumbing the resolver into `post_render`'s
arguments — already overdue, since today's signature
reconstructs lib-dir math by hand instead of asking the resolver.

**Construction-level invariant** (write a unit test that
enforces it): for every artifact in `project_artifacts`, the
URL embedded in HTML by `html_url_for(Project, p)` and the
write-target computed by `on_disk_path_for(Project, p)` must
both round-trip through the same resolver. A future patch
that changes one and not the other fails this test. This is
the structural guarantee that native and hub-client behavior
stay aligned by construction (answers Decision-2 user concern).

No companion `_wasm` module. No cfg-branched bodies inside
`flush_site_libs`. One function, two callers, one resolver.

### Decision 5 — Hub-client switches `renderToHtml` to call `render_page_in_project`, unconditionally

Today `renderToHtml` calls `renderQmd` which calls `wasm.render_qmd`.
That path will become the single-file fallback inside
`render_page_in_project` (Decision 1 step 1).

By switching the TS layer unconditionally to
`render_page_in_project`, the WASM side owns the project-vs-single
classification (it already needs to do project discovery to find
`_quarto.yml`). TS does no extra work and the contract is one
function.

We keep `render_qmd_content` (path-less, content-string-only) as
an explicit single-document entry point for callers like the
About-page changelog renderer that have no project to discover.

### Decision 6 — Re-render trigger: any edit to any project file (including `_quarto.yml`)

Today `Preview.tsx` re-renders only when the active file's
content changes (`Preview.tsx:268-279`,
`useEffect([content, updatePreview, currentFile?.path])`). With
project rendering active, an edit to *another* file (a sibling's
title; the project's `_quarto.yml`; a `_metadata.yml` deeper in
the tree) changes the active page's *sidebar HTML* or its
metadata-merge result — but the user expects to see those changes
live.

**The infrastructure for "any edit triggers a re-render" already
exists.** Per code audit:

- `automergeSync.ts:91-95` writes every file change (including
  `_quarto.yml`) to the WASM VFS via `vfsAddFile` automatically.
- `App.tsx:370-377` updates `fileContents` via
  `setFileContents(prev => { const next = new Map(prev); ... })`,
  giving a fresh Map identity on every edit. Threaded through
  Editor → PreviewRouter → Preview, this is a stable
  `useEffect`-friendly dependency.
- The Phase-8 cache key
  (`crates/quarto-core/src/project/cache_key.rs`, fed by
  `orchestrator.rs:498-513`) bakes in `_quarto.yml` raw bytes
  and every layered `_metadata.yml`'s raw bytes. A one-byte edit
  invalidates *every* profile cache entry simultaneously — the
  "drop all pass-1 caches on `_quarto.yml` change" behavior is
  achieved structurally without a manual button or special-case
  detection.

**Resolved:** add `fileContents` (the Map) as a `useEffect` dep
in `Preview.tsx`. PreviewRouter already destructures
`fileContents` out of its props; thread it down to Preview.
Existing 20ms debounce in `Preview.tsx:258-265` absorbs burst
edits. No `_quarto.yml`-specific path; no manual reload affordance
in Phase 9.

A lighter alternative — pass a `vfsRevision: number` counter
that increments on each edit instead of the whole Map — is
identical in semantics. Pick whichever fits cleaner during
implementation; the Map identity already works as-is.

In a 100-file project this means every edit triggers a re-render.
On the warm path that's: (a) Pass 1 = 99 IndexedDB reads + one
profile re-extract + one optional re-cache; (b) Pass 2 = render
just the active page. Order-of-100ms territory; debounce makes
it usable.

**The `_quarto.yml`-edit cold path** is the genuine performance
concern: invalidating every profile means N cache misses, each
running the head pipeline. On a 100-file project that's
order-of-seconds. The Phase-1 orchestrator's `pass_one` loop is
sequential today; the work is independent and trivially
parallelizable via `futures::future::join_all` (or the WASM
equivalent). Filed as a Phase-9 follow-up rather than blocking
v1; the user's "drop all caches and re-render" expectation is
met functionally on the first render after a `_quarto.yml`
edit, just not as fast as it could be.

A "Clear cache & reload" UI affordance is also deferred to a
follow-up. The structural automatic invalidation handles
`_quarto.yml`-edit correctness; the manual button is for
escape-hatch debugging, not normal flow.

The active-only-edit case is fast either way because the
sibling profile cache hits short-circuit the heavy work.

### Decision 7 — `vfs_clear` is no longer safe to call between renders; document the invariant in CLAUDE.md and the WASM module

Hub-client already avoids `vfs_clear` between renders (the VFS is
populated once at session start by Automerge and accumulates the
`/.quarto/project-artifacts/...` outputs over time). Phase 9 makes
this implicit invariant load-bearing: the orchestrator's pass-1
cache lives in IndexedDB but artifacts live in VFS, and clearing
mid-session would lose the Phase-5 / Phase-7 outputs the
post-processor needs.

**Make the footgun harder to hit.** Two writes:

1. Doc-comment on `vfs_clear` (`crates/wasm-quarto-hub-client/src/lib.rs:407`)
   spelling out "this is for session teardown only, not between
   renders" with a one-sentence pointer to this plan.
2. A short note in `crates/wasm-quarto-hub-client/CLAUDE.md`
   (the per-crate dev doc) on the VFS state contract — what's
   in `/.quarto/project-artifacts/`, why it must persist across
   renders, and which APIs are safe to call when.

These are cheap and put the invariant where the code lives. No
need for a runtime guard (e.g. asserting non-empty VFS post-clear)
— the doc + the test for "post-processor finds Project artifact
after a re-render" catches regressions structurally.

### Decision 8 — Project discovery on the active path is per-render, not cached across renders

`render_page_in_project` calls `ProjectContext::discover(path,
runtime)` every time it's invoked. That walks parent directories
in the VFS looking for `_quarto.yml`, then enumerates project
files — order of millisecond cost on a VFS whose top directory
has tens of files.

**Resolved:** don't cache discovery in JS. The VFS is in-process,
the walk is cheap, and any caching opens stale-state bugs when
the user adds or removes files. If profiling shows the discovery
cost matters, an in-process Rust-side cache keyed on
`(project_dir, vfs_version)` is a smaller change than a TS-level
one.

### Decision 9 — End-to-end smoke test runs in a real browser session, not vitest

Per CLAUDE.md §"End-to-end verification before declaring success"
and the epic's call-out for Phase 9, this phase requires a real
browser-driven smoke test. Vitest's `*.wasm.test.ts` infrastructure
doesn't exercise Monaco, MorphIframe, or the post-processor — it
calls WASM functions in isolation.

**Resolved:** the smoke test for Phase 9 is:

1. A fixture website project committed under
   `crates/quarto-core/tests/fixtures/websites/hub-smoke/` (mirrors
   the simplest Phase 2 sidebar fixture).
2. A reproducible recipe documented in the plan's verification
   section: open the fixture in hub-client, observe sidebar +
   navbar render, click a sidebar link, observe navigation.
3. A claude-in-chrome browser session captures the GIF as part of
   the close-out evidence (per CLAUDE.md §browser automation).

Vitest covers WASM API correctness in isolation
(`render_page_in_project` returns the same shape as `render_qmd`
on a single-file fixture; it returns sidebar HTML on a website
fixture; etc.). The browser smoke completes the loop.

### Decision 10 — `render_qmd` is **kept** as the single-file entry point

The temptation: replace `render_qmd` outright with
`render_page_in_project`. Don't. `render_qmd` currently:

- Has stable downstream consumers in tests and examples.
- Accepts a single VFS path with no project context (callers can
  pass `/anywhere.qmd` and get a render even outside a project
  directory tree).
- Has been hardened over many sessions (error paths, format
  detection, user grammars, source-context).

`render_page_in_project` *internally* falls through to the same
single-file render code path that `render_qmd` calls today (per
Decision 1 step 1). The two entry points end up sharing 80% of
their bodies — which is an opportunity to extract a private
helper (`render_qmd_inner(...)` or a shared `RenderInputs`
struct), not to delete the public surface. We can deprecate
`render_qmd` in a follow-up once the new entry point has bedded
in.

### Decision 11 — No new TS service module; switch lives in `wasmRenderer.ts`

Tempting to introduce a `projectRenderer.ts` service to hold
project-rendering state. But after Decision 1 (one entry point)
and Decision 6 (no JS-side cache), there's no state to hold. The
change in `wasmRenderer.ts` is one line of dispatch.

A new service module would be appropriate later, when TS-level
features land (e.g. a project-tree visualization sourced from
`ProjectIndex`, a "broken-link panel" surfacing
`LinkRewriteTransform` diagnostics across files). Phase 9 doesn't
have those — defer the module.

### Decision 12 — Diagnostic surfacing: project-level diagnostics flow into the same Monaco markers panel

Phase 7 added `ProjectRenderSummary.project_diagnostics` for
post-render warnings (e.g. "favicon source not found"). Phase 9's
WASM post-render is much smaller, but `pre_render` /
`flush_site_libs_to_vfs` could emit warnings. Those should reach
the user.

**Resolved:** the WASM `RenderResponse.warnings` array (already
present) absorbs project-level diagnostics in addition to per-page
ones. The orchestrator's `project_diagnostics` get appended to the
returned page's `output.diagnostics` before the JSON
serialization. Monaco markers handling in TS is unchanged.

If a future feature needs to distinguish per-page vs project-level
warnings in the UI, we'll add a `scope: "page" | "project"` field
then. For now they flow through the same channel.

## Architecture sketch

### Module shape after Phase 9

```
crates/quarto-core/src/project/
├── orchestrator.rs              # un-gated; uses Pass2Renderer trait
├── pass2_renderer.rs            # NEW: trait + native impl
├── pass2_renderer_wasm.rs       # NEW: WASM impl (cfg-gated)
├── website_post_render.rs       # native disk-writing hooks
└── website_post_render_wasm.rs  # NEW: VFS flush only

crates/wasm-quarto-hub-client/src/
└── lib.rs                       # adds render_page_in_project(...)

hub-client/src/services/
└── wasmRenderer.ts              # one-line switch + type binding
```

### Data flow on a project page render

```
Editor commits an edit
  ↓
Automerge sync pushes the change to VFS
  ↓
Preview.tsx debounce fires → renderToHtml({ documentPath, ... })
  ↓
wasmRenderer.ts → wasm.render_page_in_project(path, grammars)
  ↓                      [Rust, in WASM]
ProjectContext::discover(path, runtime)
  ├── single-file project? → renderQmdSingle (existing path) → return HTML
  └── website project?      ↓
                            ProjectPipeline<WasmPass2Renderer>::run()
                              ↓
                            Pass 1 over every file:
                              for each file in project.files:
                                  cache_get(profile_key) → hit? return profile
                                                       → miss: run head pipeline,
                                                                cache_set, return
                              build ProjectIndex
                              ↓
                            WebsiteProjectType::pre_render(project, index)  [v1: no-op]
                              ↓
                            Pass 2 for the active page only:
                              run_pipeline(...) with project_index injected
                              → returns HTML + diagnostics + page_artifacts
                                                              + drained project_artifacts
                              ↓
                            WebsiteProjectType::post_render(WASM impl):
                              flush_site_libs_to_vfs(project, project_artifacts, lib_dir, runtime)
                              ↓
                            return RenderResponse{ html, warnings: page_diags + project_diags, ... }
                                                    ↑
                                                    serialize to JSON
  ↓
renderToHtml unwraps → MorphIframe receives html
  ↓
post-processor reads /.quarto/project-artifacts/... from VFS
  ↓
sidebar / navbar / page-nav / cross-doc links render in iframe
```

### What's *not* in the data flow

- No JS-side `ProjectNavState` cache.
- No file-watcher; edits arrive through the existing Automerge
  channel.
- No `_site/` writing; no sitemap, robots.txt, or favicon copy.
- No JS-side project discovery; Rust does it from the VFS each
  render (cheap; see Decision 8).
- No new IndexedDB structure; profile cache reuses Phase 8's.

### Single-doc vs project regression check

Single-file renders (a `.qmd` with no `_quarto.yml` ancestor)
take the `renderQmdSingle` branch inside `render_page_in_project`,
which is the existing `render_qmd` body extracted into a helper.
Behavior must be byte-identical to today.

Project files inside `DefaultProjectType` (a directory with a
`_quarto.yml` declaring `project.type:` absent or `default`):
Pass-1 still runs (so `ProjectIndex` is built and the
`LinkRewriteTransform` can resolve cross-doc links) but
`DefaultProjectType::post_render` is a no-op. So we get
cross-doc link rewriting "for free" on default projects, which
is consistent with native CLI behavior. No regression risk: the
website-only transforms (sidebar, navbar, page-nav generate)
short-circuit when the project config doesn't include their
config keys.

## Tests (TDD: write and fail first)

### Unit tests — `Pass2Renderer` trait + impls (`crates/quarto-core/src/project/pass2_renderer.rs`)

**Test 1.** `RenderToFileRenderer` round-trips: stub `ProjectContext`,
stub `Format`, stub render function — confirm the trait dispatch
calls the underlying `render_document_to_file` once per call.

**Test 2.** WASM `RenderToHtmlRenderer` populates `WasmPassTwoOutput`
with HTML, diagnostics, and drained `page_artifacts`.

### Unit tests — `ProjectPipeline` un-gating

**Test 3.** `ProjectPipeline::pass_one` compiles on `target_arch =
"wasm32"` (rustdoc cfg-gated test, or a `#[cfg(test)]` smoke
that constructs a pipeline against a `MockRuntime` on both
targets).

**Test 4.** `RenderMode::Full` with a `Pass2Renderer` that filters
to a single target file produces output only for that file.
This proves hub-client's "render only the active page" pattern
works through the existing orchestrator without inventing a new
mode.

### Unit tests — WASM post-render (`website_post_render_wasm.rs`)

**Test 5.** `flush_site_libs_to_vfs` writes Project-scoped
artifacts to `/.quarto/project-artifacts/<lib_dir>/<path>`.

**Test 6.** No artifacts → no-op (no spurious empty directories
in VFS).

**Test 7.** Empty `lib_dir` (DefaultProjectType) → artifacts go to
`/.quarto/project-artifacts/<path>` (matches Phase 5 single-doc).

### Integration tests — `crates/wasm-quarto-hub-client/tests/` or vitest under hub-client

**Test 8** (vitest, `wasmRenderer.test.ts` extension or new
`projectRender.wasm.test.ts`). Two-file website fixture: load
into VFS, call `render_page_in_project('/project/index.qmd')`,
assert response HTML contains the sidebar entry for `/project/about.qmd`.

**Test 9.** Edit `about.qmd`'s title via `vfs_add_file`, re-render
`index.qmd`, assert the sidebar entry text reflects the new title.
This is the live-preview invariant.

**Test 10.** Single-file (no `_quarto.yml`) project: behavior
identical to today's `render_qmd` (no sidebar markup in HTML).

**Test 11.** `_quarto.yml` declares `project.type: website` but no
`website.sidebar` config: orchestrator runs without errors,
returns HTML with no sidebar block (graceful absence).

**Test 12.** Cross-document link rewriting: page A contains
`[link](b.qmd)`, after `render_page_in_project` the returned HTML
contains `href="b.html"`.

**Test 13.** Project-scoped artifacts land in VFS at
`/.quarto/project-artifacts/site_libs/...` after a website render;
the post-processor's `<link>` tags reference these paths.

**Test 14.** Phase 7 per-page transforms still fire: title prefix
is applied (`<title>Page — Project</title>`), `<link rel="icon">`
is inserted when `website.favicon` is set in `_quarto.yml`.

### Browser smoke (per CLAUDE.md §"End-to-end verification")

**Test 15** (manual + scripted via claude-in-chrome). Open a
fresh hub-client session against a website fixture at
`crates/quarto-core/tests/fixtures/websites/hub-smoke/`:

- Three pages: `index.qmd`, `about.qmd`, `posts/first.qmd`.
- `_quarto.yml` declares `project.type: website` plus a sidebar
  with manual entries for the three pages.

Verify:

a. Opening `index.qmd` shows the sidebar with three entries.
b. Clicking the "About" sidebar entry navigates to `about.qmd`.
c. Editing `about.qmd`'s frontmatter title in Monaco causes the
   `index.qmd` preview's sidebar entry to update on next focus
   switch (or in real time if the user is on `index.qmd`).
d. A cross-document link in `index.qmd`'s body
   (`[link to about](about.qmd)`) renders as `href="about.html"`
   and clicking it triggers `onNavigateToDocument`.
e. No console errors; no broken `<link>` to theme CSS.

GIF capture lives in
`claude-notes/research/2026-04-27-websites-phase-9-smoke.gif`
(or similar) and is referenced from the close-out commit.

### Snapshot tests

**Test 16.** Snapshot of the integration-test website's rendered
`index.qmd` HTML, scoped to the sidebar+navbar block. Captures
regressions when Phase 2/3 transforms evolve.

## Work items (checklist)

### Sub-phase 9.0 — Trait extraction (`Pass2Renderer`)

- [x] Add `pass2_renderer.rs` with the `Pass2Renderer` trait.
- [x] Move `render_document_to_file` calls in
      `ProjectPipeline::pass_two` behind a `RenderToFileRenderer`
      impl.
- [x] Confirm native test suite is byte-identical (snapshot
      diff = empty). 8062 tests pass; 0 snapshot files changed.
- [x] **Verification gate:** `cargo xtask verify --skip-hub-build`
      passes.

**Notes.** `ProjectPipeline<'a>` became
`ProjectPipeline<'a, R: Pass2Renderer = RenderToFileRenderer<'a>>`;
the existing `new()` constructor is unchanged for callers (it
forwards to the new `with_renderer`). `ProjectRenderSummary` is
now generic over the per-page output type with a
`RenderToFileResult` default. The `run()` method carries an
extra `R::Output = RenderToFileResult` bound until sub-phase 9.2
relaxes `ProjectType::post_render`'s output-slice contract.

### Sub-phase 9.1 — Un-gate `ProjectPipeline` for WASM

- [x] Remove `#[cfg(not(target_arch = "wasm32"))]` from
      `ProjectPipeline`, `pass_one`, `pass_two`,
      `compute_augmented_render_set`, `profile_with_cache`,
      and the helpers.
- [x] Keep `RenderMode`, `ProjectRenderSummary`, `FileFailure`
      cross-platform (`ProjectRenderSummary` was native-only after
      9.0; gates lifted now that it's generic on output type).
- [x] `RenderToFileResult` placeholder for `target_arch =
      "wasm32"` retained (orchestrator-local unit struct).
- [ ] Make `WebsiteProjectType::post_render` `#[cfg(target_arch =
      "wasm32")]` companion (Decision 4) — **deferred to 9.2**: the
      WASM body lands together with `flush_site_libs_to_vfs` and the
      resolver-plumbing refactor.
- [x] **Verification gate:** `cargo xtask verify` passes (Rust +
      hub-client WASM build + all tests). 8062 workspace tests
      green.

**Notes.** The cross-platform impl block
`impl<'a, R: Pass2Renderer> ProjectPipeline<'a, R>` now compiles on
WASM. The native-only impl block
`impl<'a> ProjectPipeline<'a, RenderToFileRenderer<'a>>` (containing
`new()`) stays gated because it depends on
`crate::render_to_file::RenderToFileOptions`. The default generic
parameter `R = RenderToFileRenderer<'a>` was dropped — all native
callers go through `ProjectPipeline::new()` and benefit from
constructor-driven type inference, so the default added no real
ergonomics and would have needed cfg-gating to work on WASM.

### Sub-phase 9.2 — WASM Pass-2 renderer + post-render

- [ ] Add `pass2_renderer_wasm.rs` with the `RenderToHtmlRenderer`
      impl (calls `render_qmd_to_html`, drains artifacts).
- [ ] Add `website_post_render_wasm.rs` with
      `flush_site_libs_to_vfs`.
- [ ] Wire the WASM `WebsiteProjectType::post_render` body to
      call `flush_site_libs_to_vfs`.
- [ ] Unit tests 5–7.
- [ ] **Verification gate:** `cargo xtask verify`.

### Sub-phase 9.3 — `render_page_in_project` WASM entry point

- [ ] Extract the body of current `render_qmd` into a private
      helper `render_qmd_single(path, content, runtime,
      user_grammars) -> RenderResponse`.
- [ ] Add `render_page_in_project(path, user_grammars)` that:
      - discovers project context,
      - falls through to `render_qmd_single` for single-file,
      - otherwise constructs `ProjectPipeline<RenderToHtmlRenderer>`
        with `RenderMode::Full` plus a single-target filter,
      - returns the same `RenderResponse` shape.
- [ ] Unit tests 8–14 (vitest under hub-client).
- [ ] Confirm the existing `render_qmd` still works
      (call-compat check).
- [ ] **Verification gate:** `cargo xtask verify` + hub-client
      vitest suite.

### Sub-phase 9.4 — Hub-client switch (`wasmRenderer.ts`)

- [ ] Add `render_page_in_project` to the `WasmModuleExtended`
      interface.
- [ ] Add a `renderPageInProject` TS function mirroring `renderQmd`.
- [ ] Switch `renderToHtml`'s call from `renderQmd` to
      `renderPageInProject`.
- [ ] Update the `Preview.tsx` `useEffect` deps to include
      `files` so any sibling edit triggers a re-render.
- [ ] **Verification gate:** `cd hub-client && npm run build:all
      && npm run test:ci` passes.

### Sub-phase 9.5 — Browser smoke fixture + verification

- [ ] Add `crates/quarto-core/tests/fixtures/websites/hub-smoke/`
      with `_quarto.yml`, `index.qmd`, `about.qmd`,
      `posts/first.qmd`.
- [ ] Run the smoke test manually, document the recipe in this
      plan and in close-out commit.
- [ ] Capture GIF via claude-in-chrome.
- [ ] **End-to-end gate:** the recipe in Test 15 passes; the GIF
      is committed.

### Sub-phase 9.6 — Close-out

- [ ] Update epic plan §"Work items" to mark Phase 9 done.
- [ ] File follow-ups discovered during implementation
      (`discovered-from:bd-ayj6`, `parent-child:bd-0tr6`).
- [ ] Per-CLAUDE.md: snapshot file count + summary; explicit
      callouts for any surprising changes.
- [ ] Run `cargo xtask verify` one more time clean.
- [ ] Stage commits, ask user before pushing.

## Risks and mitigations

- **Risk:** un-gating `ProjectPipeline` blows up the WASM build
  with hidden native dependencies (e.g. tokio file I/O,
  `walkdir`).
  *Mitigation:* sub-phase 9.0 + 9.1 are the un-gating phases; the
  trait extraction goes first specifically so the file-I/O
  callsite is the only thing left native. The verification gate
  for 9.1 is a full WASM build — if it fails, we know exactly
  which import to migrate behind the trait.

- **Risk:** rendering all sibling profiles on every keystroke
  (Decision 6) makes hub-client feel laggy.
  *Mitigation:* warm-path is one IndexedDB read per file plus a
  Pass-2 only on the active page — measure on a 100-file fixture
  before releasing. If latency is a problem, two ways to fix it
  short of full Mode-B graph machinery: (a) only re-run Pass-1 on
  files whose VFS bytes changed since last render; (b) cache the
  `ProjectIndex` itself in JS keyed on (project_dir, vfs_version).
  Both are TS-side and don't change the Rust contract. Defer
  unless measurement says it matters.

- **Risk:** `flush_site_libs_to_vfs` accumulates stale artifacts
  in VFS over the session (e.g. an old theme CSS hash sticks
  around when the user changes themes).
  *Mitigation:* Phase 5 already keys theme CSS by content
  fingerprint; stale entries don't *poison* the page (the new URL
  doesn't reference them) but they do leak VFS storage. Add a
  follow-up to GC `/.quarto/project-artifacts/...` entries with
  no live references at session end. Not a Phase-9 blocker.

- **Risk:** project discovery walks the VFS up to root every
  render and finds no `_quarto.yml`, but its absence makes us
  fall through to single-file rendering — which is correct, but
  costs a few `path_exists` calls per render.
  *Mitigation:* irrelevant; the walk is bounded by directory
  depth (typically 3–5 levels) and `path_exists` on the in-memory
  VFS is microseconds. No-op for performance. If a future fixture
  has 20-level nesting we'll cache the discovery result; not
  before.

- **Risk:** `WebsiteProjectType::post_render` WASM body and native
  body drift: a Phase-7 follow-up adds a fifth post-render hook,
  someone wires it native-only, and the in-browser preview falls
  out of parity with the deployed site.
  *Mitigation:* document this explicitly in
  `WebsiteProjectType::post_render`'s rustdoc — every new hook
  must answer "is this disk-only or does it shape the rendered
  page?" and if it shapes the page, both bodies need to call it.
  The Phase-7 transforms (title prefix, favicon link, canonical
  URL) are *Pass-2 transforms*, not post-render hooks, so this
  risk is small.

- **Risk:** the `Pass2Renderer` trait shape calcifies before we
  know if `quarto preview` (separate epic) needs a different
  output type.
  *Mitigation:* the trait is internal to `quarto-core`, not a
  public API. We can change it freely. Adding a third impl for
  a future preview-server context is fully expected.

- **Risk:** browser smoke is flaky because real user grammars or
  Monaco timing.
  *Mitigation:* the smoke fixture deliberately has no exotic
  grammars (plain markdown + frontmatter). The Monaco-load
  problem from Phase-3 syntax-highlighting is documented; we
  rely on the existing init order, not on race-prone timing.

## Explicit non-goals for this phase

- No `quarto preview` CLI wrapper.
- No on-disk site_libs flush from hub-client.
- No sitemap.xml / robots.txt / favicon.ico in hub-client.
- No JS-side `ProjectNavState` data type.
- No JS-side project-discovery cache.
- No selective re-render (the Phase-8 dependency graph remains
  CLI-Mode-B-only).
- No Pass-2 caching (per Phase 8 §"Why no Pass-2 cache").
- No multi-tab cross-coordination.
- No GC of stale VFS artifacts (deferred follow-up).
- No book / manuscript project rendering in hub-client.
- No `freeze` integration (separate epic).
- No deprecation of `render_qmd` (kept; deprecation is a
  follow-up).

## Open questions (remaining after user review 2026-04-28)

1. **Browser smoke recipe vs. scripted harness.** User said
   "GIF is fine; a one-off recipe with clear ops + expected
   outcomes is also fine." Phase 9 will produce both: a
   claude-in-chrome GIF and a written recipe in the close-out
   commit. After we observe how the smoke test goes in
   practice, we'll design follow-up automated coverage if the
   manual recipe surfaces specific failure modes.

2. **No remaining blockers to implementation.** All other
   decisions confirmed in the user-review pass; see Decisions
   log below.

## Decisions log (user confirmed 2026-04-28)

- **Decisions 1–6, 8, 10–12.** Confirmed as written.
- **Decision 4 (single `flush_site_libs` parameterized on
  resolver).** Redrafted to use one function with a resolver
  parameter rather than a companion `_wasm` module. Construction
  guarantees the URL the resolver embeds in HTML matches the
  on-disk path the post-render writes to. Closes the open
  question about how to keep native and hub-client behavior
  aligned.
- **Decision 6 (re-render on any edit).** Confirmed; relies on
  the `fileContents` Map identity passed through from
  `App.tsx:370-377`. Code audit confirms `_quarto.yml` edits
  arrive at the WASM VFS automatically and the Phase-8 cache
  key invalidates all profiles in lockstep when `_quarto.yml`
  bytes change, so structural automatic invalidation handles
  the "drop all caches" case without a manual button.
- **Decision 7 (no `vfs_clear` between renders).** User flagged
  this as a footgun worth proactive guardrails; expanded to
  include both an inline doc-comment and a per-crate
  `CLAUDE.md` note. No runtime guard (would be redundant with
  the test that asserts artifact persistence across renders).
- **Trait abstraction over `Pass2Renderer`** (former open
  question 1). Confirmed. Q1's parallel-implementation pattern
  was a recurring bug source; Q2 actively avoids it. The
  `Pass2Renderer` trait is the single source of truth for
  Pass-2 dispatch.
- **Companion module vs. cfg-gated bodies** (former open
  question 2). Resolved in favor of *neither* by the Decision-4
  redraft: one parameterized function. The user noted some
  diverging native/WASM impls are acceptable but would prefer
  guardrails on expected behavior — that's exactly the
  resolver/post-render unit test specified in Decision 4
  ("construction-level invariant").
- **Re-render on any edit** (former open question 3). Confirmed.
  Performance optimization deferred to follow-up.
- **`_quarto.yml` change propagation** (former open question 5).
  Resolved: structural automatic invalidation handles it. No
  special detection needed; full re-render on any edit is the
  v1 behavior. Manual "Clear cache & reload" UI is a deferred
  follow-up.

## Epic-level impact

- **`bd-ayj6`** closes when sub-phase 9.6 commits.
- **`bd-ee4z`** (Pass-2 resumption from cached AtProfile,
  Phase-1 follow-up) becomes more attractive but is still not
  a Phase-9 blocker — Phase 9 inherits Phase 1's "re-run head
  pipeline in Pass-2" pattern.
- **`bd-vdl8`** (retire `DEFAULT_CSS_ARTIFACT_PATH`,
  Phase-5 follow-up) was tagged "rides with Phase 9". Phase 9's
  `flush_site_libs_to_vfs` is the natural spot to handle the
  cleanup. Will close as part of 9.2 if straightforward;
  otherwise file as discovered-from-9.
- **Future `quarto preview` epic** depends on Phase 9's
  `render_page_in_project` API surface. Phase 9 explicitly
  validates the API shape works for the preview use case — by
  building it and using it in hub-client, which *is* the
  in-browser preview.

## Follow-up beads (to file at close-out)

To be filled in as the implementation surfaces them. Likely
candidates already visible:

- `bd-XXXX` — VFS GC for stale `/.quarto/project-artifacts/...`
  entries on session end. Risk-list item.
- `bd-XXXX` — Smarter Preview re-render filter (don't re-render
  on edits that can't affect the active page). Decision-6
  follow-up.
- `bd-XXXX` — Cache the `ProjectIndex` (not just profiles) in
  IndexedDB to skip re-building it across renders. Optimization.
- `bd-XXXX` — Deprecate `render_qmd` once `render_page_in_project`
  is the universal entry point. Decision 10.
- `bd-XXXX` — Hub-client UI affordance for project-level
  diagnostics (today they fold into the per-page Monaco markers;
  consider a dedicated panel for cross-cutting warnings like
  broken cross-doc links).
- `bd-XXXX` — Parallel Pass-1 in the orchestrator. The
  `_quarto.yml`-edit cold path invalidates every profile cache
  entry simultaneously; on a 100-file project the sequential
  loop in `pass_one` runs N head pipelines. Trivially
  parallelizable (independent work per file). Decision-6
  follow-up.
- `bd-XXXX` — "Clear cache & reload" UI affordance in
  hub-client (calls `cache_clear_namespace("profiles")` then
  triggers a re-render). Escape hatch for debugging; not
  required by automatic invalidation. Decision-6 follow-up.
