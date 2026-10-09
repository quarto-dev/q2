---
title: 'Default-project rendering and project-level diagnostics'
date: 2026-05-01
---

## Status

Design questions resolved (2026-05-01): user accepted all three
recommendations. Awaiting explicit go-ahead to start implementation.

## Resolved decisions

1. **Default-project output dir**: keep "beside the source file"
   (current behavior). The fix is a guard in `discover_project_files`
   so the output_dir-exclusion check is skipped when
   `output_dir == project_dir`.
2. **Project-level diagnostic surface**: extend `RenderSummary` with
   `project_diagnostics: Vec<ProjectDiagnostic>` carrying
   `severity`, `code`, `message`, optional `hint`. Both CLI and
   hub-client consume it. Generalizes to future project-level
   diagnostics.
3. **`NotInRenderList` wording**: no change. Once the discovery bug
   is fixed it only fires on real misconfiguration (user-set
   `project.render` excludes the explicit input), and the existing
   message is appropriate.

## Problem

After the websites feature merge (commit `dbaa5bbf`), the presence of
a `_quarto.yml` changed how a single-file render behaves. The
regression manifests in three user-visible ways, all reproducible on
`/Users/cscheid/Desktop/daily-log/2026/05/01/default-project-test/`
(an `index.qmd` next to a minimal `_quarto.yml` containing only
`project: { type: default }`):

1. **`q2 render` with no args** in the project directory exits 0 and
   produces no output. No file is written, no message is printed,
   nothing in `_site/` (which doesn't exist for a default project
   anyway). The user has no way to tell whether the render
   succeeded, was a no-op, or silently failed.

2. **`q2 render index.qmd`** fails with
   `Error: …/index.qmd is excluded from the render list of project
   …/default-project-test (check `project.render` in `\_quarto.yml`
   and the underscore/hidden file conventions).` This is a
   misleading error: the user did not configure any `project.render`
   list, and the file is not hidden, underscore-prefixed, or a
   README.

3. **Hub-client preview** of the same project would presumably fail
   the same way (not yet reproduced in the browser; assumed from
   the shared orchestration path). There is no surface for
   project-level diagnostics in hub-client today, only file-level
   ones.

Previously (pre-websites) the same `index.qmd` would have rendered
as a single-file project because `_quarto.yml` was effectively
ignored for individual file renders.

## Root cause

`crates/quarto-core/src/project/mod.rs:60` defines
`default_output_dir`:

```rust
fn default_output_dir(dir: &Path, config: Option<&ProjectConfig>) -> PathBuf {
    match config.map(|c| c.project_kind) {
        Some(ProjectKind::Website) => dir.join("_site"),
        _ => dir.to_path_buf(),
    }
}
```

For `ProjectKind::Default` (and Book / Manuscript, which currently
fall back to default), the output directory is the project root
itself. `discover_project_files` then runs:

```rust
// crates/quarto-core/src/project/discovery.rs:90
if starts_with(candidate, config.output_dir) {
    return false;
}
```

Every candidate path starts with the project root, so the
exclusion check rejects all of them. The render list is empty,
which causes:

- Mode A (`q2 render` with no args) to render zero files silently —
  no error because "all zero files succeeded."
- Mode B (`q2 render index.qmd`) to report `NotInRenderList` because
  the explicit input is not in the (empty) project file list.

Confirmed by swapping `type: default` → `type: website` in the
fixture: the website branch produces `_site/index.html` correctly,
because `output_dir = project_dir/_site` excludes only the build
output directory rather than everything.

## Goals

1. **Fix the discovery bug** so default projects discover their
   `.qmd` files the same way websites do (walk the project root,
   exclude conventional dotfiles / underscore / README, exclude the
   actual build output directory if any).

2. **Replace the silent no-output behavior** with a clear
   project-level diagnostic when the render set is empty. The
   diagnostic should distinguish:
   - "this project has zero renderable files" (likely a
     misconfiguration: `project.render` excludes everything, or the
     directory really is empty),
   - from "you asked to render a specific file that the project
     intentionally excludes."

