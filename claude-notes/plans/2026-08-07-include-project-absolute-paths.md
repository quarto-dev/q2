# Include shortcode: project-absolute (root-relative) path resolution

**Strand:** bd-w9koo1i2
**Status:** implemented; PR open — https://github.com/quarto-dev/q2/pull/468
**Date:** 2026-08-07

## Overview

`{{< include /admin/_includes/license-file.qmd >}}` fails with Q-17-2
("Include file not found"), because the leading-`/` path is handed to the
filesystem as an OS-absolute path. Per the Quarto path convention (Q1, and
q2's own glob layer, decision D2), a leading `/` means
**project-root-relative**: `/admin/_includes/license-file.qmd` =
`<project>/admin/_includes/license-file.qmd`.

Found while porting the posit-connect docs to Quarto 2
(`~/Desktop/daily-log/2026/08/05/q2-connect-docs/docs-quarto-2`,
`admin/licensing/index.md:146` and many more sites).

### Minimal reproduction (verified 2026-08-07)

```
repro/
  _quarto.yml            # project: {type: default}
  sub/doc.qmd            # contains: {{< include /sub/_includes/snippet.qmd >}}
  sub/_includes/snippet.qmd
```

`cargo run --bin q2 -- render repro/` warns:

```
Warning: [Q-17-2] Include file not found
  Could not read included file '/sub/_includes/snippet.qmd':
  I/O error: No such file or directory (os error 2)
```

## Diagnosis

### Primary defect

`crates/quarto-core/src/stage/stages/include_expansion.rs:150-152`
(`IncludeExpander::expand_blocks`):

```rust
// Resolve relative to the including file's directory
let base_dir = current_file.parent().unwrap_or(Path::new("."));
let resolved = base_dir.join(&include_path);
```

`Path::join` with an absolute right-hand side **discards the base**, so a
leading-`/` include path becomes a filesystem-absolute path and every
downstream step (cycle detection, `file_read`, `recorded_includes`,
diagnostics) operates on the wrong path.

### Secondary site (preview dep-graph)

`crates/quarto-preview/src/deps.rs:213` (`extract_include_deps`):

```rust
let joined = page_dir.join(&raw);
normalize_forward_slash(&joined)
```

Same `join` trap. `normalize_forward_slash` then hits the
`Component::RootDir` arm and returns the raw leading-`/` string, which never
matches the SPA's forward-slash project-relative paths — so editing the
included file does not trigger a re-render of the includer in `q2 preview`.

### Reference semantics (Q1)

`external-sources/quarto-cli/src/core/handlers/base.ts:224-236`:

```ts
resolvePath(path: string): string {
  const sourceDir = dirname(options.context.target.source);
  const rootDir = options.context.project.isSingleFile
    ? sourceDir
    : options.context.project.dir;
  if (path.startsWith("/")) {
    return resolve(rootDir, `.${path}`);   // root-relative
  } else {
    return resolve(sourceDir, path);       // relative
  }
}
```

Key points:

- Leading `/` → resolve against **project root**; for single-file renders,
  against the **source file's directory**.
- q2's `ProjectContext.dir` already encodes exactly this anchor: it is the
  `_quarto.yml` directory when a project exists, and the input file's
  directory when `is_single_file` (`crates/quarto-core/src/project/mod.rs:505-587`).
  So `ctx.project.dir` is the correct anchor in *all* modes, with no
  `is_single_file` branch needed.
- Nested includes: Q1 anchors leading-`/` at the same fixed root at every
  nesting level. (q2 resolves *relative* nested includes against the
  included file's own directory — bd-1fz3vh99, deliberate; unaffected here.)

### Existing q2 precedent

The convention is already implemented lexically for globs/resources:
`quarto_core::glob::join_and_normalize`
(`crates/quarto-core/src/glob/pattern.rs:88`) — "A pattern beginning with
`/` is **project-root-relative** (decision D2, Quarto YAML convention)".
It also normalizes backslashes and clamps `..` at the project root. It is
`pub use`d from `quarto_core::glob`.

### WASM note

The same `IncludeExpansionStage` runs in the hub-client WASM build, where
VFS paths live under `/project/`. Today a leading-`/` include resolves to
`/sub/...` (outside the VFS root → always missing). Anchoring at
`ctx.project.dir` (which is the VFS project root in that context) fixes
WASM/live-preview too. Verify during implementation what
`project.dir` actually is in the WASM render entry points.

## Fix design

1. **New helper** in `include_expansion.rs` (unit-testable, no fs access):

   ```rust
   /// Resolve a raw include-shortcode path. A leading `/` (or `\`) is
   /// project-root-relative per the Quarto path convention (see
   /// glob::join_and_normalize, decision D2); anything else resolves
   /// against the including file's directory.
   fn resolve_include_target(base_dir: &Path, project_dir: &Path, raw: &str) -> PathBuf
   ```

   Behavior:
   - `raw` starts with `/` or `\` → `project_dir.join(raw trimmed of leading
     slashes, backslashes normalized)`.
   - otherwise → `base_dir.join(raw)` (unchanged behavior).
   - Windows drive-absolute paths (`C:\...`) fall through the `join` as
     today (Q1 has no defined semantics for them either; do not invent one).

   Call it at `expand_blocks` (replacing lines 150-152), passing
   `self.ctx.project.dir`. Cycle detection, `file_read`, diagnostics, and
   `recorded_includes` all pick up the corrected path for free.

2. **Preview dep-graph** (`quarto-preview/src/deps.rs`): in
   `extract_include_deps`, treat a leading-`/` raw path as already
   project-relative — bypass `page_dir.join` and normalize the stripped
   path directly (or route both arms through
   `quarto_core::glob::join_and_normalize(page_dir_str, raw)`, which
   implements exactly this and additionally clamps `..`; preferred if the
   `Option` (root-escape) case maps cleanly onto the existing fallback).

3. **Out of scope** (documented, not changed):
   - `include_resolve.rs` (metadata `include-in-header` etc.) is
     document-relative by its own plan
     (2026-05-04-includes-feature.md §Resolved questions #4, pending
     `!path`); if it should also learn the leading-`/` convention, that is
     a separate strand.
   - Reference-doc update: `docs/` include documentation should mention
     root-relative includes once implemented (check whether a page exists).

## Work items (TDD order)

### Phase 1 — tests first (must fail before the fix)

- [x] Unit tests for `resolve_include_target` in `include_expansion.rs`:
      relative path unchanged; leading `/` anchors at project dir; leading
      `\` (Windows-authored) same; `//double` collapse. (Written alongside
      the helper; the behavioral failing-first evidence came from the
      integration + preview tests.)
- [x] Integration test (new file
      `crates/quarto-core/tests/integration/include_project_absolute.rs`,
      registered alphabetically in `tests/integration/main.rs`). Harness
      improves on the plan: it uses `ProjectContext::discover` on a real
      temp-dir layout (with/without `_quarto.yml`) so the anchor comes from
      the CLI's own discovery branch. Five tests: project mode, cross-tree
      target, single-file parity, nested leading-`/`, relative regression
      guard.
- [x] Preview dep test in `quarto-preview` (alongside the existing
      `extract_include_deps` unit tests): a page `sub/doc.qmd` with
      `{{< include /sub/_includes/x.qmd >}}` yields dep
      `sub/_includes/x.qmd`.
- [x] Run the new tests, **verify they fail** for the expected reason.
      Verified 2026-08-07: the four leading-`/` integration tests fail
      with Q-17-2 ("Could not read included file '/other/_b.qmd'" etc.),
      the relative-include guard passes; the preview test fails returning
      the raw `/sub/_includes/x.qmd` (RootDir fallback), not the
      project-relative form.

### Phase 2 — implementation

- [x] Add `resolve_include_target` and use it in `expand_blocks`
      (module + method docs updated to state the two anchors).
- [x] Fix `extract_include_deps` leading-`/` handling (local branch in
      `deps.rs`, mirroring the stage's anchors; kept deps' existing
      lenient `..` normalization untouched to avoid scope creep).
- [x] All new tests pass (43 include-related quarto-core tests, full
      quarto-preview suite 89/89).

### Phase 3 — regression + end-to-end verification

- [x] `cargo build --workspace` and `cargo nextest run --workspace` —
      11060 passed, 197 skipped (2026-08-07).
- [x] Full `cargo xtask verify` (quarto-core changed → WASM leg affected) —
      all 14 steps passed (2026-08-07).
- [x] End-to-end: `cargo run --bin q2 -- render
      <scratch>/include-repro` — no warnings; rendered
      `sub/doc.html:30` contains `<p>INCLUDED CONTENT MARKER</p>`
      (file inspected).
- [x] End-to-end: re-rendered the posit-connect docs project
      (`q2 render ~/Desktop/daily-log/2026/08/05/q2-connect-docs/docs-quarto-2`);
      Q-17-2 count went from many to **0**, and the license-file callout
      content ("license file activation") appears 5× in the rendered
      `admin/licensing/index.html` — one per former warning site.
- [ ] Optional: verify in `q2 preview` that editing
      `sub/_includes/snippet.qmd` re-renders `sub/doc.qmd` (dep-graph fix);
      remember the preview WASM rebuild chain if exercising the SPA.
- [ ] Close bd-w9koo1i2 with a summary; changelog entry if user wants one.

## Notes

- The Q-17-2 diagnostic prints the resolved path; after the fix it will
  print the real `<project>/...` path, which is a strictly better message.
  No diagnostic-text change needed.
- `collect_include_paths` itself stays raw-string; both consumers
  (expander, preview deps) apply the convention at their own resolution
  points.
