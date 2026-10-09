# Project Resources: user- and engine-declared additional files

**Date:** 2026-05-03
**Status:** Draft v2 — design questions resolved with the user 2026-05-03.
**Do not start implementation until the user gives the go-ahead.**

## Goal

Quarto 2 currently determines the published file set from the render
pipeline alone. There is no way for an author, a Lua filter, or an
engine to say "also include this file in `_site/` and in any `quarto
publish` deployment."

This plan adds that mechanism, modelled on Quarto 1's `resources:`
metadata field plus the engine `supporting`/`resourceFiles` channel,
adapted to Quarto 2's stage pipeline + artifact store.

In scope:

1. Author-declared resources in `_quarto.yml` (project level) and in
   document YAML frontmatter (document level).
2. Engine-declared resources surfaced through `ExecuteResult`.
3. Lua-filter-declared resources via a small Quarto Lua API
   (analogous to Q1's `quarto_global_state.results.resourceFiles`).
4. Routing of declared resources into the output directory and into
   the `publish` command's file list.

Out of scope (filed as follow-ups, not this issue):

- Auto-discovery of referenced files (Image `src` walks, OJS
  `FileAttachment`, include-file scanning). Q1 does this with a
  built-in Lua filter; we'll port it once we have an explicit channel
  to write to.
- Negative-glob exclusion (`resources: ["!internal/*"]`).
- Resource-aware incremental rebuilds (treating declared resources
  as cache-invalidation inputs).

## Q1 reference

Already researched in this session — see "Findings" section below.
Key Q1 file paths for back-reference during implementation:

- `external-sources/quarto-cli/src/command/render/resources.ts`
  — `resourcesFromMetadata`, `resolveFileResources` (glob expansion).
- `external-sources/quarto-cli/src/project/project-resources.ts`
  — `projectResourceFiles` (project-level).
- `external-sources/quarto-cli/src/execute/types.ts:166-178`
  — `ExecuteResult.supporting` + `ExecuteResult.resourceFiles`.
- `external-sources/quarto-cli/src/resources/filters/quarto-pre/resourcefiles.lua`
  — `recordFileResource` Lua API + Image-walking auto-discovery.
- `external-sources/quarto-cli/src/resources/filters/mainstateinit.lua`
  — `quarto_global_state.results.resourceFiles` initialization.
- `external-sources/quarto-cli/src/command/render/pandoc.ts:1405-1437`
  — sidecar JSON read-back, merge into `RunPandocResult.resources`.
- `external-sources/quarto-cli/src/command/render/types.ts`
  — `RenderResultFile.resourceFiles` shape.
- `external-sources/quarto-cli/src/publish/publish.ts`
  — resource paths flow into the publish file list.

### Findings (Q1 mechanism summary)

```
User YAML: resources: ["data/*.csv", "img/**/*.png"]    (string | array)
       │
       ▼  resourcesFromMetadata()
   string[]
       │
       ▼  resolveFileResources()  (glob expand, exclude .quarto/, hidden dirs)
   absolute paths
       │
   ┌───┼──────────────────────────────────────────────────┐
   │   ▼                                                  │
   │   Lua filter pass:                                   │
   │     - built-in resourcefiles.lua walks Image, OJS    │
   │     - user filters call recordFileResource(path)     │
   │     - all push into                                  │
   │         quarto_global_state.results.resourceFiles    │
   │     - filter pipeline writes a sidecar JSON          │
   │                                                      │
   │   Engine pass (jupyter/knitr):                       │
   │     - returns ExecuteResult { supporting, resourceFiles }
   │     - supporting = directory of engine assets        │
   │     - resourceFiles = specific extra paths           │
   │                                                      │
   ▼                                                      ▼
RunPandocResult.resources  +  ExecuteResult.{supporting,resourceFiles}
       │
       ▼
RenderResultFile { resourceFiles, supporting }
       │
       ▼