3. **Establish a project-level diagnostic surface** that both the
   CLI and hub-client can render, and route the empty-render-set
   diagnostic through it. We do not yet have one — the existing
   hub-client `Diagnostic` flow is per-file (driven by
   `summary.pass1_failures` / `pass2_failures`).

## Non-goals

- Wider rework of project discovery rules (extension support beyond
  `.qmd`, glob-engine swap, etc.) is out of scope. See the websites
  Phase-1 plan for those.
- Changing what `project.render: []` means is out of scope. If the
  user explicitly sets an empty render list, we still report it
  empty — but with a clearer message.
- Book and manuscript-specific behavior is out of scope; they will
  benefit incidentally from the default-project fix because they
  fall through to `DefaultProjectType` today.

## Test plan (TDD — these go in first)

### Unit tests in `crates/quarto-core/src/project/discovery.rs`

- New: `discovery_default_project_walks_when_output_dir_equals_root`.
  Project dir contains `index.qmd` and `about.qmd`. `DiscoveryConfig`
  passes `output_dir == project_dir`. Expect both files in result.
  Currently fails (returns `[]`).

- New: `discovery_excludes_real_output_dir`. Project dir contains
  `index.qmd` and `_site/old.qmd`. `output_dir = project_dir/_site`.
  Expect only `index.qmd` (existing behavior, regression guard for
  the website case).

### Unit tests in `crates/quarto/src/commands/render.rs`

- New: `render_empty_project_emits_diagnostic`. Use a
  `MemRuntime` with a `_quarto.yml` whose `project.render` is `[]`
  (explicit empty list). `q2 render` should emit an
  `EmptyRenderSet` project-level diagnostic via
  `RenderSummary.project_diagnostics`.

- New: `render_default_project_with_one_file_succeeds`. Mirror of
  the user's repro: `_quarto.yml` with `type: default` only,
  `index.qmd` next to it. `q2 render index.qmd` should produce
  output and exit zero. Currently fails with `NotInRenderList`.

### CLI integration test

- New: end-to-end test under
  `crates/quarto/tests/` that runs the binary against a temp
  project matching the user's fixture and asserts on stdout/stderr
  and the produced `index.html`. This is the verification that
  matches CLAUDE.md's "End-to-end verification before declaring
  success" requirement.

### Hub-client tests

- A test in `wasm-quarto-hub-client` that an empty-project render
  surfaces the diagnostic in the summary it returns to JS.
- A React test in `hub-client/src/components/render/` that the
  diagnostic renders in the new project-diagnostic banner.

## Work items

### Phase 0: Tests in (TDD)

- [x] Add discovery unit tests above; verify they fail.
      `discovery_default_project_walks_when_output_dir_equals_root`
      fails (returns `[]`); `discovery_excludes_real_output_dir`
      passes (regression guard).
- [x] Add CLI dispatcher unit tests; verify they fail.
      `classify_one_qmd_in_default_project_succeeds` fails with
      `NotInRenderList`. `classify_no_args_in_default_project_returns_full_project`
      already passes (the silent-no-output bug manifests downstream
      of classification — Phase 2 diagnostic catches it).
- [x] Add CLI end-to-end test against a real binary invocation.
      `default_project_renders_named_file_and_produces_html` fails
      with the user's exact stderr; `default_project_renders_with_no_args_and_produces_html`
      fails because no `index.html` is written.

### Phase 1: Fix the discovery regression

- [x] In `is_renderable_qmd`, skip the
      `starts_with(candidate, output_dir)` exclusion when
      `output_dir == project_dir`. (Decision Q1: keep
      "beside the source file" for default projects; narrowest
      fix.)
- [x] Run new tests; verify they pass. All Phase-0 tests now green.
- [x] Run full workspace tests; verify no regression. 8194/8194
      passed.
- [x] Run `cargo xtask verify` (full, since `quarto-core` was
      touched). All 9 steps green, including the WASM build and
      hub-client tests.

### Phase 2: Project-level diagnostic surface

