# Phase 1 — Project orchestration (`ProjectType` trait + two-pass driver)

**Date:** 2026-04-23
**Beads:** `bd-w5os` (phase); parent `bd-0tr6` (website epic). Blocked-by
`bd-f3jc` (Phase 0) — closed.
**Parent plan:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Previous phase:** `claude-notes/plans/2026-04-23-websites-phase-0.md`
**Contract this phase consumes:**
`claude-notes/designs/document-profile-contract.md`
**Status:** Design approved (2026-04-23). Implementation in a
subsequent session.

## Goal of this phase

Introduce the infrastructure that makes multi-file project rendering
work:

1. A **`ProjectType` trait** with `pre_render` / per-file contribution
   hooks / `post_render` — the narrow Rust equivalent of Q1's
   `ProjectType` interface.
2. A **`DefaultProjectType`** no-op implementation that is used for
   every render today (single-file and loose-directory), so Phase 1
   ships a pure refactor of the CLI path.
3. A **`ProjectIndex`** type that holds `Vec<DocumentProfile>` plus
   lookup helpers, seeded by Pass 1 and read by Pass 2.
4. A **`ProjectPipeline`** driver that: (a) expands the file list,
   (b) runs Pass 1 for each file to produce a `DocumentAtProfile`,
   (c) builds the `ProjectIndex`, (d) calls `pre_render` /
   `post_render` hooks, and (e) runs Pass 2 to finish each file.
5. **CLI wiring** so `quarto render` invokes the driver rather than
   the current inline `for` loop in `crates/quarto/src/commands/render.rs`.
6. **File-list expansion** for multi-file projects (populate the
   today-empty `project.files` when `_quarto.yml` exists), respecting
   `project.render` globs when present and a sensible default
   otherwise.
7. A **rename**: the existing enum
   `quarto_core::project::ProjectType` → `ProjectKind`, freeing the
   `ProjectType` name for the new trait. Minimal-blast-radius — only
   one external caller (the CLI info-log line).

**No user-visible behavior change for existing single-file renders.**
The CLI must produce byte-identical HTML on the 3 fixtures used at
the end of Phase 0. Multi-file project rendering starts producing
output *for the first time*, but it won't do anything website-shaped
yet — Phase 2+ adds sidebars / navbars / cross-doc links.

This phase does **not** implement:

- Any website-specific behavior (sidebars, navbars, site_libs, etc.)
  — those are Phases 2–7.
- The `WebsiteProjectType` implementation beyond a registered
  placeholder type-tag.
- Book or Manuscript types.
- Parallel rendering.
- Incremental rebuilds / the on-disk profile cache (Phase 8).
- Hub-client orchestration (Phase 9).
- `freeze`.

## Reference material

- **Parent epic plan** §"Phase 1 — Project orchestration" and
  §"Architecture sketch" / §"`ProjectType` trait".
- **Phase 0 plan** (for naming / shape / crate placement context)
  and its §"Project root invariant" (must continue to hold).
- `claude-notes/designs/document-profile-contract.md` — how to read
  profiles.
- `crates/quarto/src/commands/render.rs` — current CLI entry point
  and per-file loop.
- `crates/quarto-core/src/render_to_file.rs` — the single-file
  render workhorse (`render_document_to_file`, `render_to_file`).
- `crates/quarto-core/src/project.rs` — `ProjectContext`,
  `ProjectConfig`, `DocumentInfo`, and the existing `ProjectType`
  enum (to be renamed).
- `crates/quarto-core/src/pipeline.rs` —
  `build_html_pipeline_stages`, `run_pipeline`, `render_qmd_to_html`.
- Q1 reference: `external-sources/quarto-cli/src/project/types/types.ts`
  (`ProjectType` interface) and
  `external-sources/quarto-cli/src/command/render/project.ts`
  (`renderProject` flow — lines 310–862).

## Key decisions (already settled by the epic)

From `claude-notes/plans/2026-04-23-website-project-epic.md`
§"Key decisions":

- Project orchestration uses a trait with pre-render / per-file
  contributions / post-render hooks.
- Profiles are read-only to user filters.
- **One new user-filter position** after the snapshot checkpoint,
  where filters can read `Vec<DocumentProfile>` but cannot mutate the
  snapshot. *For Phase 1, we only add the machinery; the user-filter
  position itself is a future phase that ties into the Lua API.*
- Single-document renders continue to work as a degenerate "default"
  project.

