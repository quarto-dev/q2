---
title: '`q2 render` walks the cwd before checking for `_quarto.yml`'
date: 2026-05-20
description: 'Makes `q2 render` with no arguments check for an enclosing `_quarto.yml` before scanning the working directory, so running it outside a project fails instantly instead of walking large trees first.'
braid:
  strand: bd-nmkmi  # `q2 render` with no args scans cwd for `.qmd` files before checking whether a project exists
  priority: P2
  labels: [cli, perf, render]
---

**Type:** bug · **Priority:** 2

## Summary

`q2 render` invoked with no arguments in a directory that has no
`_quarto.yml` (and no `_quarto.yml` in any ancestor) eventually emits
the correct error:

```
Error: No input given and no `_quarto.yml` found at or above <cwd>
```

…but only *after* walking the entire `cwd` looking for `.qmd` files.
In an empty directory that error appears instantly. In a directory
that contains a large tree (e.g. the q2 repo root, where `target/`
has \~64k entries and a populated `external-sources/quarto-cli/` adds
tens of thousands more), the command appears to hang for many
seconds before printing the error.

The walk is pure waste: we already know the answer once the upward
search for `_quarto.yml` finishes.

## Reproduction

Warm filesystem cache, q2 repo root, `target/` populated:

```
$ time target/debug/q2 render
Error: No input given and no `_quarto.yml` found at or above /Users/cscheid/rooms/room-1/q2
target/debug/q2 render  0.03s user 0.28s system 99% cpu  0.318 total
```

Compare to an empty directory:

```
$ mkdir /tmp/empty && cd /tmp/empty && time q2 render
Error: No input given and no `_quarto.yml` found at or above /tmp/empty
0.01s user 0.00s system 99% cpu  0.013 total
```

300 ms is the *warm-cache* cost on this SSD. On a cold cache or a
fully populated `external-sources/quarto-cli/` checkout (where
`is_excluded_component` does not exclude `external-sources`), the
wall-clock cost is in the multi-second-to-minute range. That is the
user-observed "freeze".

## Root cause

`crates/quarto/src/commands/render.rs:331-346` — `classify_no_inputs`:

```rust
fn classify_no_inputs(
    cwd: &Path,
    runtime: &dyn SystemRuntime,
) -> std::result::Result<RenderTarget, DispatchError> {
    let cwd_canon = runtime
        .canonicalize(cwd)
        .map_err(|e| DispatchError::Discover(e.to_string()))?;
    let project = ProjectContext::discover(&cwd_canon, runtime)   // ← does the walk
        .map_err(|e| DispatchError::Discover(e.to_string()))?;
    if !is_real_project(&project, runtime) {                       // ← only NOW we know
        return Err(DispatchError::NoInputAndNoProject(cwd_canon));
    }
    Ok(RenderTarget::FullProject { project_dir: project.dir })
}
```

`ProjectContext::discover` (crates/quarto-core/src/project/mod.rs:424)
does two things back-to-back when handed a directory:

1. `find_project_config(&search_dir, ...)` — walks *upward* looking
   for `_quarto.yml`/`_quarto.yaml`. Cheap (O(depth)).
2. If no config is found, falls through to
   `discover_project_files(...)` which calls `walk_qmd(project_dir)`
   — a **recursive walk of the entire cwd** collecting `.qmd` files
   (`crates/quarto-core/src/project/discovery.rs:304`).

`walk_qmd`\'s `is_excluded_component` excludes `_*`, `.*`, and
`node_modules`, but not `target/`, `external-sources/`, or any other
large but legitimately-named directory. So in the q2 root we descend
into `target/` and walk all 64k entries (and on machines with
`external-sources/quarto-cli/` fully populated, tens of thousands
more).

The walk's output is then thrown away — `classify_no_inputs`\'s very
next line decides we have no project anyway and returns
`NoInputAndNoProject`.

## Fix approach

Make the cheap upward check the first thing `classify_no_inputs`
does. If no `_quarto.yml` ancestor exists, return
`NoInputAndNoProject` immediately — *without* calling
`ProjectContext::discover` (and therefore without walking the
directory tree at all).

