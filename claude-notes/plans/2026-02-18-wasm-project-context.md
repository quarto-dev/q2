# WASM Project Context Discovery

## Overview

Make the WASM `render_qmd` function use the shared project discovery and directory metadata infrastructure from `quarto-core`, instead of hardcoding a minimal single-file `ProjectContext`. This enables WASM rendering to support `_quarto.yml` and `_metadata.yml` files from the VFS.

## Current State

- `wasm-quarto-hub-client/src/lib.rs` has `create_wasm_project_context()` which always returns `ProjectContext { config: None, is_single_file: true }`
- `quarto-core/src/project.rs` has `ProjectContext::discover()` which walks parent directories for `_quarto.yml` — already takes `&dyn SystemRuntime`, so it works with VFS
- `quarto-core/src/project.rs` has `directory_metadata_for_document()` which uses `std::fs` directly — needs porting to `SystemRuntime`
- `AstTransformsStage` already handles the full merge pipeline (project + directory + document metadata) — it's shared code
- `StageContext` already carries `runtime: Arc<dyn SystemRuntime>` — the runtime is available in every pipeline stage

## Key Design Decision: Unify the Two WasmRuntime Instances

### Problem

`render_qmd()` (and similar functions) currently create two separate `WasmRuntime` instances:

1. **Global singleton** (`get_runtime()`, line 36-45): Populated by JavaScript via Automerge sync with ALL project files — `.qmd` documents, `_quarto.yml`, `_metadata.yml`, images, etc. Used for VFS reads (`file_read`) and artifact write-back.
2. **Fresh empty runtime** (`Arc::new(WasmRuntime::new())`, lines 475/639/726/861): Created per-render and passed to the `render_qmd_to_html` pipeline. Contains nothing.

The empty pipeline runtime is **not an intentional isolation boundary** — it's a placeholder that satisfied the `Arc<dyn SystemRuntime>` type signature when single-file rendering was first wired up. Evidence:

- The pipeline runtime is never used for file reads (document content is passed as `&[u8]`)
- Artifacts are written back to the *global* runtime, not the pipeline runtime (lines 645-649 use `get_runtime()`)
- There's no snapshot/fork/copy-on-write VFS mechanism — `WasmRuntime::new()` just creates an empty `HashMap`
- `render_qmd_content_with_options` manually constructs project config that `discover()` would provide if it had VFS access

### Solution

Change the global from `OnceLock<WasmRuntime>` to `OnceLock<Arc<WasmRuntime>>`. Then clone the `Arc` wherever the pipeline needs `Arc<dyn SystemRuntime>`, so the pipeline reads from the same VFS that JavaScript populates.

## Work Items

### Phase 0: Unify WasmRuntime — share the global VFS with the pipeline

- [x] Change `static RUNTIME: OnceLock<WasmRuntime>` to `OnceLock<Arc<WasmRuntime>>` in `wasm-quarto-hub-client/src/lib.rs`
- [x] Update `get_runtime()` to return `&Arc<WasmRuntime>` (via `&'static Arc<WasmRuntime>`)
- [x] All `get_runtime()` call sites work unchanged via `Deref` on `Arc`
- [x] Replace all `Arc::new(WasmRuntime::new())` calls (lines 475, 639, 726, 861) with `Arc::clone(get_runtime()) as Arc<dyn SystemRuntime>`
- [x] Artifact write-back (lines 645-649) stays: pipeline writes to `ctx.artifacts` (in-memory), not to the runtime VFS. The explicit copy to VFS is still needed for JS access.
- [x] Verify existing workspace tests still pass (6546/6546 passed)

#### Tests for Phase 0

- [ ] **Existing tests**: All existing WASM tests should pass unchanged (single-file rendering behavior is identical)
- [ ] **test_shared_runtime_sees_vfs_files**: Add file to global VFS, verify that the `Arc<dyn SystemRuntime>` passed to the pipeline can read it via `file_read_string()`

### Phase 1: Port `directory_metadata_for_document` to SystemRuntime

- [x] Add `runtime: &dyn SystemRuntime` parameter to `directory_metadata_for_document()`
- [x] Replace `std::fs::read_to_string(&path)` with `runtime.file_read_string(&path)`
- [x] Replace `find_metadata_file()` to use `runtime.is_file()` instead of `Path::exists()`
- [x] Update the call site in `AstTransformsStage` to pass `ctx.runtime.as_ref()`
- [x] Update existing unit tests in `project.rs` to pass `NativeRuntime` (15/15 pass, 6546/6546 workspace pass)

#### Unit tests for Phase 1

VFS-specific unit tests skipped: `WasmRuntime` only compiles on wasm32 targets. The 15 existing tests with `NativeRuntime` + `TempDir` prove the `SystemRuntime` abstraction works. VFS behavior is verified by Phase 2/3 end-to-end WASM tests.

### Phase 2: Wire `ProjectContext::discover()` into WASM `render_qmd`

- [x] Replace `create_wasm_project_context(path)` in `render_qmd()` with `ProjectContext::discover(path, runtime)`
- [x] `WasmRuntime` already implements `SystemRuntime` — `discover()` works as-is
- [x] `render_qmd_content()` and `render_qmd_content_with_options()` stay single-file (inline content, no VFS path)
- [x] Added `get_runtime_arc()` helper for pipeline `Arc<dyn SystemRuntime>`, `get_runtime()` still returns `&WasmRuntime` for direct calls

#### Tests for Phase 2 + Phase 3

Implemented as WASM end-to-end tests in `hub-client/src/services/projectContext.wasm.test.ts` (run with `npm run test:wasm`):

- [x] **renders single file without _quarto.yml**: Verifies backward compatibility
- [x] **inherits project title from _quarto.yml**: VFS has `_quarto.yml` with title, doc has none — title appears in HTML
- [x] **document title overrides project title**: Both have title — document wins
- [x] **discovers _quarto.yml from parent directories**: Nested doc at `/project/chapters/intro/doc.qmd` finds `/project/_quarto.yml`
- [x] **picks up directory metadata from _metadata.yml**: Author from `chapters/_metadata.yml` appears in HTML
- [x] **merges directory metadata hierarchy correctly**: Two `_metadata.yml` layers both contribute to output

## Out of Scope

- **`Format.metadata` merge with project config**: `Format.metadata` (a `serde_json::Value` extracted from the document's `format.<target>` block) is read by AST transforms via `ctx.format_metadata()` but is never merged with project/directory config. This is a pre-existing gap in both native CLI and WASM paths. Tracked separately in `claude-notes/plans/2026-03-04-format-metadata-merge.md`.

## Notes

- `ProjectContext::discover()` walks parent directories using `runtime.path_exists()`. The `WasmRuntime` VFS uses `/project/` prefix by default. The walk goes `/project/sub/dir/` → `/project/sub/` → `/project/` → `/` → stop. This should terminate correctly but Phase 2 tests should verify it.
- The `render_qmd_content*` functions pass content directly (not via VFS), so they don't participate in project discovery. No conflict with `source_location` injection.
- `directory_metadata_for_document` currently takes `&ProjectContext` — the runtime comes as a new explicit parameter, matching the pattern used by `ProjectContext::discover()`.
- Phase 0 artifact write-back: Need to verify whether `render_qmd_to_html` writes artifacts to the runtime's VFS or only to `ctx.artifacts`. If only to `ctx.artifacts`, the explicit write-back in `render_qmd()` lines 645-649 must stay (just using the same runtime instance now).