publish.ts → PublishFiles → provider.publish(...)
```

The two-channel split (`supporting` vs `resourceFiles`) matters:

- `supporting` is a **directory** of engine-managed assets (e.g.
  `foo_files/`). Treated as a unit, copied verbatim, can be
  garbage-collected.
- `resourceFiles` is a **list of specific file paths**. Treated as
  loose deliverables, kept across renders.

We should preserve that distinction in Q2.

## Q2 current state (relevant)

- `DocumentProfile` (`crates/quarto-core/src/document_profile.rs`,
  v2): no resources field. Frozen after `MetadataMergeStage` and
  `IncludeExpansionStage`, **before** user filters run.
- `ExecuteResult` (`crates/quarto-core/src/engine/context.rs:132`):
  has `supporting_files: Vec<PathBuf>` but not yet wired into
  publish; no `resource_files` field.
- `RenderedOutput` and `FinalOutput`
  (`crates/quarto-core/src/stage/data.rs:388-421`):
  have `supporting_files`; not yet propagated to the publish step.
- User filters (`UserFiltersStage::pre/post`) run after the profile
  checkpoint, so any meta mutation they do is **not** retro-applied
  to the profile.
- Publish (`crates/quarto/src/commands/publish.rs`) currently
  discovers files by walking `output_dir`. No render-side manifest.
- Lua filter infrastructure: actively being ported (epic `k-thpl`),
  no `quarto.*` Lua API yet — we'll need to add one (or piggyback
  on an existing extension point) for `resource_files`.

## Proposed design

### Resolved design principles (from 2026-05-03 review)

- **Author declarations are intended to be irrevocable.** A
  document or `_quarto.yml` `resources:` entry should be treated
  by engines and filters as an affirmative, irrevocable choice.
  This is a **documented requirement**, not a structural one: a
  Lua filter can mutate `meta.resources` on the AST and an engine
  can clobber metadata before returning it. We do not police that.
  We document the requirement and design the *Quarto-provided*
  APIs (`quarto.doc.add_resource`, `ExecuteResult.supporting_files`)
  as append-only — well-behaved code has no reason to reach around
  them. Downstream consumers (publish, static analysis tools)
  must not rely on additive-only as a hard invariant; they treat
  it as a best-effort lower bound.
- **DocumentProfile remains read-only.** Per the profile contract
  (`claude-notes/designs/document-profile-contract.md`), profiles
  are immutable post-checkpoint. We do **not** introduce a mutable
  channel on the profile or in `StageContext`. Instead, engine and
  filter contributions are accumulated in a separate **render
  report** that's collected by a late-pipeline pass and merged
  with the profile's static list at the end. The profile keeps a
  snapshot of *what the author declared at frontmatter freeze
  time*; the report records *what was contributed downstream*.
  Static-analysis tools can read the profile alone to compute a
  best-effort lower bound without running the pipeline.
- **`resources:` is format-agnostic in v1.** Q1's HTML-only tag is
  almost certainly a website-vs-single-file historical accident;
  Q2 lets formats that have no use for resources simply ignore the
  field. We can re-introduce per-format gating later if a real
  conflict appears.

### Data model

```rust
pub struct ResourceEntry {
    /// Source path (project-relative if possible, otherwise absolute).
    pub source: PathBuf,
    /// Where this entry came from. Used for diagnostics and de-dup.
    pub origin: ResourceOrigin,
    /// Project- vs document-anchored.
    pub scope: ResourceScope,
}

pub enum ResourceOrigin {
    ProjectMetadata,           // _quarto.yml `resources:`
    DocumentMetadata(PathBuf), // doc YAML `resources:` (path = source qmd)
    Engine { engine: String, source: PathBuf },
    LuaFilter { source: PathBuf },
    // Reserved for the auto-discovery story (open question 7):
    // AutoDiscovery { kind: AutoDiscoveryKind, source: PathBuf },
}