Two ways to express this:

**Option A (minimal, recommended).** Inline the upward search in
`classify_no_inputs`. The loop is six lines and uses only
`runtime.path_exists`.

```rust
fn classify_no_inputs(
    cwd: &Path,
    runtime: &dyn SystemRuntime,
) -> std::result::Result<RenderTarget, DispatchError> {
    let cwd_canon = runtime.canonicalize(cwd)
        .map_err(|e| DispatchError::Discover(e.to_string()))?;

    let Some(project_root) = find_project_root_upward(&cwd_canon, runtime)? else {
        return Err(DispatchError::NoInputAndNoProject(cwd_canon));
    };

    // We know a real project exists. Now do the full discovery
    // (which includes the file walk we deliberately skipped above).
    let project = ProjectContext::discover(&project_root, runtime)
        .map_err(|e| DispatchError::Discover(e.to_string()))?;
    Ok(RenderTarget::FullProject { project_dir: project.dir })
}
```

with a small private helper that walks `current → parent → ...`
checking for `_quarto.yml` / `_quarto.yaml` and returns
`Ok(Some(dir))` on first hit, `Ok(None)` at filesystem root.

**Option B (factored).** Extract `ProjectContext::find_project_config`
(currently private, crates/quarto-core/src/project/mod.rs:525) into
a public free function `find_project_root(start, runtime) -> Result<Option<PathBuf>>`
that returns the root without parsing the config. Use it from both
`classify_no_inputs` and `ProjectContext::discover` itself (the
latter's upward walk becomes a call to the shared helper, then
parses the config if a hit).

**Recommendation: Option A.** The duplication is six lines and one
upward loop; lifting it into quarto-core just to share six lines
costs a public API surface that no other caller asked for. Revisit
if a third caller appears.

`is_real_project` (render.rs:353) can stay — it remains the correct
check for the *with-inputs* path, where a per-input `ProjectContext`
exists and we have to disambiguate "directory input outside any
project" from "directory input is a project root".

### Out of scope (explicitly)

- **Excluding `target/` from `walk_qmd`.** The walker is wrong for
  this case too — even when a real project's `_quarto.yml` lives in
  a directory that happens to contain a `target/` sibling, the walk
  will descend into it. But that fix is a separate behavioral
  change (what counts as an "excluded component"?) and risks
  surprising users with directories legitimately named `target`.
  File as a follow-up if the maintainer agrees the heuristic should
  grow.
- **Improving the empty-cwd error message.** The current text is
  already accurate and points the user at the right thing
  (`_quarto.yml` or an explicit file argument).

## Test plan (TDD — tests first, then fix)

### Test 1 — `classify_no_inputs` in an empty dir errors without walking

`crates/quarto/src/commands/render.rs` test module.

A `RecordingRuntime` wrapper around `NativeRuntime` that counts
`dir_list` calls. The test sets up:

- Temp dir with no `_quarto.yml` and a sentinel subdirectory
  (e.g. `target/`) with several files inside.
- Calls `classify_inputs(&[], &temp_dir, &recording_runtime)`.
- Asserts the result is `Err(DispatchError::NoInputAndNoProject(_))`.
- **Asserts `recording_runtime.dir_list_count() == 0`.** This is
  the regression assertion — pre-fix it is non-zero (at least 1 for
  the temp dir, plus 1 per subdirectory descended).

This makes the test independent of wall-clock time and robust on
fast filesystems.

### Test 2 — `classify_no_inputs` in a real project still works

Already covered by `classify_no_args_in_project_returns_full_project`
(render.rs:799). Keep it green.

### Test 3 — `classify_no_inputs` in a subdir of a project still finds it