## Naming decisions (confirmed 2026-04-23)

**Rename first.** The existing enum
`quarto_core::project::ProjectType` conflicts with the new trait.
Rename the enum to `ProjectKind` (values: `Default`, `Website`,
`Book`, `Manuscript`) and use `ProjectType` for the trait.

| Concept | Proposed name |
|---|---|
| The kind tag (Default/Website/Book/Manuscript) | `ProjectKind` (was `ProjectType`) |
| The orchestration trait | `ProjectType` |
| No-op implementation used for single-file and loose-directory | `DefaultProjectType` |
| Tag-only placeholder for websites | `WebsiteProjectType` (unimplemented for Phase 1) |
| Cross-doc index | `ProjectIndex` |
| The driver | `ProjectPipeline` |
| Pass 1 output per file | `DocumentAtProfile` (Phase 0) |
| Module path for the trait + driver | `quarto_core::project::orchestrator` |
| Module path for `ProjectIndex` | `quarto_core::project::index` |

**Crate placement.** Still `quarto-core` for Phase 1 — defer the
`quarto-project` split until an actual consumer (e.g. a shared
hub-client side-crate) demands it.

**Trait method names:** `pre_render` / `post_render`, mirroring Q1.

**Driver name:** `ProjectPipeline` (parallel to `Pipeline`).

**Rename blast radius.** `git grep -n "ProjectType::"` shows the
enum is referenced only inside `project.rs` and one CLI info-log
call (`crates/quarto/src/commands/render.rs:93`:
`project.project_type().as_str()`). Public API change: the accessor
`ProjectContext::project_type()` becomes `project_kind()`. I'll do
this as **the first commit in Phase 1** so every subsequent commit
sits on a clean naming base.

## Parallelism readiness (note — added 2026-04-23)

The user plans to parallelize website rendering in a soon-ish
follow-up session. Recording the decision surface here so Phase 1's
choices don't foreclose it.

**Today's model:** stages use `async_trait(?Send)`. This is
incompatible with tokio's multithreaded work-stealing scheduler out
of the box — a tokio-based parallelism path would require migrating
every stage to `Send` futures, a workspace-wide refactor.

**The cheaper path (recommended) is rayon + per-worker
`pollster::block_on`.** Each rayon thread drives its own file's
async pipeline on a single-threaded executor local to that thread.
Stages stay `?Send`; nothing in the current model changes. Concretely:

- Pass 1: `project.files.par_iter().map(|f| pollster::block_on(run_head(f))).collect::<Vec<_>>()`.
  Each thread builds one `StageContext`, runs the head pipeline,
  produces one `DocumentProfile`. No shared mutable state.
- Build `ProjectIndex` on the main thread from the collected
  profiles.
- Pass 2: `project.files.par_iter().map(|f| pollster::block_on(render_document_to_file(..., index_arc)))`.
  Each thread reads the shared `Arc<ProjectIndex>` and writes its
  own output file.

**Phase 1 decisions relevant to this:**

- `StageContext` is already owned per-file and has no global state
  contention — compatible.
- `ProjectIndex` is wrapped in `Arc<_>` on `StageContext` — shareable
  across threads as-is.
- `SystemRuntime` trait objects are `Arc<dyn SystemRuntime>`; we'll
  need to confirm `NativeRuntime: Send + Sync`. Today's single-doc
  tests and the hub-client WASM shim both hold an `Arc`, so this is
  almost certainly already the case — we'll add a `where T: Send +
  Sync` compile-time check on the orchestrator when parallelism
  lands.

**What Phase 1 will not do:** introduce rayon. The driver ships
sequentially in v1. A follow-up bd issue (see §Follow-up beads)
tracks the conversion.

**What Phase 1 will do to help:** avoid patterns that the rayon
conversion would have to unwind — in particular, the driver will
not thread any `&mut` through the per-file loops except via the
`ProjectContext` it already owns, and `pre_render` is called once
before the parallel section (exactly the Q1 pattern).

## Architecture sketch

### The invariant: all renders are project renders

Following Phase 0's "no project-root branch" rule, Phase 1 takes a
parallel step: **all renders go through `ProjectPipeline`.** A
single-file or loose-directory render is just a `DefaultProjectType`
project with one file in `project.files`. There is no separate
"single-file path" and "project path" anymore — the CLI discovers a
project, chooses a `ProjectType`, hands the project to the driver.

This is the inversion of Q1's synthetic-project pattern (where
`--output-dir` on a bare file creates a throw-away project). In Q2 we
never synthesize — we just *always* have a project, because
`ProjectContext::discover()` already always returns one.

### The two passes

Today, the pipeline is one-shot per file:

```
LoadedSource → Parse → Merge → [Profile] → Unwrap → Sugar → … → ApplyTemplate
```

Phase 1 wraps each file's render in two passes around that same
pipeline:

```
Pass 1 (per file):
    LoadedSource → Parse → Merge → [Profile]   ← STOP, collect DocumentAtProfile

