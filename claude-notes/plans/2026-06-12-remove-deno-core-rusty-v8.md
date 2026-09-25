# Remove deno_core / rusty_v8 from quarto-system-runtime

**Strand:** bd-3e3sam51 (discovered-from bd-c6l13j79; was blocked by
bd-kuxzj8su, now closed)
**Created:** 2026-06-12
**Status:** in progress

## Overview

`quarto-system-runtime` embeds deno_core (→ rusty_v8) solely to implement
the JS template surface (`js_available` / `js_render_simple_template` /
`render_ejs`), which existed for EJS project scaffolding. After
bd-kuxzj8su (commit `3a35de43`), `quarto-project-create` renders with
quarto-doctemplate and the JS template path has **zero callers on every
target**. The dependency costs:

- v8 prebuilt archives are \~100MB downloads at build time;
- rusty_v8 publishes no musl prebuilts — it blocked the static-musl
  release legs outright (both linux legs of the v0.1.0 dry-run 404'd; see
  run 27449454203 and PR #280, whose gnu fallback exists only because of
  rusty_v8);
- every native binary links V8 it never calls.

## Removal surface (verified 2026-06-12)

**Rust — `crates/quarto-system-runtime`:**
- `src/js_native.rs` — the only deno_core consumer in the tree (embeds
  `js/dist/simple-template-bundle.js` + `js/dist/ejs-bundle.js`)
- `src/native.rs` — `use crate::js_native::JsEngine`, the three trait-method
  impls, and their unit tests (`test_js_available`,
  `test_js_render_simple_template*`, `test_render_ejs*`)
- `src/traits.rs` — the JS EXECUTION section (three default methods)
- `src/sandbox.rs` — decorator forwarding of the three methods
- `src/wasm.rs` — the `raw_module = "/src/wasm-js-bridge/template.js"`
  extern block (`jsRenderSimpleTemplate` / `jsRenderEjs` /
  `jsTemplateAvailable`) and the three trait-method impls
- `Cargo.toml` — `deno_core`, `serde_v8`, `tokio` (no `tokio::` call sites
  in the crate; it was there for deno_core — confirm via compile)
- `js/` — entire directory (esbuild EJS/simple-template bundle package;
  not part of npm workspaces, no external references)

**Rust — workspace root `Cargo.toml`:** `deno_core` and `serde_v8`
workspace deps (no other crate references them).

**Rust — `crates/wasm-quarto-hub-client/src/lib.rs`:** `JsTestResponse` +
`test_js_simple_template` / `test_js_ejs` / `test_js_available` wrappers
(no TS callers anywhere; only `.d.ts` declarations).

**TypeScript:**
- `ts-packages/wasm-js-bridge/src/template.js` (no consumers besides
  wasm.rs) + the `ejs` dependency in `ts-packages/wasm-js-bridge/package.json`
- `hub-client/src/types/wasm-quarto-hub-client.d.ts` — `test_js_*` lines
- `ts-packages/preview-runtime/src/wasm-quarto-hub-client.d.ts` —
  `test_js_*` lines, **plus** stale `create_project(): Promise<string>`
  typing missed in bd-kuxzj8su (now returns `string`)
- `npm install` to sync the root lockfile after dropping `ejs`

**Docs:** `claude-notes/plans/js-execution-performance.md` describes the
per-call V8-isolate design — mark obsolete with a pointer here.

**Keep:** `sandbox.rs` itself (Lua sandboxing; "Deno-style" is just the
permission-model name), the sass/cache/fetch bridge files, the SASS trait
surface, reqwest/grass deps.

## Work Items

### Phase 0: "Before" measurements

