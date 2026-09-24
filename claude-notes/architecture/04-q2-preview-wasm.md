# Diagram 4 — q2 vs hub-client: Build Chain & WASM-in-Binary

**SVG:** [`q2-preview-wasm.svg`](./q2-preview-wasm.svg) · **Set index & conventions:** [`README.md`](./README.md)

Companion diagrams: [Render pipeline](./01-pipeline.md) ·
[Crate & package map](./02-crates.md) ·
[hub-client Automerge structure](./03-hub-client-automerge.md).

---

## How to read this

Same three-tier drill-down (**diagram → guide → source**). This diagram covers
the relationship between the native `q2` command and the `hub-client` TypeScript
distribution, the **multi-step build** that produces the `q2` binary, and the
key nesting: `q2 preview` is a native command that runs a **WASM build of the
engine itself**. Numbered markers ①② point to the [Notes](#notes).

## The core idea

`q2 preview` starts a local web server that serves an embedded single-page app.
That SPA renders with `wasm-quarto-hub-client` — the **same `quarto-core`/`pampa`
engine compiled to `wasm32`**. So the native `q2` binary **ships a WASM build of
itself** alongside its native engine. That one fact drives the workspace layout
(the engine must stay native + WASM-clean) and the build chain below.

## The build chain (produces the `q2` binary)

Three steps, each consuming the previous artifact. (Full rationale:
`claude-notes/instructions/preview-spa-rebuild.md`.)

| Step | Command | Does | Output artifact |
|---|---|---|---|
| 1 | `cd hub-client && npm run build:wasm` | `cargo build --target wasm32-unknown-unknown --release` on `crates/wasm-quarto-hub-client` (with `-Zbuild-std=std,panic_unwind`), then `wasm-bindgen --target web` | `crates/wasm-quarto-hub-client/pkg/` — `…_bg.wasm` + JS glue |
| — | *(import)* | `pkg/` is imported by `@quarto/preview-runtime` + `@quarto/preview-renderer` | — |
| 2 | `cargo xtask build-q2-preview-spa` | runs `npm run build` (`tsc -b && vite build`) in `q2-preview-spa/` (a Vite/React SPA depending on those two packages) | `q2-preview-spa/dist/` — `index.html` + JS + bundled `.wasm` |
| 3 | `cargo build --bin q2` | `crates/quarto-preview/build.rs` sets `QUARTO_PREVIEW_EMBED_DIR` to `q2-preview-spa/dist`; `crates/quarto-preview/src/lib.rs` embeds it: `static EMBEDDED_SPA = include_dir!("$QUARTO_PREVIEW_EMBED_DIR")` | the `q2` binary, SPA baked in |

Build-chain source: `hub-client/scripts/build-wasm.js`,
`crates/xtask/src/build_q2_preview_spa.rs`,
`crates/quarto-preview/build.rs`, `crates/quarto-preview/src/lib.rs`.

## Anatomy of the `q2` binary

One native executable that contains **two builds of the engine**:

- **Native engine** — `quarto-core` + `pampa` compiled natively. Used by `q2 render` (and `q2 preview`\'s server-side re-execution).
- **Embedded SPA** — `q2-preview-spa/dist/` baked in via `include_dir!`, which bundles `wasm-quarto-hub-client_bg.wasm` = `quarto-core` + `pampa` compiled to `wasm32`. Used by `q2 preview`\'s in-browser rendering.

See Note ① for why this is two builds rather than one, and Note ② for the
stale-WASM trap this creates.

## `q2 preview` runtime

`crates/quarto/src/commands/preview.rs` + `crates/quarto-preview/src/lib.rs`:

1. `q2 preview <path>` resolves the project/initial page, probes a port, opens the browser.
2. Boots an **ephemeral** hub server: `quarto_hub::server::run_server_with(...)` with state in a `tempfile::TempDir` (throwaway per run), bound to loopback, `HubConfig.register_root_ws = false` (the SPA owns `/`).
3. The server routes:
   - `/` (+ unknown paths) → `spa_handler` serving `EMBEDDED_SPA` (the baked-in `dist/`).
   - `/ws` → the **samod Automerge sync** endpoint (ephemeral, in the TempDir).
   - `/api/preview/re-execute`, `/api/preview/deps`, `/api/preview/diagnostics` → preview-specific routes (`extend_with_preview`).
4. In the browser: the SPA loads the `.wasm`, connects to `/ws` (Automerge), mirrors files into the WASM **VFS**, and renders via the WASM pipeline — exactly the flow in [diagram 3](./03-hub-client-automerge.md).
5. Native side: a file watcher re-executes engines and records **capture documents** the WASM render replays (so engines don't run in the browser).

## Three roles, one server + schema

| Role | What it is | Server | UI | Rendering |
|---|---|---|---|---|
| `q2 render` | native CLI render | none | none | native engine (diagram 1) |
| `q2 preview` | native CLI live preview | **embedded** ephemeral hub (loopback, TempDir) | embedded SPA | WASM in browser |
| `hub` (bin) | standalone collab server | persistent, all interfaces, optional auth | none (serves clients) | clients render |
| `hub-client` | collaborative web app | needs an external server | full editor SPA | WASM in browser |

All four share `quarto-hub::server` (HTTP/WS), the Automerge schema
([diagram 3](./03-hub-client-automerge.md)), and the engine
([diagram 2](./02-crates.md)). `q2 preview` is the unusual one: a *native*
binary that bundles a *WASM* renderer and an ephemeral sync server.

---

## Notes

### ① Why two builds of the same engine — *detail*

The native engine can't run in a browser sandbox, and the WASM engine can't do
native file I/O or run subprocess engines. So `q2 preview` keeps the heavy/native
work (file watching, engine execution → capture documents) on the native side,
and does *rendering* in WASM so it shares the exact React preview stack with
`hub-client`. The price is shipping the engine twice (native + `wasm32`) and the
discipline that `quarto-core`/`pampa` and everything they touch must compile to
both targets — the I/O seam is `quarto-system-runtime` (see
[diagram 2](./02-crates.md)) and the async-trait rule is `.claude/rules/wasm.md`.

### ② `cargo build --bin q2` does NOT rebuild the WASM — *amber*

The three build steps are **not** chained automatically. A plain
`cargo build --bin q2` re-embeds whatever is already in `q2-preview-spa/dist/`;
it does not re-run steps 1–2. After Rust engine changes, `q2 preview` will
silently serve a **stale** WASM image — tests pass, the render path looks
correct, but the preview iframe runs pre-change code. To refresh, run the full
chain (steps 1→2→3). `cargo xtask verify` (without `--skip-hub-build`) runs
steps 1–2; step 3 is still manual.
→ `CLAUDE.md` ("Verifying Rust changes in `q2 preview`"),
`claude-notes/instructions/preview-spa-rebuild.md`. Documented incident:
2026-05-20 stale-WASM preview.