Build ProjectIndex from all Pass 1 results.

ProjectType::pre_render(&mut ProjectContext, &ProjectIndex)
    (Phase 1: no-op for DefaultProjectType; placeholder for Website.)

Pass 2 (per file):
    DocumentAtProfile → Unwrap → Sugar → Engine → … → ApplyTemplate → FinalOutput

ProjectType::post_render(&ProjectContext, &ProjectIndex, &[RenderToFileResult])
    (Phase 1: no-op for DefaultProjectType.)
```

Pass 1 is cheap: no engine execution, no user filters, no AST
transforms. Pass 2 resumes from the cloned `AtProfile` (or, for
simplicity in v1, re-runs Pass 1 in-process — see §"Pass 2
resumption strategy" below).

### `ProjectType` trait shape

```rust
// crates/quarto-core/src/project/orchestrator.rs

use async_trait::async_trait;

use crate::project::ProjectContext;
use crate::project::index::ProjectIndex;
use crate::render_to_file::RenderToFileResult;

/// Trait implemented by each project kind (default, website, book, …).
///
/// Phase 1 ships only `DefaultProjectType` and a placeholder
/// `WebsiteProjectType` with no-op hooks. Phase 2+ fills in the
/// website hooks.
#[async_trait(?Send)]
pub trait ProjectType {
    /// The tag used to pick this implementation from a parsed
    /// `_quarto.yml`'s `project.type`.
    fn kind(&self) -> crate::project::ProjectKind;

    /// Called once per project, after `ProjectContext::discover` and
    /// before any per-file rendering. Default implementation is a
    /// no-op. Websites will use this (eventually) to, e.g., resolve
    /// sidebar config.
    async fn pre_render(
        &self,
        _project: &mut ProjectContext,
        _index: &ProjectIndex,
    ) -> crate::Result<()> {
        Ok(())
    }

    /// Called once per project, after every file has rendered.
    /// Default implementation is a no-op. Websites will use this to
    /// emit `sitemap.xml`, copy favicon, etc. (Phase 7).
    async fn post_render(
        &self,
        _project: &ProjectContext,
        _index: &ProjectIndex,
        _outputs: &[RenderToFileResult],
    ) -> crate::Result<()> {
        Ok(())
    }
}
```

The trait is intentionally minimal for Phase 1. Q1's
`ProjectType` interface has ~25 hooks; we'll grow into them only as
the phases actually need them. Growing a trait is easy; unwinding a
premature design isn't.

Why `async`? Two reasons. (a) `render_qmd_to_html` and its stages are
already `async_trait(?Send)` — matching preserves one executor model.
(b) Future website hooks (sitemap writing, favicon copying, fetch of
remote resources) want async I/O. The no-op default implementations
mean today's code pays zero cost.

### `ProjectIndex` shape

```rust
// crates/quarto-core/src/project/index.rs

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::document_profile::DocumentProfile;

#[derive(Debug, Clone, Default)]
pub struct ProjectIndex {
    profiles: Vec<DocumentProfile>,
    by_source_path: HashMap<PathBuf, usize>,
    by_output_href: HashMap<String, usize>,
}

impl ProjectIndex {
    pub fn new(profiles: Vec<DocumentProfile>) -> Self { /* … */ }
    pub fn profiles(&self) -> &[DocumentProfile] { &self.profiles }
    pub fn lookup_by_source(&self, path: &Path) -> Option<&DocumentProfile> { /* … */ }
    pub fn lookup_by_href(&self, href: &str) -> Option<&DocumentProfile> { /* … */ }
}
```

- `HashMap` is fine here — the struct doesn't derive `Serialize`
  (profiles do; the index is rebuilt in memory each run). Per
  `claude-notes/instructions/coding.md` §"HashMap and Determinism",
  non-serialized, lookup-only maps may use `HashMap`.
- Ordering-sensitive output (e.g. "all pages in sidebar auto order")
  reads `profiles()`, which preserves insertion order.

### `ProjectPipeline` driver

```rust
// crates/quarto-core/src/project/orchestrator.rs

