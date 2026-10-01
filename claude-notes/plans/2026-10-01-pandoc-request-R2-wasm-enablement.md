# Plan: wasm enablement and exports (pandoc-request R2)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-request-epic.md`](2026-10-01-pandoc-request-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D1, D3, D8.7)
**Depends on:** R1. **Unblocks:** host H3 and H5, R3, R4, R5, R6, R7, R8.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** R (workspace-6). **Start gate:** R1 is fully ticked on `feature/pandoc-wasm`, both STOPs included. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** R3 (its Start gate decides whether it can begin).

## Overview

Make the code `prepare()` needs compile for `wasm32`, expose the request and its companions from `wasm-quarto-hub-client`, and prove it end to end at the Rust and wasm-test level. Ends with a working docx request produced inside the hub's wasm.

## Decisions

- The HTML stage list minus `PANDOC_STAGE_EXCLUDED` becomes an ungated prefix builder; the native tail stays native-only (design: Target architecture).
- Bytes cross the wasm boundary as `Uint8Array` built with `js_sys`; every current export returns a JSON `String` and `serde-wasm-bindgen` is not a dependency (it would turn `Vec<u8>` into number arrays). Request bytes are copies, never views of wasm memory (a view cannot be transferred), and the host treats a posted request as consumed.

## Checklist

### Tests first
- [ ] The prefix builder's stage names equal the old pandoc list minus the tail; the exact-name tests (`pandoc_stage_excluded_names_exist_in_html_pipeline`, `t6_2_pandoc_stage_list_produces_exact_surviving_name_list`) are updated.
- [ ] The in-memory bundle's file set equals the disk-extracted set.
- [ ] A hub-client vitest (`hub-client/src/services/*.wasm.test.ts` pattern) calls the built `render_pandoc_request` and asserts `files[i].bytes instanceof Uint8Array` and that the decoded golden deep-equals the wire object.
- [ ] Resolver, format table and classify export: first key of a format map, `_quarto.yml` override, unknown format; classify gives `Q-20-3` carrying `json_path` on non-zero exit (omitting "input JSON retained at" for a wasm-sourced request, since that path is virtual) and `Q-11-1` on warnings.

### Tasks
- [ ] **Write the export surface first:** the `.d.ts` stubs in `ts-packages/preview-runtime/src/wasm-quarto-hub-client.d.ts` for every export below, with names, arguments and the `render_pandoc_request` response envelope `{success, error?, diagnostics[], stats: {unexecuted_cells}, request?}` (the request as a JS object carrying `Uint8Array`s, the rest JSON-compatible; a document with errors, or an active path absent from the VFS, returns no request). `async render_pandoc_request(path, format, source_date_epoch: Option<f64>, capture_gz_json: Option<Vec<u8>>, typst_available_fonts: Option<Vec<String>>)` (`source_date_epoch` is seconds as an `f64`, cast in Rust, because an `i64` crosses wasm-bindgen as a BigInt and a JS number would throw) takes the capture input the way `parse_capture_from` does for the preview render (`lib.rs:~1458-1490`); R3 only wires it. Done when H3 and H5 can code against the stubs.
- [ ] **Real wasm32 compile blockers.** Discovered by compiling: ungate, run `cargo check --target wasm32-unknown-unknown`, record the real error list; the known ones are below. Serial with the next two tasks (all touch `pipeline.rs`, `pandoc_write.rs` and `pandoc_filters/bundle.rs`).
  - `bundle.rs` imports the native-only `ResourceError` (`bundle.rs:32`; `resources.rs:347,523`): give it its own error type or ungate it (ungating `resources::embedded` needs `tempfile` moved from the native-only to the shared dependencies; it compiles for wasm32; an error type of its own in `bundle.rs` avoids both).
  - `pandoc_write.rs` calls the `pub(crate)` helpers `typst_compile::{resolve_font_paths, brand_for_mode, with_brand_file_fonts, with_google_font_cache, string_array, font_path_args, discover_available_typst_fonts}` in a native-only module (`stages/mod.rs:95-96,134`): move the pure ones out of the native-only module. `typst_compile` cannot simply be ungated: `typst-gather` pulls `openssl-sys` (via `typst-kit` and `native-tls`), which does not build for wasm32, so only the pure helpers move and `TypstCompileStage` stays native-gated (ungating the module otherwise leaves its items as dead code under `-D warnings`).
  - `diagnostics` ungates trivially (`mod pandoc_write`, `mod bundle` and `mod diagnostics` are all still gated); `format_defaults` is already ungated.
  - `std::fs`/`Command` compile on wasm32 and fail only at runtime, but `stage_typst_brand_fonts`'s `pollster::block_on(fetch_url)` (`:335`) would hang on wasm, so it must be cfg-gated.
  - Done when `cargo check --target wasm32-unknown-unknown` passes in `crates/wasm-quarto-hub-client` (a trial ungate needed about six edits over three error layers; the check takes ~3 minutes cold and seconds warm). Follow `.claude/rules/wasm.md` (cfg guards without `test`; `#[async_trait(?Send)]` on new stages; `execute()`/`Command` stay `#[cfg(not(target_arch = "wasm32"))]`; none of the four files in the "update `wasm_lua.rs`" rule change).
