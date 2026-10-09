---
title: 'bd-i6jy4 — Filter eager-capture driver to `.qmd` files only'
date: 2026-05-19
description: 'Makes the preview''s eager capture pass iterate only the project''s `.qmd` files, so binary assets and config files such as `_quarto.yml` no longer reach the parser and trigger a panic in `q2 preview` at startup.'
braid:
  strand: bd-i6jy4
  priority: P1
---

## Overview

`crates/quarto-preview/src/capture_driver.rs::record_eager_captures` iterates
`ctx.index().get_all_files()`, which returns every file in the project index
— qmd, `_quarto.yml`, images, extension files. Each non-qmd entry then gets
shoved through `compute_input_qmd → parse-document`. Symptoms in the wild:

- `_quarto.yml` → parse-document errors out ("Indented code blocks are not
  supported"), caught at the per-file soft-fail and logged as a noisy WARN.
- `quarto.png` (or any binary) → tree-sitter parses binary bytes, produces
  parse errors, and `produce_diagnostic_messages` panics on a non-UTF-8
  byte (sibling bug bd-6qbto). The panic crosses the soft-fail boundary
  because it kills the worker thread instead of returning `Err`.

The function's doc comment already says "Walk the project's `.qmd` files".
The code never matched. This issue fixes the code so it matches.

The secondary defensive fix (panic-free `error_generation.rs`) is tracked
separately as bd-6qbto and is out of scope here.

## Reproduction

```
$ q2 preview          # in q2/docs/, which contains quarto.png
  q2 preview
  → http://127.0.0.1:64561/?page=index.qmd

thread 'tokio-rt-worker' panicked at
  crates/quarto-parse-errors/src/error_generation.rs:121:35:
start byte index 7748 is not a char boundary;
  it is inside '�' (bytes 7746..7749 of string)
```

The panic happens on startup (the eager-capture pass), regardless of any
HTTP request.

## Design

The iteration body uses only `rel_path` from each `(rel_path, _doc_id)`
pair — the doc_id is discarded (`_doc_id`). So the filter doesn't have to
preserve the index lookup; it can iterate `ProjectFiles::qmd_files`
directly.

`ProjectFiles::qmd_files` (in `crates/quarto-hub/src/discovery.rs`) is a
`Vec<PathBuf>` of project-root-relative paths. `ctx.project_files()`
returns `Option<&ProjectFiles>`; the existing early-return at L75 already
bails when it's `None`, so we can unwrap unconditionally after that
guard.

`record_one` takes `rel_path: &str`, so each `PathBuf` from `qmd_files`
needs to be string-rendered. Index keys use `to_string_lossy().into_owned()`
(see `reconcile_files_with_index` at `crates/quarto-hub/src/context.rs:459`),
so we follow the same convention to keep `has_capture(&rel_path)` lookups
consistent with the index's keying.

### Why not filter `get_all_files()` post-hoc?

Option B would be to filter the existing iteration: `if !rel_path.ends_with(".qmd") { continue; }`.
Rejected because:

1. The extension check duplicates the classification logic that
   `ProjectFiles::discover` already encodes. If qmd discovery ever
   accepts other extensions (the TS Quarto tree historically supported
   `.Rmd`, `.ipynb`, etc.), the filter would silently fall behind.
2. Iterating the index does an indirect lookup through automerge; it's
   more code than directly walking `qmd_files`.
3. The doc comment promised "Walk the project's `.qmd` files" — the
   direct-iteration version matches that promise word-for-word.

### Touched files

- `crates/quarto-preview/src/capture_driver.rs` — change the iteration in
  `record_eager_captures` and add a regression test that mixes qmd +
  binary in the project.
- `crates/quarto-preview/src/capture_driver.rs::tests::build_ctx_with_files`
  — extend to accept binary content too (currently text-only).

No public-API changes. No changes outside `quarto-preview`.

## Work items

### Phase 1 — Test first (TDD)

- [x] Add binary-fixture variant of `build_ctx_with_files` (or a sibling
      helper) that writes raw bytes to the project root, so a `.png`-shaped
      fixture can be created from a tiny byte literal.
- [x] Write `mixed_qmd_and_binary_does_not_panic` test:
      - Project with one qmd ("doc.qmd": prose only, no engine cells) and
        one binary ("logo.png": a few bytes of `0xff` / arbitrary non-UTF-8).
      - Call `record_eager_captures` with `EnginePolicy::Manual`, no engine
        registry.
      - Assert it returns `Ok(_)` (no panic).
      - Assert `ctx.index().has_capture("logo.png")` is `false`.
- [x] Run the test on the unmodified driver; confirm it **panics** (not
      just errors) so we know the test would have caught the bug.

### Phase 2 — Implement the filter

- [x] Change `record_eager_captures` to iterate `project_files.qmd_files`
      (drop the `_doc_id` from the loop).
- [x] Update the doc comment if needed to reflect the implementation
      (it already says ".qmd files", so likely no change).
- [x] Re-run the regression test; confirm pass.

### Phase 3 — Full verification

- [x] Run all tests in `quarto-preview`.
- [x] Run `cargo nextest run --workspace` to catch any downstream
      regression (per CLAUDE.md monorepo rule).
- [x] Run `cargo xtask verify --skip-hub-build --skip-hub-tests` (this
      worktree doesn't have the WASM build wired up; the change is
      Rust-only so `--skip-hub-build` is the right scope per CLAUDE.md).

### Phase 4 — End-to-end verification

- [x] Run `cargo run --release --bin q2 -- preview --no-browser` from
      `docs/` (which contains `quarto.png`) and confirm:
      - No thread panic.
      - The startup WARN for `_quarto.yml` is also gone (config files
        no longer reach the parse-document path either).
      - The server boots and stays up.
- [x] Capture exact invocation + observed output in this plan.

### Phase 5 — Wrap-up

- [x] Stage commit on `beads/bd-i6jy4-preview-eager-capture-driver`.
- [ ] Ask user before pushing.
- [ ] After merge / push: `br close bd-i6jy4 --reason "..."`, sync, commit
      `.beads/` on main.

## End-to-end observation

### Before the fix

```
$ cd docs && q2 preview --no-browser
  q2 preview
  → http://127.0.0.1:64561/?page=index.qmd
[...] WARN quarto_preview::capture_driver: failed to record engine
       capture; continuing rel_path=_quarto.yml
       error=engine capture pipeline failed: Stage 'parse-document'
       failed: Indented code blocks are not supported
thread 'tokio-rt-worker' (46568702) panicked at
  crates/quarto-parse-errors/src/error_generation.rs:121:35:
start byte index 7748 is not a char boundary;
  it is inside '�' (bytes 7746..7749 of string)
```

### After the fix

```
$ cd docs
$ .worktrees/bd-i6jy4-.../target/release/q2 preview --no-browser --port 64565
  q2 preview
  → http://127.0.0.1:64565/?page=index.qmd
[...] INFO quarto_hub::context: Discovered project files
       qmd_count=2 config_count=1 binary_count=1 ...
[...] INFO quarto_hub::context: Reconciled new files with index count=4
[...] INFO quarto_hub::context: Initial filesystem sync complete
       synced=4 errors=0
[...] INFO quarto_hub::server: Hub server listening
       (project mode) addr=127.0.0.1:64565
[...] INFO quarto_hub::server: Starting filesystem watcher
[...] INFO quarto_hub::server: Received SIGTERM, initiating graceful shutdown...
```

Confirmed:

- No `tokio-rt-worker` panic line.
- No `failed to record engine capture` WARN for `_quarto.yml`.
- The `binary_count=1` (the PNG) and `config_count=1` (`_quarto.yml`)
  are still discovered and reconciled into the index — they just no
  longer get fed into parse-document.
- Server boots cleanly and accepts SIGTERM normally.

Both failure modes the fix targeted are gone; behavior for the qmd
files themselves is unchanged (Reconciled count=4 = 2 qmd + 1 yml +
1 png, same as before).