use std::sync::Arc;

use quarto_system_runtime::SystemRuntime;

use crate::format::Format;
use crate::project::ProjectContext;
use crate::project::index::ProjectIndex;
use crate::render_to_file::{RenderToFileOptions, RenderToFileResult};

/// Orchestrate a two-pass render of every file in a project.
pub struct ProjectPipeline<'a> {
    project: &'a mut ProjectContext,
    project_type: Box<dyn ProjectType>,
    format: Format,
    options: &'a RenderToFileOptions,
    runtime: Arc<dyn SystemRuntime>,
}

impl<'a> ProjectPipeline<'a> {
    pub fn new(
        project: &'a mut ProjectContext,
        project_type: Box<dyn ProjectType>,
        format: Format,
        options: &'a RenderToFileOptions,
        runtime: Arc<dyn SystemRuntime>,
    ) -> Self { /* … */ }

    /// Run the full two-pass flow. Returns one `RenderToFileResult`
    /// per rendered file, in `project.files` order.
    pub async fn run(&mut self) -> crate::Result<Vec<RenderToFileResult>> {
        // 1. Pass 1: profile every file. (Re-using the head pipeline from Phase 0.)
        let profiles = self.pass_one().await?;
        let index = ProjectIndex::new(profiles);

        // 2. pre_render hook.
        self.project_type.pre_render(self.project, &index).await?;

        // 3. Pass 2: render every file with the index in scope.
        let outputs = self.pass_two(&index).await?;

        // 4. post_render hook.
        self.project_type
            .post_render(self.project, &index, &outputs)
            .await?;

        Ok(outputs)
    }

    async fn pass_one(&self) -> crate::Result<Vec<DocumentProfile>> { /* … */ }
    async fn pass_two(&mut self, index: &ProjectIndex)
        -> crate::Result<Vec<RenderToFileResult>> { /* … */ }
}

/// Factory: pick the `ProjectType` implementation for a project.
pub fn project_type_for(project: &ProjectContext) -> Box<dyn ProjectType> {
    match project.project_kind() {
        ProjectKind::Default => Box::new(DefaultProjectType),
        ProjectKind::Website => Box::new(WebsiteProjectType),
        ProjectKind::Book | ProjectKind::Manuscript => Box::new(DefaultProjectType),
    }
}
```

### Pass 2 resumption strategy

Phase 0's clone-and-resume test shows we *can* pause after Profile,
clone, and resume cleanly. But threading a `PipelineData::AtProfile`
through `render_document_to_file` today is a sizeable refactor of
that function's internals.

**Phase 1 takes the simpler path:** Pass 2 calls
`render_document_to_file` as it exists today. This means the head
pipeline re-runs for each file — exactly the redundant work Phase 0's
checkpoint was designed to avoid. We accept the redundancy in v1 for
two reasons:

1. It keeps Phase 1's diff scoped to *orchestration*. Rewiring
   `render_document_to_file` to accept a pre-built `AtProfile` is its
   own change with its own test surface.
2. The `StageContext` carries mutable per-file state (artifacts,
   registries, diagnostics). Threading a pre-built `AtProfile` needs
   a careful decision about what *else* to carry forward from Pass 1
   — and we should make that call once we have a concrete consumer
   (Phase 2 sidebar generate).

**What Phase 1 *does* need from the profile work:** the fact that the
profile is computed once per file in Pass 1 and kept in
`ProjectIndex`, so that website hooks can read across files without
re-running pipelines. That alone is enough to unblock Phase 2.

A follow-up task (file as `bd-<new>` during Phase 1) tracks converting
Pass 2 to resume from a cached `AtProfile` when the clone-and-resume
infrastructure is plumbed through `render_document_to_file`.

### Injecting `ProjectIndex` into per-file rendering

Pass-2 transforms and future user filters read
`&[DocumentProfile]` by reading an `Arc<ProjectIndex>` off
`StageContext`. Add:

```rust
// crates/quarto-core/src/stage/context.rs
pub struct StageContext {
    // … existing fields …

