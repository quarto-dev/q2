# Plan: Runtime Metadata Layer

## Overview

Add a runtime metadata layer to the `SystemRuntime` trait, allowing each execution
environment (WASM preview, native CLI, sandboxed runtime) to inject metadata into the
configuration merge pipeline. This metadata sits at the highest precedence — above
project, directory, and document layers — matching how quarto-cli handles `--metadata`
flags and `metadataOverride`.

**Motivation:** The hub-client currently uses a separate `render_qmd_content_with_options`
WASM entry point to inject `source-location: full` for scroll sync. This is a special-case
hack for what is fundamentally just metadata. By giving the runtime a proper metadata slot,
any runtime can inject arbitrary configuration without needing per-feature API additions.

**Depends on:** Nothing (standalone)
**Depended on by:** `2026-03-07-hub-client-render-qmd-switch.md`

## Merge Order After This Change

```
Precedence (lowest → highest):
1. Project top-level settings (_quarto.yml)
2. Project format-specific settings (format.{target}.*)
3. Directory _metadata.yml layers (root → leaf)
4. Document top-level settings (frontmatter)
5. Document format-specific settings (format.{target}.*)
6. Runtime metadata ← NEW
```

This matches quarto-cli where `--metadata` flags override everything including the document.

## Work Items

### Phase 1: Trait and Merge Pipeline (Rust)

- [x] Add `fn runtime_metadata(&self) -> Option<serde_json::Value>` to `SystemRuntime` trait
  - Default implementation returns `None`
  - Located in `crates/quarto-system-runtime/src/traits.rs`
  - Uses `serde_json::Value` to avoid coupling quarto-system-runtime to quarto-pandoc-types
  - Also added forwarding in `SandboxedRuntime` (`sandbox.rs`)

- [x] Update `AstTransformsStage::run` to include runtime metadata as final layer
  - In `crates/quarto-core/src/stage/stages/ast_transforms.rs`
  - Added `json_to_config_value` converter (serde_json::Value → ConfigValue)
  - After building the document layer, queries `ctx.runtime.runtime_metadata()`
  - If `Some`, flattens with `resolve_format_config` and pushes as final layer
  - Relaxed merge gate: now triggers on `has_project_config || runtime_meta_json.is_some()`

- [x] Write tests for runtime metadata merging (in `ast_transforms.rs`)
  - 6 unit tests with `MockRuntimeWithMetadata`

### Phase 2: WASM Runtime Storage

- [x] Add runtime metadata storage to `WasmRuntime`
  - In `crates/quarto-system-runtime/src/wasm.rs`
  - Added `runtime_metadata: RwLock<Option<serde_json::Value>>` field
  - Implemented `runtime_metadata()` trait method
  - Added `set_runtime_metadata` / `get_runtime_metadata` public methods

- [x] Add `vfs_set_runtime_metadata` WASM entry point
  - In `crates/wasm-quarto-hub-client/src/lib.rs`
  - Accepts YAML string, parses to serde_json::Value, validates it's a mapping
  - Returns JSON success/error response like other VFS operations
  - Passing empty string clears the runtime metadata

- [x] Add `vfs_get_runtime_metadata` WASM entry point (for debugging/testing)
  - Returns current runtime metadata as YAML string, or null if not set

### Phase 3: Tests

- [x] **Unit tests in `ast_transforms.rs`** — runtime metadata merge behavior (6 tests, all pass)

- [x] **WASM integration tests** — `runtimeMetadata.wasm.test.ts` (12 tests, all pass)
  - API: accepts YAML, clears with empty string, rejects non-mapping, rejects invalid YAML, round-trips
  - Pipeline: injects metadata, overrides document, overrides project, format-specific, no data-loc baseline, cleared metadata, works without project config

### Phase 4: Fix pre-existing bug (discovered during testing)

- [x] Fix `extract_config_from_metadata` in pampa HTML writer
  - Was looking for nested `format.html.source-location` — but after `AstTransformsStage`
    flattens metadata via `resolve_format_config`, `source-location` is at the top level
  - The nested lookup was never consistent with Pandoc (which receives flattened metadata)
  - Changed to look for top-level `source-location` only
  - Updated pampa unit tests to match
  - This bug existed since Feb 17 2026 (commit `9d25b246`) when format resolution was
    introduced, but was never caught because no test went through the full pipeline AND
    checked for `data-loc` in output

## Key Design Decisions

1. **Runtime metadata returns `Option<serde_json::Value>`** — not ConfigValue, to avoid
   coupling `quarto-system-runtime` to `quarto-pandoc-types`. Converted at the merge site
   via `json_to_config_value`.

2. **Default returns `None`** — existing runtimes (native, sandboxed) are unaffected.
   The native CLI can later populate this from `--metadata` flags.

3. **Merge gate relaxation** — currently the merge block in `AstTransformsStage` is gated
   on `ctx.project.config.is_some()`. With runtime metadata, we also merge when
   runtime metadata is present even without a project config. This is important for
   single-file renders in the hub-client.

4. **WASM entry point accepts YAML** — consistent with how `_quarto.yml` and frontmatter
   are expressed. The hub-client can set it as:
   ```
   vfs_set_runtime_metadata("format:\n  html:\n    source-location: full\n")
   ```

5. **pampa HTML writer reads top-level keys** — consistent with Pandoc, which never sees
   `format.html.*` nesting (Quarto flattens before calling Pandoc). Format resolution is
   the pipeline's responsibility, not the writer's.

## Files Modified

- `crates/quarto-system-runtime/src/traits.rs` — trait method
- `crates/quarto-system-runtime/src/sandbox.rs` — forwarding
- `crates/quarto-system-runtime/src/wasm.rs` — WASM implementation
- `crates/quarto-core/src/stage/stages/ast_transforms.rs` — merge logic + tests
- `crates/quarto-core/Cargo.toml` — moved yaml-rust2 to regular dependencies
- `crates/wasm-quarto-hub-client/src/lib.rs` — WASM entry points
- `crates/pampa/src/writers/html.rs` — fixed extract_config_from_metadata
- `hub-client/src/services/runtimeMetadata.wasm.test.ts` — WASM integration tests
- `hub-client/src/types/wasm-quarto-hub-client.d.ts` — TypeScript type declarations