Add a new test (currently no coverage for the "cwd is N levels deep
inside a project" case from the no-input path):

- Project with `_quarto.yml` at `<temp>/p/_quarto.yml`.
- Call `classify_inputs(&[], &<temp>/p/sub/sub2, &runtime)`.
- Assert `Ok(FullProject { project_dir: <temp>/p })`.

### End-to-end verification (per CLAUDE.md)

After the fix:

1. `cd /Users/cscheid/rooms/room-1/q2 && time target/debug/q2 render`
   — record wall-clock time. Expected: well under 50 ms even on a
   cold cache (only a handful of `_quarto.yml`/`.yaml`
   `path_exists` calls walking up to `/`).
2. `cd /tmp/empty && time q2 render` — should be unchanged
   (already fast).
3. `cd docs && time q2 render` — should be unchanged
   (real project, walks `docs/` after confirming `_quarto.yml`).

Record both the timing and the unchanged error text in the close
note.

## Work items

- [x] Add `RecordingRuntime` test helper (or equivalent) in
  `render.rs` tests that counts `dir_list` calls against an
  inner `NativeRuntime`. Modelled on
  `CountingRuntime` in
  `crates/quarto-core/src/project/listing/post_render_upgrade/substitute.rs:852`.
- [x] Write **Test 1** (`classify_no_inputs_does_not_walk_cwd`):
  no-project cwd populated with a decoy subdir,
  `dir_list_count == 0`, result is `NoInputAndNoProject`.
  Pre-fix it observed `dir_list_count = 2` (cwd + `sub/`).
- [x] Write **Test 3**
  (`classify_no_args_from_project_subdir_returns_full_project`):
  cwd is two levels deep inside a project; result is
  `FullProject{ project_dir: <project root> }`. Passes both
  pre-fix and post-fix (regression sanity).
- [x] Implement Option A: inline upward `_quarto.yml`/`.yaml`
  search via new `find_project_root_upward` helper;
  `classify_no_inputs` short-circuits on miss.
- [x] Run Tests 1, 2, 3 + all `classify_no_args_*` tests — green.
- [x] Run `cargo nextest run -p quarto` — 100/100 pass.
- [x] Run `cargo xtask verify --skip-hub-build` — all 12 steps
  green (lint, fmt, Rust build with `-D warnings`, tree-sitter,
  Rust tests, hub-client tests, trace-viewer, preview-\*,
  q2-preview-spa build).
- [x] End-to-end verify (warm cache, debug build, this machine):

  | scenario                       | before  | after   |
  | ------------------------------ | ------- | ------- |
  | `q2 render` in q2 root         | 0.318 s | 0.021 s |
  | `q2 render` in `/tmp/empty/…`  | 0.013 s | 0.006 s |
  | `q2 render` in `docs/`         | renders | renders |
  | `q2 render` in `docs/<sub>/`   | n/a     | renders |

  Error text in the two error scenarios is byte-identical:
  ``Error: No input given and no `_quarto.yml` found at or above <cwd>``.
  The 15× speedup is the warm-cache delta — the cold-cache
  delta (where `target/` was the user-reported "freeze") is
  orders of magnitude larger.
- [x] Close bd-nmkmi with the table above in the close reason.

## Discovered work

- **bd-k4ahh** — `q2 render <large-non-project-dir>` walks the tree
  before erroring. Symmetric to bd-nmkmi but on the *with-arguments*
  path (`classify_inputs` calls `ProjectContext::discover` per input,
  which falls through to the recursive walk for directory inputs
  without a project marker). Filed `discovered-from: bd-nmkmi`,
  priority 3 — much rarer in practice than the bare `q2 render`.

## Related

- `crates/quarto/src/commands/render.rs:331-346` — `classify_no_inputs`
  (the fix site).
- `crates/quarto-core/src/project/mod.rs:424-500` —
  `ProjectContext::discover` (the function that does too much when
  asked about a project-less directory).
- `crates/quarto-core/src/project/discovery.rs:304-340` —
  `walk_qmd` / `walk_rec` / `is_excluded_component` (the walk we
  are skipping; also the home of the out-of-scope `target/`
  exclusion question).
- bd-m9rm — Default project render regression and project-level
  diagnostics. Different bug (default project *with* `_quarto.yml`
  produces zero output), but lives in the same neighborhood; not a
  blocker.