    /// Project-wide index of profiles from Pass 1. `None` for the
    /// short-lived head-only runs in Pass 1 itself. Set by
    /// `ProjectPipeline::pass_two` before each file's tail run.
    pub project_index: Option<Arc<ProjectIndex>>,
}
```

`Arc` so every per-file `StageContext` shares one underlying index
without copying. Phase 1 doesn't *read* `project_index` anywhere —
Phase 2+ does — but we put the slot in now so Phase 2 is a drop-in.

### File-list expansion

Today `ProjectContext::discover` leaves `files = Vec::new()` when
`_quarto.yml` is present (`project.rs:422`). Phase 1 fills it:

1. If `config.render_patterns` is non-empty, treat each entry as a
   glob relative to `project.dir` and expand. Keep only matches with
   a `.qmd` extension.
2. Otherwise, recursively walk `project.dir`, collecting files with
   extension `.qmd` only.
3. **Always exclude:**
   - Files under `project.output_dir` (default `_site/` for
     websites).
   - Files under `.quarto/`, `.git/`, `node_modules/`.
   - Files whose path has a component starting with `_` (Q1
     convention: `_metadata.yml`, `_includes/`, `_*` partials).
   - Files whose path has a component starting with `.` (hidden).
   - Files whose *name* starts with `README` (case-insensitive) —
     mirrors Q1's behavior; these are GitHub-facing, not rendered
     pages.
   - The project config file itself.
4. Respect extension-based type detection
   (`SourceType::from_extension`) — though for Phase 1 the only
   accepted extension is `.qmd`.

**Scope choice (user directive, 2026-04-23): Phase 1 discovers only
`.qmd`.** Support for `.md`, `.ipynb`, `.rmd`, etc. is deferred — the
decision about which of those are "renderable documents" vs "source
artifacts" is a separate conversation. This is a conservative choice
that keeps Phase 1 tightly scoped and lets Phase 2's sidebar work
operate on a single, well-understood input shape.

The walker lives in a new module `crates/quarto-core/src/project/discovery.rs`
with focused unit tests.

**Not yet** respecting (each will be its own follow-up bd issue at
close-out — see §Follow-up beads):
- Non-`.qmd` input extensions.
- `.quartoignore`.
- `resources` key.
- `profile.render` / conditional render lists.

### Binary integration

In `crates/quarto/src/commands/render.rs`:

```rust
// Before:
for doc_info in &project.files {
    let result = render_document_to_file(...)?;
    // report diagnostics
}

// After:
let format = resolve_format(format_str)?;
let runtime_arc: Arc<dyn SystemRuntime> = /* same as today */;

let project_type = quarto_core::project_type_for(&project);
let mut pipeline = ProjectPipeline::new(
    &mut project,
    project_type,
    format,
    &options,
    runtime_arc,
);

