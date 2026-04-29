# wasm-quarto-hub-client

WASM module for the hub-client live-preview render pipeline.
Exposes `render_qmd`, `render_qmd_content`, `render_page_in_project`
(Phase 9), `parse_qmd_to_ast`, and the VFS bridge.

## VFS state contract (Phase 9)

The synthetic VFS at `/.quarto/project-artifacts/...` is **load-bearing
across renders**:

- `WebsiteProjectType::post_render` flushes Project-scoped artifacts
  (theme CSS, shared JS, fonts) to that path on every render.
- The iframe post-processor reads those entries back from VFS when
  it sees `href="/.quarto/..."` or `src="/.quarto/..."` in the
  rendered HTML. The browser never makes a network request for
  these — they're served out of `WasmRuntime::file_read`.
- The orchestrator's Pass-1 profile cache lives in IndexedDB, not
  VFS. Clearing VFS does **not** invalidate that cache; it just
  deletes the rendered artifacts the post-processor depends on.

**Do not call `vfs_clear` between renders.** Safe call sites:
session disconnect, project switch, end-to-end test teardown.
See the doc-comment on `vfs_clear` in `src/lib.rs` and Phase 9
plan §Decision 7.

## Render entry points

- `render_qmd(path, user_grammars)` — single-document render against
  whatever's in VFS at `path`. Predates Phase 9; kept as a stable
  surface for tests/examples that don't need project context.
- `render_page_in_project(path, user_grammars)` — Phase 9 entry
  point. Discovers `_quarto.yml` from the active path; falls
  through to the single-doc path when no project ancestor exists,
  otherwise drives `ProjectPipeline<RenderToHtmlRenderer>` with
  `RenderMode::ActivePage(path)` so Pass-1 builds a full project
  index but Pass-2 renders only the active page.
- `render_qmd_content(content, template_bundle, user_grammars)` —
  path-less render for callers like the about-page changelog.

## Build

This crate is built via `npm run build:wasm` from `hub-client/`,
not via `cargo build --target wasm32-unknown-unknown`. The npm
script handles wasm-bindgen invocation and pkg/ output layout the
hub-client TypeScript code expects.

To rebuild during development:
- `cd hub-client && npm run build:wasm` — release WASM build.
- `cargo xtask verify` — full Rust + WASM + hub-client tests.
- `cargo xtask verify --skip-hub-build` — skip WASM rebuild
  (useful when only Rust-side changes and no signature changes
  cross the wasm-bindgen boundary).