- [x] Add `ProjectDiagnostic { severity, code, message, hint }` and
      `RenderSummary.project_diagnostics: Vec<ProjectDiagnostic>`.
      **Discovered already exists**: `ProjectRenderSummary` has
      `project_diagnostics: Vec<DiagnosticMessage>` (built for
      Phase 7 `WebsiteProjectType.post_render`). `DiagnosticMessage`
      already carries `code`, `kind` (severity), `title`, `problem`,
      `hints`, `details` — superset of the planned shape. Reused
      directly; no new type needed.
- [x] Emit `EmptyRenderSet` from the orchestrator when it sees
      zero candidate files. Implemented as
      `ProjectPipeline::empty_render_set_diagnostic` (gated on
      `Full` and `ActivePage` modes — `Subset` is dispatcher-
      guaranteed non-empty). Diagnostic code: `Q-PROJECT-EMPTY`.
      Hint adapts based on whether `project.render` is configured.
- [x] CLI: print project diagnostics after the Pass-2 summary
      (`print_render_diagnostics` already does this); add Error-
      severity to the non-zero exit policy via new
      `should_exit_nonzero` helper.
- [x] Hub-client WASM: include project diagnostics in the summary
      returned to JS. **Already wired**: `wasm-quarto-hub-client`
      extends `all_diags` with `summary.project_diagnostics` at
      `lib.rs:1522`, returns them in the `warnings` field.
- [x] Hub-client UI: surfaces project diagnostics through the
      existing `warnings → allDiagnostics → onDiagnosticsChange`
      pipeline (`Preview.tsx:118`). **No separate banner needed**
      — file-level and project-level diagnostics share the same
      surface, and once a real project diagnostic flows (e.g.
      hub-client opens a project with `render: ['ghost.qmd']`),
      it shows up alongside file-level warnings without UI work.

### Phase 3: Verification

- [x] Reproduce the user's failure modes against the fixed binary
      and confirm they're resolved.
- [x] Record the end-to-end runs in this plan (see below).
- [x] No new follow-up beads needed: the empty-glob case is the
      same diagnostic, and the hub-client analog is covered by the
      `ActivePage`-mode extension.

#### End-to-end runs (CLAUDE.md verification policy)

Tested against the user's exact fixture:
`/Users/cscheid/Desktop/daily-log/2026/05/01/default-project-test/`
with `_quarto.yml` containing only `project: { type: default }`
and a single `index.qmd`.

**Mode A (no args)** — pre-fix: silent exit 0, no output.
Post-fix:
```
$ q2 render
EXIT: 0
$ ls index.html
index.html
$ head -2 index.html
<!DOCTYPE html>
<html>
```

**Mode B (file arg)** — pre-fix: `Error: …/index.qmd is excluded
from the render list of project …`. Post-fix:
```
$ q2 render index.qmd
EXIT: 0
$ head -2 index.html
<!DOCTYPE html>
<html>
```

**Diagnostic (misconfigured `render:` list)** — new scenario,
verified at `/tmp/empty-render-test/` with `render: [ghost.qmd]`:
```
$ q2 render
Error [Q-PROJECT-EMPTY]: Project has no renderable files
Project at `/private/tmp/empty-render-test` resolved to an empty render set.
ℹ Check `project.render` in `_quarto.yml` — its globs matched no `.qmd` files.

EXIT: 1
```

## Files likely touched

- `crates/quarto-core/src/project/mod.rs` — `default_output_dir`
  or callers.
- `crates/quarto-core/src/project/discovery.rs` —
  `is_renderable_qmd` output_dir check.
- `crates/quarto-core/src/project/orchestrator.rs` — emit
  project-level diagnostic if the file list is empty.
- `crates/quarto/src/commands/render.rs` — print project
  diagnostics; possibly soften `NotInRenderList` wording.
- `crates/wasm-quarto-hub-client/src/lib.rs` — pass project
  diagnostics through the summary.
- `hub-client/src/components/render/Preview.tsx` (and a new
  `ProjectDiagnosticsBanner.tsx`) — display them.

## Tracking

- Beads issue: **bd-h736**.
- Parent: bd-0tr6 (websites epic) via `discovered-from`.