let results = pollster::block_on(pipeline.run())?;
for result in &results {
    // Same diagnostic reporting as today.
}
```

`pollster::block_on` wraps the async driver for the native sync CLI,
matching the pattern already used for `render_qmd_to_html`.

### Error handling

One file's render failure should not by default abort the rest of
the project. Phase 1 adopts a middle-ground rule:

- Pass 1 failures on a single file → log diagnostics, *exclude* that
  file from the index, continue. (A file that won't parse still
  shouldn't stop sibling profiles from being built.)
- Pass 2 failures on a single file → log diagnostics, collect an
  error for that file, continue. Exit non-zero at the end if any
  file failed.
- `pre_render` / `post_render` failures → abort the whole project
  render. Those hooks are project-wide; their failure means the
  project is broken.

This is conservative and matches Q1's behavior. `--fail-fast` / other
strictness modes are follow-ups.

## Tests

Per CLAUDE.md §TDD: tests first, verify they fail, then implement.

### Unit tests

In `crates/quarto-core/src/project/index.rs`:

1. **`index_round_trips_profiles`** — construct with 3 profiles,
   verify `profiles()` order is preserved, `lookup_by_source` and
   `lookup_by_href` return the right ones.
2. **`index_lookup_miss_returns_none`** — unknown keys return `None`.

In `crates/quarto-core/src/project/orchestrator.rs`:

3. **`default_project_type_hooks_are_no_ops`** — build a
   `DefaultProjectType`, call `pre_render` and `post_render` on a
   minimal context; both return `Ok(())` without mutating state.

4. **`project_kind_rename_regression`** — call `ProjectKind::Default`
   / `Website` / `Book` / `Manuscript` via both `as_str()` and
   `TryFrom<&str>`; make sure the rename didn't break the string
   mapping (the stringly-typed round-trip matters for `_quarto.yml`
   parsing).

### Discovery tests

In `crates/quarto-core/src/project/discovery.rs`:

5. **`discovery_walks_directory`** — construct a temp dir with
   `a.qmd`, `sub/b.qmd`, `_partial.qmd`, `.hidden.qmd`, `README.md`,
   `README.qmd`, `notes.md`, `notebook.ipynb`; assert only `a.qmd`
   and `sub/b.qmd` are returned. Everything else is excluded: the
   underscore and dot paths by component rule, the READMEs by the
   README-name rule, and `notes.md` / `notebook.ipynb` because
   Phase 1 is `.qmd`-only.
6. **`discovery_honors_render_patterns`** — with
   `render_patterns = ["index.qmd", "docs/**/*.qmd"]`, only those
   match.
7. **`discovery_excludes_output_dir`** — a file under `_site/` is
   never returned, even if it matches a pattern.
8. **`discovery_excludes_quarto_scratch`** — files under `.quarto/`
   are never returned.
9. **`discovery_unicode_and_spaces`** — a file with non-ASCII
   characters and spaces is discovered correctly.

### Pipeline / integration tests

In `crates/quarto-core/tests/project_pipeline.rs` (new):

10. **`single_file_goes_through_default_project_type`** — render a
    single `.qmd` with no `_quarto.yml`. Verify the output is
    byte-identical to a pre-Phase-1 reference rendering, and that
    the `ProjectIndex` given to Pass 2 contains exactly one entry
    with the expected title and source path.
11. **`two_file_project_builds_index_of_both`** — a project dir with
    `_quarto.yml` and two qmds. Both render; the `ProjectIndex`
    contains both profiles; pre/post hooks are called once each.
12. **`pre_render_failure_aborts_project`** — use a `ProjectType`
    whose `pre_render` returns `Err`; assert the driver propagates
    the error and no Pass-2 rendering happened.
13. **`per_file_render_failure_continues_others`** — a project with
    one syntactically valid and one broken qmd; driver returns
    non-zero overall but the valid file still produces output.
14. **`project_index_passes_through_to_stage_context`** — a test
    `ProjectType` whose `post_render` inspects
    `StageContext.project_index` (via a side channel or by looking
    at `outputs`) confirms the index was non-None during Pass 2.

### CLI end-to-end tests

In `crates/quarto/tests/smoke-all/` — add a small website fixture
(see §End-to-end below) and assert it renders into `_site/`.

15. **`cli_renders_two_file_website_into_site_dir`** — project dir
    with two qmds and `_quarto.yml: project.type: website`. Run
    `cargo run --bin q2 -- render <dir>`. Assert both files produce
    `_site/<stem>.html`. No assertion on content *yet* beyond valid
    HTML (sidebar / navbar is Phase 2+).

### Snapshot regression

16. Run `cargo nextest run --workspace` before and after. Any
    snapshot change is a red flag — Phase 1 is orchestration, not
    per-file rendering. Flag any diff per CLAUDE.md §"Snapshot Test
    Changes".

### End-to-end CLI verification

Per CLAUDE.md §"End-to-end verification before declaring success":

- **Single-file path:** same 3 fixtures from Phase 0 close-out,
  assert MD5 of rendered HTML matches the Phase-0-close-out values
  (`65d0bf7f…`, `d95941f7…`, `88f0a5d8…`). This proves the refactor
  doesn't regress single-file rendering.
- **Multi-file path (new):** a 2-page project with `_quarto.yml`:
  `project.type: default`. Running `q2 render <dir>` produces both
  `*.html` outputs. Inspect one to confirm valid HTML.
- **Multi-file path, website kind:** same fixture with
  `project.type: website`. Output goes into `_site/` (Phase 1 just
  uses the default output-dir math — website-specific
  `_site/site_libs/` is Phase 5). Confirm the two HTML outputs
  exist where expected.

## Work items (checklist)

### Preparation
- [x] Re-read `claude-notes/instructions/testing.md`, `coding.md`,
      and `review.md`.
- [x] Commit directly on `feature/websites` — no worktree, no
      sub-branch (per user preference 2026-04-23).

### Rename pre-commit
- [x] Rename enum `ProjectType` → `ProjectKind` across
      `crates/quarto-core/src/project.rs` and
      `crates/quarto/src/commands/render.rs`; update
      `ProjectContext::project_type()` →
      `project_kind()`. Run full workspace tests. Commit this
      rename as its own atomic commit. (commit `5bd92a4a`).

### TDD phase — tests first, all failing
- [x] Add skeleton `ProjectIndex` type in `project/index.rs`.
- [x] Add skeleton `ProjectType` trait + `DefaultProjectType` in
      `project/orchestrator.rs`.
- [x] Add skeleton `ProjectPipeline::{new, run}`.
- [x] Write unit tests 1–4, discovery tests 5–9, integration tests
      10–14.

### Implementation
- [x] Implement `ProjectIndex::{new, lookup_*, profiles}`.
- [x] Implement `ProjectPipeline::pass_one` — build the head stage
      list (up through `DocumentProfileStage`), run per file via
      `run_pipeline`, extract `DocumentProfile` from each
      `AtProfile` output, collect into `Vec`.
- [x] Implement `ProjectPipeline::pass_two` — call
      `render_document_to_file` per file with a freshly-built
      `StageContext` whose `project_index` is set.
      `render_document_to_file` gained an `Option<Arc<ProjectIndex>>`
      parameter; `RenderContext` gained a matching slot; `run_pipeline`
      transfers it into `StageContext`.
- [x] Implement `ProjectPipeline::run`: pass-1 → hook → pass-2 →
      hook, with the error-handling rules. Added one refinement
      not in the original plan: a file that fails Pass 1 is skipped
      in Pass 2 (it would otherwise produce a duplicate error).
- [x] Implement `DefaultProjectType` no-op and
      `WebsiteProjectType` no-op placeholder. Wire
      `project_type_for()` factory.
- [x] Add `project_index: Option<Arc<ProjectIndex>>` to
      `StageContext`; default `None`.
- [x] Implement file-list expansion in `project/discovery.rs`.
      `ProjectContext::discover` now populates `files` via
      `discover_project_files` for multi-file projects.
- [x] Confirm tests 1–14 pass. All 7674 workspace tests pass.

### CLI wiring
- [x] Replace the `for doc_info in &project.files` loop in
      `crates/quarto/src/commands/render.rs` with a
      `ProjectPipeline` invocation.
- [x] Phase-0 regression: native smoke + workspace test suite pass.
      (Phase-0 ad-hoc MD5 fixtures were not checked into the repo,
      so the 1055 `smoke-all` tests and the pre-existing
      `render_integration`/`navigation_e2e` suites serve as the
      regression gate; no snapshots drifted.)

### Verification and close-out
- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` — all green, no snapshot
      diffs (7674 tests passed, 195 skipped).