pub enum ResourceScope {
    Project, // anchored at output_dir root
    Page,    // anchored at document's output dir
}
```

### Two channels, joined at end of render

| Origin                | Channel                                | When known     | Mutability |
|-----------------------|----------------------------------------|----------------|------------|
| `_quarto.yml`         | `ProjectConfig.resources` (new field)  | Project setup  | frozen     |
| Document YAML         | `DocumentProfile.resources` (v3)       | Profile freeze | frozen     |
| Engine                | `ExecuteResult.supporting_files` (extended use — see "Engine channel" below) | Post-engine | append-only |
| Lua filter            | Sidecar Lua table → drained by a late stage | Filter pass | append-only |

A new **late-pipeline collector stage** (call it
`ResourceReportStage`, sitting after `UserFiltersStage::post`) reads:

1. The frozen profile's `resources` (snapshot of author declarations
   at frontmatter freeze).
2. The current AST's `meta.resources` at collector time (which a
   filter may have mutated since the profile was taken).
3. The engine's `supporting_files` (already on `ExecuteResult`,
   propagated through `RenderedOutput`).
4. The Lua sidecar table (drained from the Lua state after each
   filter pass).

…and produces a single `DocumentResourceReport` attached to
`RenderedOutput` / `FinalOutput`. The orchestrator's post-render
hook merges per-document reports with the project-level list and
emits the manifest.

Item (2) means: we re-read `meta.resources` from the post-filter
AST so that filters mutating metadata still take effect. We
**union** that against the profile snapshot from (1) — we never
treat the post-filter AST as authoritative for *removing* items
the author declared. If a filter shrunk the list, the dropped
entries are still published (with a diagnostic noting the divergence
in verbose mode). This implements the documented "additive-only"
intent without depending on filter authors to honour it.

Note that we still cannot prevent an engine or filter from
clobbering arbitrary metadata in surprising ways; the union with
the profile snapshot is a defence in depth, not a guarantee.
Downstream consumers should not rely on the report being a
strict superset of the author declarations under all
circumstances — only that we do our best to make it so.

### Lua API for filters

`quarto.doc` already exists at
`crates/pampa/src/lua/quarto_doc.rs` with `is_format`,
`add_html_dependency`, `include_text`. We extend it with:

```lua
quarto.doc.add_resource(path)            -- snake_case
quarto.doc.addResource(path)             -- camelCase alias (Q1-style)
```

Filter writes go into a Lua table on the global state; the
post-filter step extracts them via the same pattern used today for
HTML dependencies and text includes (see `unified_filter.rs:32+`).
`path` is interpreted relative to the document being processed; an
absolute path or out-of-project path is an error caught at extraction
time.

### Engine channel

Q2's `ExecuteResult.supporting_files` is currently a single
`Vec<PathBuf>` documented as "additional files produced (lib/,
resources, etc.)" and is actively populated by the knitr engine
(`engine/knitr/mod.rs:212`) — but it's not yet wired into publish.
Q1 splits this into two concepts:

- `supporting` = a directory of engine-managed assets that gets
  GC'd between renders.
- `resourceFiles` = loose, persistent file paths.

Q2 doesn't have engine-output GC yet, so the split provides no
value today. **Decision: do not introduce a second field.** Keep
`supporting_files` as the engine resource channel; treat every
entry as a published resource. If/when we add GC, we can either
add a flag to `ResourceEntry` or split the field then.

What this issue does add: the `ResourceReportStage` reads
`supporting_files` from `ExecuteResult` and routes those entries
into the report with `ResourceOrigin::Engine`.

### Glob handling

Globs **are** supported in v1 for *author* declarations (Q1 users
expect them) but **not** for engine- or filter-declared paths
(those should be exact). Glob expansion happens during
profile/config construction for the static channel. Use the
`globset` crate (verify availability during implementation).

Negative globs (`!internal/*`) are explicitly skipped — the long
tail of Q1 bugs from positive+negative interaction means this
needs its own design.

### Output layout

For `ResourceScope::Project`: mirror the project-relative path
into `output_dir`, i.e. `data/file.csv` → `_site/data/file.csv`.

For `ResourceScope::Page`: anchored at the document's output dir.
Document at `posts/foo.qmd` declaring `data/x.csv` yields
`_site/posts/data/x.csv`.

Resources outside the project root (`../shared/data.csv`) are an
**error** in v1.

### Manifest for publish

The render manifest does **not** exist yet — `crates/quarto/src/
commands/publish.rs:246-254` walks the output dir today and has a
`// TODO` noting that a `ProjectRenderSummary` extension carrying a
manifest can replace the walk. This issue introduces it.

Emit `.quarto/render-manifest.json` at the end of project render:

```json
{
  "rendered_files": [...],
  "resources": [
    { "source": "data/file.csv", "output": "data/file.csv",
      "scope": "project", "origin": "ProjectMetadata" }
  ]
}
```

`quarto publish` reads the manifest if present; otherwise falls
back to the existing dir-walk (forward compat for sites rendered
before this lands, and a safety net if the manifest is missing for
any reason). The manifest format is owned by this issue and can
evolve; publish treats unknown fields as forward-compatible.

### Future work: misbehaviour detection (not in this issue)

The collector's union-with-profile-snapshot defends against
filters that drop entries, but it's silent about *why* the post-
filter `meta.resources` diverged from the snapshot. A future
stage could compare the snapshot to the post-filter view and:

- emit a structured diagnostic identifying which filter or engine
  ran between the snapshot and the divergence (the pipeline knows
  the order),
- attribute removed/clobbered entries to the responsible
  transformer when possible,
- optionally fail the build under a strict-mode flag so CI can
  catch regressions in third-party filters.

This is intentionally **not** part of this issue. It belongs in a
broader "filter/engine hygiene" effort that would also cover
metadata clobbering more generally (not just `resources:`). File
as a follow-up once the resource channel is in use and we have
real-world examples of filters that misbehave.

### Internal use: auto-discovery (deferred but designed-for)

The intent is that built-in Image-`src` walkers, OJS
`FileAttachment` extraction, etc. eventually use the **same**
filter channel as user code — i.e. they call `add_resource`
internally. This means the channel must be expressive enough to
carry the metadata an internal walker would want (origin kind,
source location for diagnostics). The `ResourceOrigin` enum
includes a commented-out `AutoDiscovery` variant for that future
work.

## Phases

### Phase 0 — Test plan

(TDD: write tests first.)

Static-channel tests are written and confirmed failing now in
`crates/quarto-core/tests/project_resources.rs` (7 tests, all
failing as expected). Engine, Lua, and publish tests are written
alongside the code that supports them in their respective phases.

- [x] End-to-end fixture: project with `_quarto.yml` declaring
      `resources: ["data/*.csv", "extras/notes.txt"]`, plus a
      document with `resources: [include.html]`. Run `quarto
      render`, assert files land in `\_site/` at expected locations.
      *(Implemented as `project_resources_literal_paths_copy_to_output_dir`,
      `project_resources_glob_expansion`,
      `project_resources_single_scalar`,
      `document_resources_copy_anchored_at_doc_output_dir`.)*
- [ ] Engine fixture: a stub engine returning
      `ExecuteResult { supporting_files: vec!["out.csv"] }`.
      Assert the file appears in `_site/`. *(Phase 2.)*
- [ ] Lua filter fixture: a tiny user filter that calls
      `quarto.doc.add_resource("from-filter.txt")`. Assert the file
      appears in `_site/`. *(Phase 3.)*
- [ ] **Filter-as-additive test**: a filter that *removes* an
      entry from `meta.resources`. The author-declared entry must
      still be published (the union with the profile snapshot
      defends it). *(Phase 3.)*
- [ ] **Filter-augments-meta test**: a filter that *adds* an
      entry to `meta.resources` (rather than calling
      `add_resource`). The added entry must be published — we
      re-read `meta.resources` at collector time. *(Phase 3.)*
- [x] Manifest test: render produces
      `.quarto/render-manifest.json` with the expected resources
      array. *(`render_manifest_contains_resources` — failing,
      drives Phase 4.)*
- [ ] Publish test (with a stub publish backend): all three
      origins flow into the publish file list. *(Phase 4.)*
- [x] Negative test: declaring `resources: ["../outside.csv"]`
      yields a clear error, not silent failure.
      *(`project_resources_out_of_project_path_is_error`,
      `document_resources_out_of_project_path_is_error`.)*

### Phase 1 — Static channel (project + document metadata) ✅

- [x] Add `resources: Vec<String>` to `ProjectConfig`
      (project-level), parse from `_quarto.yml` (`project.resources`).
- [x] Bump `DOCUMENT_PROFILE_VERSION` to 3 and add
      `resources: Vec<String>` (raw patterns) to `DocumentProfile`.
      *(Refined from `Vec<ResourceEntry>` in the original plan:
      ResourceEntry is the resolved/expanded form, computed at the
      collector. Storing raw patterns in the profile keeps profile
      extraction pure (no I/O), which the contract requires, and
      defers glob expansion to the orchestrator where the
      filesystem walk happens.)*
- [x] Read document-level `resources:` from frontmatter via
      `DocumentProfile::extract`.
- [x] Glob expansion helper
      (`project_resources::expand_patterns`), plus a
      `looks_like_glob` shortcut for literal paths.
- [x] Out-of-project guard with a friendly error message
      (`ResourceError::OutOfProject`).
- [x] Wire into post-render copy: `collect_static_resources` +
      `copy_resources_to_output_dir` called from
      `ProjectPipeline::run` after `post_render`. Native-only —
      WASM hub-client preview doesn't write to a real output dir.
      *(`RenderedOutput`/`FinalOutput` plumbing deferred — current
      design uses a project-level collector instead of per-doc
      output threading. Will revisit if Phase 2's report needs
      per-doc carriage.)*
- [x] Tests for Phase 0 items 1 (project + doc resources, globs,
      single-scalar) and 8 (out-of-project errors) pass. Tests 4
      (filter-additivity) and 6 (manifest) belong to Phases 2 and 4.

End-to-end verified through the `q2 render` CLI on
`/tmp/q2-resource-e2e`:

```
$ q2 render /tmp/q2-resource-e2e
$ find _site -type f
_site/blob/info.txt        # doc-level resource from posts.qmd
_site/data/one.csv         # project-level glob
_site/data/two.csv         # project-level glob
_site/extras/notes.txt     # project-level literal
_site/index.html
_site/posts.html
_site/site_libs/...        # (existing dedup'd assets)
```

Workspace tests: 8315/8316 pass. The 1 failure is the Phase 4
manifest test that's expected to fail until Phase 4 lands.

### Phase 2 — Engine channel ✅

- [x] Introduce `DocumentResourceReport` and `ReportedResource` in
      `project_resources.rs`.
- [x] Thread the report through the pipeline:
      `StageContext.resource_report` ↔ `RenderContext.resource_report`
      ↔ `RenderToFileResult.resource_report`. `run_pipeline`
      transfers in/out symmetric with `artifacts`.
- [x] Extend `EngineExecutionStage` to push
      `ExecuteResult.supporting_files` (with `std::mem::take`) into
      `ctx.resource_report` tagged `ResourceOrigin::Engine`.
- [x] Add a `Pass2Renderer::extract_resource_report` method (default
      `None` so WASM stays opt-out). Native `RenderToFileRenderer`
      returns the per-doc report.
- [x] Orchestrator drains per-doc reports after Pass-2, resolves
      against the project root via `resolve_reported_resources`, and
      merges with the static-channel list before
      `copy_resources_to_output_dir`.
- [x] Phase 0 item 2 covered by
      `orchestrator_engine_channel::orchestrator_drains_engine_report_and_copies_to_output_dir`
      (uses a `MockRenderer` implementing `Pass2Renderer` to inject
      synthetic engine reports — exercises the entire drain →
      resolve → copy path through the real orchestrator). Plus 4
      unit tests in `project_resources::tests` covering
      `resolve_reported_resources`.

**Design refinement** (from the plan's original sketch): the plan
called for a dedicated `ResourceReportStage` running after
`UserFiltersStage::post`. For engine-only contributions, that stage
has no real work — the engine stage directly populates
`ctx.resource_report`, and the orchestrator drains it after
Pass-2. Phase 3 (Lua filters) is when a separate report-finalizing
stage earns its keep, because that's where the additivity defense
(re-reading `meta.resources` after filters mutate it, then unioning
with the profile snapshot) becomes load-bearing. **Defer the
named stage to Phase 3.**

End-to-end through `q2 render` with a real engine fixture (knitr
or jupyter) is left for Phase 3 / Phase 5 docs verification — the
orchestrator-level mock test plus the engine-stage modification
together cover the wiring without requiring an R/Python
environment in CI.

Workspace tests: 8320/8321 pass. The one failure remains the
Phase-4 manifest test.

### Phase 3 — Lua filter channel ✅

- [x] Added `quarto.doc.add_resource(path)` and `addResource(path)`
      alias in `crates/pampa/src/lua/quarto_doc.rs`. New Lua table
      `_resources`; `extract_resources(&lua)` drains it.
- [x] Plumbed through `pampa::lua::filter::FilterOutput.resources` →
      `pampa::unified_filter::FilterOutput.resources` →
      `UserFiltersStage` (calls
      `ctx.resource_report.add_lua_filter_files`).
- [x] Implemented `ResourceReportStage` (sits after
      `UserFiltersStage::post()`, before `CodeHighlightStage`).
      Reads post-filter `meta.resources`, diffs against the
      profile snapshot from `ProjectIndex`, pushes additions as
      `ResourceOrigin::LuaFilter` contributions. Logs divergence
      from the snapshot at debug level. Inserted into both
      `build_html_pipeline_stages_with_apply_config` and
      `build_wasm_html_pipeline`. Pipeline length tests updated
      from 15→16 (native) and 14→15 (WASM).
- [x] Path validation lives in
      `project_resources::resolve_reported_resources` — a Lua
      filter passing an out-of-project path produces
      `ResourceError::OutOfProject` at orchestrator drain time.
- [ ] User-facing docs page for `quarto.doc.add_resource` —
      *deferred to Phase 5.*
- [x] Phase 0 item 3 (`lua_filter_add_resource_lands_in_output_dir`)
      passes via the real `q2 render` CLI path.

**Tests**:

- 2 unit tests in `crates/quarto-core/src/stage/stages/resource_report.rs`
  cover the addition + removal cases of the additivity defense.
- 3 E2E integration tests in
  `crates/quarto-core/tests/project_resources.rs`:
  `lua_filter_add_resource_lands_in_output_dir`,
  `lua_filter_camel_case_alias_works`,
  `filter_removing_meta_resources_does_not_drop_author_declaration`.

**Discovered limitations** (filed as separate beads issues):

- `bd-uy3z`: pampa's typewise filter dispatch doesn't yet invoke
  `Meta(meta)` / `Pandoc(doc)` callbacks (the names are recognized
  but never called). Blocks the E2E "filter adds via meta.resources
  mutation" test — the unit test in `resource_report.rs` covers
  the logic instead. Fixing this in pampa unlocks both this test
  and broad pandoc filter compatibility.
- `bd-45yw`: Replay engine for deterministic tests. Engine-channel
  E2E (jupyter / knitr) needs either real engine installs or a
  test injection point. A "replay engine" — records a real
  engine's transcript once, replays in pure Rust — would cover
  this cleanly. Particularly important for Jupyter where custom
  kernels are common.

End-to-end verified through `q2 render`:

```
$ cat addres.lua
local registered = false
function Para(p)
  if not registered then
    quarto.doc.add_resource('from-filter.txt')
    registered = true
  end
  return p
end

$ q2 render /tmp/q2-lua-resource
$ ls _site/from-filter.txt   # exists, contents copied
```

Workspace tests: 8325/8326 pass. The one failure is the Phase-4
manifest test.

### Phase 4 — Manifest + publish ✅

- [x] `RenderManifest` (with `version`, `rendered_files`,
      `resources`) and `ManifestResource` types in
      `crates/quarto-core/src/project_resources.rs`.
- [x] Project render emits `.quarto/render-manifest.json` after
      the resource-copy step. Schema is permissive: extra fields
      are ignored, `version` bumps only for breaking changes.
- [x] `quarto publish` reads the manifest if present and slots
      `manifest.resources[].output` into `PublishFiles.files`.
      Falls back to the existing `collect_sidecar_files` dir-walk
      for `_site/site_libs/` and the like (those still come from
      the artifact store, not the manifest). When no manifest is
      present (older renders), pure dir-walk takes over.
- [x] Existing `bd-t3ny` publish path preserved end-to-end —
      manifest reading is additive on top of the dir-walk.
- [x] Phase 0 items 4 (`render_manifest_contains_resources`) and
      5 (`commands::publish::tests::declared_resources_appear_in_publish_files`)
      pass. The latter exercises all three origins (project YAML +
      doc YAML + `quarto.doc.add_resource`) flowing through the
      production `ProjectPublishRenderer::render` into
      `PublishFiles.files`.

**Bug caught + fixed during E2E verification**: the
`ResourceReportStage` was looking up profiles by absolute path
when `ProjectIndex.lookup_by_source` keys on project-relative
paths. The lookup miss made the snapshot always empty, so any
post-filter `meta.resources` entry got reported as a `LuaFilter`
contribution — even when it just matched what the author
declared. Now strip `ctx.project.dir` from `doc.path` before the
lookup. Test fixture in `resource_report.rs` updated to mirror
production (profiles store project-relative `source_path`).

End-to-end manifest example verified through `q2 render`:

```json
{
  "version": 1,
  "rendered_files": ["index.html"],
  "resources": [
    {
      "source": "extras/notes.txt",
      "output": "extras/notes.txt",
      "origin": { "kind": "ProjectMetadata" }
    },
    {
      "source": "blob/info.txt",
      "output": "blob/info.txt",
      "origin": {
        "kind": "DocumentMetadata",
        "source": "/path/to/index.qmd"
      }
    }
  ]
}
```

Workspace tests: **8327/8327 pass** — first fully-clean run.

### Phase 5 — Docs ✅

- [x] User-facing doc page at `docs/projects/resources.qmd`
      covering all three declaration channels (project YAML, doc
      YAML, `quarto.doc.add_resource`), output layout, the
      irrevocable-author-declaration rule, the out-of-project
      error, and how `quarto publish` consumes the manifest.
      Linked from the navbar in `docs/_quarto.yml`.
- [x] Lua filter author guidance lives in the same page (no
      separate filter-authors doc exists yet — folded in there
      with a worked Image example).
- [x] Engine-author note via the doc comment on
      `ExecuteResult.supporting_files` in
      `crates/quarto-core/src/engine/context.rs`. Explains that
      paths land as published resources, where they're drained,
      and how relative paths anchor.
- [x] Docs site renders cleanly via `q2 render docs` — the
      navbar entry resolves and the TOC is generated.

## Resolved decisions (2026-05-03 review)

1. **Lua filter API.** Ship `quarto.doc.add_resource(path)` (with
   `addResource` alias) — `quarto.doc` already exists, this fits
   the existing surface.
2. **Mutability.** DocumentProfile stays read-only. Author
   declarations are *irrevocable* (additive-only by API design).
   Engine + filter contributions are accumulated in a separate
   `DocumentResourceReport` collected by `ResourceReportStage`
   late in the pipeline and merged with the static list at the
   end. No mutable channel on the profile or `StageContext`.
3. **Engine channel naming.** No new field; reuse
   `ExecuteResult.supporting_files`. Q1's `supporting` /
   `resourceFiles` split exists to support engine-output GC, which
   Q2 doesn't have yet — defer the split.
4. **Manifest.** Manifest does not exist today (publish walks the
   output dir). This issue introduces it. Manifest is the canonical
   input to publish; dir-walk is the fallback.
5. **Out-of-project resources.** Error in v1.
6. **Negative globs.** Skip in v1; defer to a dedicated glob
   design.
7. **Auto-discovery.** Out of scope as a feature, but the channel
   is designed so that built-in walkers (Image `src`, OJS
   `FileAttachment`, includes) can use the same `add_resource`
   API internally as a follow-up. `ResourceOrigin` reserves an
   `AutoDiscovery` variant.
8. **Format-conditional.** Format-agnostic. Formats that don't use
   resources simply ignore them.

## Risks / Tradeoffs

- **Profile version bump.** Phase 1 bumps `DOCUMENT_PROFILE_VERSION`
  to 3, invalidating any Phase-8 incremental cache. Acceptable —
  we're still pre-1.0 and the cache is rebuilt on first render.
- **Engine channel reuse.** Reusing `supporting_files` for both
  "engine intermediates" and "publish me" means we can't later
  distinguish the two without an API break or a new field. If
  we add engine-output GC, we'll need to introduce that
  distinction then.
- **`bd-t3ny` is closed.** The publish epic is already complete
  and uses the dir-walk approach. Phase 4 of this plan changes
  that contract; we'll need to update the publish code path to
  prefer the manifest. Low risk because the dir-walk fallback
  preserves existing behaviour.