- [ ] **Split the stage-list builder.** `build_pandoc_pipeline_stages` (`pipeline.rs:561`) pushes native-only `PandocWriteStage`/`ResourceCopyFlushStage`/`TypstCompileStage` (imported under cfg at `:59-67`), so it cannot simply be ungated. Add an ungated `build_pandoc_prefix_stages(fmt, captures)` (the HTML list minus `PANDOC_STAGE_EXCLUDED`, taking captures so the capture splice, `insert_capture_splice_stage` at `:694`, can apply in R3) and keep the native tail. The pause/finishing builders (`:655, :671`) stay native-only here; the wasm fallback `partial_stage_list` (`:1275`) stays HTML-only. On wasm, typst gets no `ResourceCopyFlushStage` or `TypstCompileStage`.
- [ ] **In-memory bundle** (after the blockers; same files). `bundle` returns `(relative_path, bytes)` iterators over the embedded statics (recursive: `include_dir::Dir::files()` is top-level only, use `entries()`/`find`; `FORMATS_DOCX_DIR` sits inside `FORMATS_DIR`, so dedupe); native keeps a disk-extracting wrapper. The mount layout is exactly native's `extract_share_tree` output (`filters`, `pandoc/datadir`, `formats/docx`); the epub snippets and typst template are reached through `files` entries (R4, R5) and typst packages and fonts are a separate export (R4), so a docx worker never mounts them. `share_tree_version` is the R1 definition (D1).
- [ ] **`resource_refs` for docx** (after the blockers; `pandoc_write.rs`). The purpose-built computation inside `prepare()` (D3; bytes are collected only when `collect_resources`), for the docx subset: resource-path images (AST walk), `reference-doc`/`template` and `highlight-style` (`.theme`/`.xml`). User entry-point Lua filters with their containing directory (extensions arrive as project files, D3). Entry-point filters of type `Json` are external executables and cannot run in wasm: they yield a diagnostic. Path rules per D3 (allowed root: the project root; absent file). Images resolve as pandoc does (percent-decoded, `default-image-extension` appended). A filter's containing directory mounts recursively unless the filter is at the project root, where only the filter file itself mounts (D3); a `require` or `io.open` of any other file from a root-level filter is the documented limitation. Mounted bytes count against the size limits. Collection stops at the size limits from the constants file with a diagnostic naming the file, so the main thread does not allocate a request the host will reject. Unit tests per key.
- [ ] **Exports** in `wasm-quarto-hub-client`, after the tasks above:
  - `render_pandoc_request` (single-doc branch `render_single_doc_to_response`, `lib.rs:1513`, already takes `format_override` at `:1532/:1538`; the logic lives in a `quarto-core` function with a thin wasm wrapper; it is tested natively with `NativeRuntime` over a tempdir outside `/tmp` (wasm-mode requests reject `files` and `resource_refs` under `/tmp`, and Windows temp paths are `X:/`) and the `vfs_root` resolver, since `WasmRuntime` is wasm32-only, and the wrapper only by the vitest);
  - the share-tree export;
  - a completion-classify export taking `(success, status, stderr)`;
  - the supported-format table and the project-aware format resolver (D8.7), built on `ProjectContext::discover`.

  The `WasmModuleExtended` interface and wrappers go in `wasmRenderer.ts`; state whether `q2-preview-spa` (same package) is affected. A document inside a `_quarto.yml` project (the branch is `project.is_single_file`, `lib.rs:~1325`) returns a "projects not yet supported" error until R7 stage 0, so H5 can handle it.
- [ ] **Check the `vfs_root` resolver.** The hub prelude installs it (`lib.rs:~1573`) and native pandoc rendering does not (`render_to_file.rs:~380`); confirm it does not rewrite links (`link_rewrite.rs:~413`) differently from native for pandoc formats.
- [ ] **Measure** (after the export exists): the Rust wasm size delta from the share tree (D1; note `TYPST_TEMPLATE_DIR` and the 2.5 MB `TYPST_PACKAGES_DIR` for later), against R0's baseline. The cost of mounting ~250 files into a fresh instance is a host measurement, taken in H3.

## Verification

Crate-scoped clippy/nextest for `quarto-core` and the wasm crate; the wasm compile check after every cfg-touching task; `npm run build:wasm` at phase end; the hub-client vitest; one workspace nextest at phase end.

## Exit

A docx request is produced inside the hub wasm. Host H3 can now run its dev harness and parity net; R3, R4, R6 and R7 can start.

## Close-out

- [ ] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [ ] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [ ] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: _none yet_
- Last commit; tasks ticked: _none_
- State and gotchas: _none_
- Next step: _the first unticked task_