- [x] `cargo xtask lint` passes (613 files checked).
- [x] `cargo xtask verify --skip-hub-tests --skip-rust-tests`
      passes end-to-end: Rust build, hub-client build (including
      WASM), trace-viewer tests. Full `cargo xtask verify` was not
      re-run because the rust-tests / hub-client-tests phases were
      already validated above by the direct workspace runs.
- [x] End-to-end CLI runs per §"End-to-end CLI verification":
      - Single-file: `q2 render /tmp/q2-phase1-test/simple.qmd`
        emits `simple.html` beside the input with valid HTML.
      - Multi-file website: `_quarto.yml` with
        `project.type: website, output-dir: _site` + three qmds
        (including `docs/api.qmd`) renders into
        `_site/index.html`, `_site/about.html`, and
        `_site/docs/api.html`, each with its own
        `{stem}_files/styles.css` sibling. Output inspected —
        titles and body match.
- [ ] File follow-up beads issues for: (a) Pass-2 resumption from
      `AtProfile`, (b) `.quartoignore` support, (c) `project.resources`
      support, (d) conditional render lists / profiles, (e) non-`.qmd`
      input extensions, (f) parallel per-file rendering.
- [ ] `br close bd-w5os --reason …`.
- [ ] `br sync --flush-only && git add .beads/ && git commit`.
- [ ] Ask user permission before pushing.

## Risks and mitigations

- **Risk:** the single-file render path regresses because everything
  now goes through `ProjectPipeline`. *Mitigation:* the first commit
  in Phase 1 is the pure rename; subsequent commits are additive. The
  Phase-0 fixture-MD5 check is an explicit acceptance gate (step 5 in
  §Work items / CLI wiring).
- **Risk:** multi-file discovery silently picks up the wrong files
  (e.g. READMEs, partials, stale outputs under `_site/`). *Mitigation:*
  discovery unit tests 5–9 codify the exclusion rules up front.
  Additions to the rules become tests first.