- [x] Recorded at pre-removal HEAD (`67b61ef3`), clean
      `cargo clean && cargo build --bin q2` (dev profile = optimized +
      debuginfo), macOS arm64:
      - wall **64.92 s** (user 880.32 s, sys 56.75 s)
      - `target/debug/q2` **203,267,120 bytes (\~194 MB)**
      - `target/debug` total **5.7 GB**
      - Cargo.lock: **721 packages**, 5 deno/v8-related
        (v8, deno_core, serde_v8, deno_ops, deno_error)
      - (v8 prebuilt archive was already cached locally; cold-cache builds
        additionally download \~100MB)

### Phase 1: Guard test first (TDD)

- [x] Added `test_no_v8_in_workspace_lockfile` to
      `quarto-system-runtime/src/lib.rs` asserting the workspace
      `Cargo.lock` contains no `v8`, `deno_core`, or `serde_v8` package
      entries.
- [x] Verified it **fails** at current HEAD (2026-06-12): fails on `v8`
      as expected.

### Phase 2: Removal

- [x] Removed the Rust surface (js_native.rs, trait methods + design-doc
      section in traits.rs, native impls + their unit tests, sandbox
      forwarding, wasm externs + impls, crate deps deno_core/serde_v8/tokio,
      js/ dir). Stale pampa `filters`-feature comment updated.
- [x] Removed workspace-root `deno_core` / `deno_web` / `deno_webidl` /
      `serde_v8` deps (deno_web/webidl were already consumer-less).
      Workspace `tokio` kept — pampa/quarto-lsp\*/quarto use it.
- [x] Removed wasm-quarto-hub-client `test_js_*` wrappers + `JsTestResponse`.
- [x] Removed `template.js` + `ejs` dep from wasm-js-bridge; updated both
      `.d.ts` files (incl. the stale `create_project: Promise<string>`
      typing in preview-runtime); `npm install` synced the lockfile (`ejs`
      now dev-only, pulled by unrelated dev tooling).
- [x] Marked `js-execution-performance.md` obsolete.
- [x] Guard test passes; `cargo tree -p quarto -i v8` → no match;
      Cargo.lock **721 → 655 packages (−66)**; repo-wide grep for the JS
      template surface finds nothing.

### Phase 3: Verification

- [x] `cargo build --workspace` + `cargo nextest run --workspace` —
      9954/9954 pass (2026-06-12).
- [x] Full `cargo xtask verify` — all steps pass (2026-06-12).
- [x] WASM vitest suite (`npm run test:wasm`) — 103/103 pass including
      `projectCreate.wasm.test.ts`: project creation works with the JS
      bridge gone.

### Phase 4: "After" measurements

- [x] Same protocol as Phase 0 (clean `cargo build --bin q2`, dev profile,
      macOS arm64), measured 2026-06-12:

      | Metric | Before | After | Δ |
      |---|---|---|---|
      | Clean-build wall | 64.92 s | 57.21 s | **−12 %** |
      | Clean-build CPU (user) | 880.32 s | 783.77 s | −11 % |
      | `target/debug/q2` size | 203.3 MB | 142.0 MB | **−61.3 MB (−30 %)** |
      | `target/debug` total | 5.7 GB | 5.0 GB | −0.7 GB |
      | Cargo.lock packages | 721 | 655 | **−66** |

      Plus: cold-cache builds no longer download the \~100 MB v8 prebuilt
      archive, and the rusty_v8 musl blocker on release targets is gone.

### Phase 5: Handoff

- [x] Filed bd-h7s7bsbk: revisit static-musl linux release targets
      (PR #280's gnu fallback) now that rusty_v8 is gone;
      discovered-from bd-3e3sam51.
- [x] Close bd-3e3sam51 with measurements + follow-up pointer.

## Notes

- The `SystemRuntime` trait keeps its `Send + Sync` bounds and the SASS
  surface; only the JS EXECUTION section goes away. The historical reason
  for fresh-V8-isolate-per-call (not Send+Sync) disappears with it.
- `tokio` removal from quarto-system-runtime is expected to be safe (no
  `tokio::` call sites); if the compile disagrees, find the real consumer
  before deciding.