- **Risk:** `pre_render` / `post_render` hooks in Phase 2+ want more
  or different arguments than Phase 1 provides. *Mitigation:* the
  trait starts minimal; trait signatures grow when consumers need them.
  Sub-plans for Phase 2+ will propose the additions.
- **Risk:** Pass 2 re-runs the head pipeline, making project renders
  2× slower than they need to be. *Mitigation:* v1 accepts this
  overhead; a follow-up bd issue converts Pass 2 to resume from
  cached `AtProfile`. Include a small benchmark in `perf-harness` to
  track the slowdown and validate the eventual improvement.
- **Risk:** the async boundary in `ProjectType` infects synchronous
  CLI code. *Mitigation:* the CLI already wraps async pipelines with
  `pollster::block_on`; `ProjectPipeline::run` stays async. No new
  pattern is introduced.
- **Risk:** `--output-dir` on a bare file (Q1's synthetic-project
  case) doesn't behave correctly. *Mitigation:* Phase 0's "no
  project-root branch" invariant means a bare file is already a
  single-file project rooted at its directory; `--output-dir` just
  sets `project.output_dir`. No synthesis needed. A smoke test covers
  this.

## Explicit non-goals for this phase

- No `WebsiteProjectType` behavior beyond the placeholder. Sidebars,
  navbars, cross-doc links, sitemap, favicon — all Phase 2+.
- No book or manuscript types.
- No parallel rendering.
- No incremental rebuild / disk cache (Phase 8).
- No hub-client orchestration (Phase 9).
- No freeze.
- No changes to the render pipeline stages themselves (the stages
  shipped in Phase 0 stay as-is).
- No user-filter-reads-profiles plumbing — that's when a concrete Lua
  API consumer lands.

## Decisions log (user confirmed 2026-04-23)

All open questions from the initial draft are resolved. Recording
them here for the audit trail.

**Naming**
- Rename `enum ProjectType` → `enum ProjectKind`. Trait takes the
  `ProjectType` name.
- Trait method names: `pre_render` / `post_render` (Q1 continuity).
- Driver name: `ProjectPipeline`.

**Shape**
- Async `ProjectType` methods (aligns with existing stage trait,
  allows future I/O hooks to be async).
- `ProjectPipeline::run` returns `Vec<Result<RenderToFileResult>>`
  so per-file failures are explicit to callers (hub-client and
  future richer integrations want this; the CLI wrapper collapses
  to any-failure → non-zero exit).

**Pass-2 resumption**
- v1 re-runs the head pipeline in Pass 2. Follow-up bd issue
  tracks the `AtProfile` resumption optimization.

**File discovery rules**
- `README*` files (case-insensitive name match) excluded — Q1 rule.
- `_quarto-*.yml` profile files excluded.
- **Phase 1 discovers only `.qmd`.** Non-`.qmd` extensions are
  deferred — follow-up bd will decide which are "renderable
  documents" vs "source artifacts".
- Empty render result (patterns and walk both find zero `.qmd`
  files) → warning diagnostic, continue with zero files; the CLI
  shell decides whether that's an error.

**Error handling**
- Pass 1 failures: drop that file from the index, continue.
- Pass 2 failures: continue other files, exit non-zero at the end.
- Hook failures: abort the whole project render.
- No `--fail-fast` in Phase 1.

**Binary integration**
- `pollster::block_on(pipeline.run())` at the CLI boundary;
  the whole `render::execute` stays sync (matches the existing
  `render_qmd_to_html` pattern).
- The async-model / `?Send` choice is revisited when parallelism
  lands — see §"Parallelism readiness". Rayon + per-worker
  `pollster` is the recommended path and does not require any
  Phase-1 change.

**Follow-up beads** (to be filed at close-out)
- Pass-2 resumption from cached `AtProfile` (optimization).
- `.quartoignore` support.
- `project.resources` support.
- Conditional render lists / `_quarto-*.yml` profiles.
- Non-`.qmd` input extensions (decide renderable vs artifact).
- Parallel per-file rendering (rayon).

**Epic-level tracking (user directive, 2026-04-23):** add a note
to the parent epic plan's §"Work items" (or equivalent) calling
for a close-out report of all beads issues created *during* the
website-epic work. This lets the user see the full scope of
follow-up work accumulated across phases in one place.
