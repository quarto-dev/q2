---
date: 2026-05-13
branch: beads/bd-???-q2-preview-phase-a (TBD — sub-issue of bd-kw93)
beads: TBD (file as sub-issue of bd-kw93 after this plan is reviewed)
status: approved 2026-05-13 (Q-A1 through Q-A5 resolved); ready to file beads sub-issues and begin A.0
---

# `q2 preview` — Phase A (engine-less CLI skeleton)

## Goal

Ship the smallest end-to-end vertical slice of `q2 preview` that
proves the architecture in
[`2026-05-11-q2-preview-epic.md`](2026-05-11-q2-preview-epic.md) works:

```
user runs `q2 preview foo.qmd`
  → CLI spins up an ephemeral quarto-hub server with a temp data_dir
  → opens a browser at the served URL
  → the q2-preview SPA loads, connects via samod ws to the hub
  → the SPA renders foo.qmd (and the rest of the project) through
    WASM
  → user edits foo.qmd on disk → FileWatcher syncs → SPA re-renders
    incrementally
  → Ctrl-C tears it all down, deletes the temp dir
```

**Out of scope for Phase A:** engine execution (any document with
`{r}` / `{python}` / `{ojs}` code cells renders the source as-is in
this phase; Phase C wires replay). PDF, multi-window, sharing, freeze
— all later.

## What changed since the epic's Phase A draft

The epic (2026-05-11) anticipated A.3 as "add a `hub-client` preview-
mode build target." bd-hfjj (2026-05-13) decomposed hub-client into
shared workspace packages and created `q2-preview-spa/` as a sibling
workspace package — exactly the SPA the preview CLI needs. So A.3
**becomes**: "fill in `q2-preview-spa/src/main.tsx` enough that the
served bundle actually drives a render."

The `crates/quarto-preview/` build.rs pattern (A.2 + A.4) is
unchanged — `quarto-trace-server/build.rs` remains the precedent.

## Phasing inside Phase A

Seven sub-tasks (A.0 → A.6) plus an e2e smoke (A.7), each executable
independently. Each ends in a green `cargo xtask verify` (now 11
steps after bd-hfjj). The TDD policy applies — tests precede every
implementation step.

### A.0 — Lift the WASM JS bridge to `@quarto/wasm-js-bridge`

(Decision: Q-A1 option (c) — make this a proper workspace package
*now* rather than carry a duplicate or symlink forward.)

**Why this is first:** A.3 (filling in the SPA) requires the SPA's
Vite root to host `/src/wasm-js-bridge/*` (the WASM module's
`raw_module = "/src/wasm-js-bridge/sass.js"` path resolves
project-root-relative). Doing it as the first sub-task means A.3
inherits a clean answer.

**Tests first:**
- Existing hub-client `test:wasm` suite continues to render the
  changelog and run the smoke-all fixtures unchanged (the bridge
  was the dynamic-import target wasmRenderer.ts used post-Phase 5;
  if alias resolution is wrong, sass compilation breaks and
  hub-client's theme/smoke wasm tests fail).
- New `ts-packages/wasm-js-bridge/src/sass.test.ts` (light) —
  imports the package's `sass.js` and asserts `jsSassAvailable` /
  `setVfsCallbacks` are functions. Just guards the public API
  against accidental rename or removal.

**Implementation:**
- Create `ts-packages/wasm-js-bridge/`:
  - `package.json` (name `@quarto/wasm-js-bridge`, `private: true`,
    `type: "module"`, no build scripts — the files are loaded
    directly by Vite at each consumer).
  - `src/sass.js`, `sass.d.ts`, `cache.js`, `cache.d.ts`,
    `fetch.js`, `template.js` — `git mv`'d from
    `hub-client/src/wasm-js-bridge/`.
- Register the new workspace in root `package.json`'s
  `workspaces`.
- Consumer wiring — `hub-client/vite.config.ts`,
  `hub-client/vitest.{,integration,wasm}.config.ts`, and
  `q2-preview-spa/vite.config.ts` all add:
  ```ts
  alias: {
    '/src/wasm-js-bridge': path.resolve(__dirname,
      '../ts-packages/wasm-js-bridge/src'),
  }
  ```
  (For hub-client the path is `../ts-packages/...`; for the SPA
  it's `../ts-packages/...` from `q2-preview-spa/`.) `wasm-bindgen`'s
  `raw_module = "/src/wasm-js-bridge/sass.js"` is unchanged — the
  alias rewrites where `/src/wasm-js-bridge/` resolves at consumer
  build time.
- Same alias added to `ts-packages/preview-renderer/vitest.integration.config.ts`
  (already has a similar one pointing at hub-client's copy from
  Phase 4 — retarget it to the new package). Drop the hub-client
  fallback.
- Remove `hub-client/src/wasm-js-bridge/` (after `git mv`'s done
  its work — the directory should be empty).

**Acceptance:**
- `cargo xtask verify` green on all 11 steps. Especially
  `hub-client test:wasm` (which exercises the bridge through real
  sass compilation) and `preview-renderer test:integration`.
- The dir `hub-client/src/wasm-js-bridge/` no longer exists.
- The bridge files have only one home in the workspace.

### A.1 — Wire up the empty `q2 preview` clap subcommand

**Tests first:**
- `crates/quarto/tests/preview_cli.rs` — invokes `quarto preview --help`
  via `assert_cmd`, asserts the args list (`[path]`, `--port`,
  `--no-browser`, `--data-dir`, `--preview-dir`) and an exit code 0.
  Confirms the subcommand is registered before any of it does anything.

**Implementation:**
- New `crates/quarto/src/commands/preview.rs` mirroring `commands/hub.rs`'s
  shape (`PreviewArgs` struct + `execute(args) -> Result<()>`).
- `execute()` is a stub that prints "preview not implemented yet" and
  exits — A.5 wires it to actually boot.
- Add `Preview(...)` variant to the `Command` enum in `main.rs`; route
  to `commands::preview::execute`.

**Acceptance:** `quarto preview --help` shows the expected args.
`quarto preview` exits 0 with the stub message. `cargo xtask verify`
green.

### A.2 — Create `crates/quarto-preview/` shell crate

A new workspace crate that owns the CLI logic + the embedded SPA +
preview-specific axum routes. `commands/preview.rs` in the `quarto`
binary becomes a thin shim that builds `PreviewConfig` and calls
`quarto_preview::run(config)`.

**Tests first:**
- `crates/quarto-preview/tests/smoke.rs` — constructs a `PreviewConfig`
  with `--no-browser` and a known port; spawns `run()` on a tokio
  runtime; awaits the "server listening" log line via a channel; hits
  `GET /` over HTTP; asserts 200 and that the body looks like the SPA's
  `index.html` (contains `<div id="root">`). Tears the server down.

  This is the "fewest-moving-parts integration test" — it covers
  routing, embedding, and lifecycle without engines or the browser.

**Implementation:**
- `crates/quarto-preview/Cargo.toml` — depends on `quarto-hub`,
  `include_dir`, `axum`, `tokio`.
- `crates/quarto-preview/src/lib.rs` — `pub async fn run(config:
  PreviewConfig) -> Result<()>` that:
  1. constructs `HubConfig` (project mode by default; standalone if
     `--no-project` per A.5),
  2. builds the hub's axum router via existing `quarto_hub` API,
  3. layers a preview-specific fallback that serves the embedded SPA
     bundle on any non-API path,
  4. spawns the server.
- `crates/quarto-preview/build.rs` — copies trace-viewer's pattern
  *exactly*: look for `q2-preview-spa/dist/index.html`; if present,
  embed; otherwise emit a placeholder into `OUT_DIR` saying "run
  `cargo xtask build-q2-preview-spa`."
- `include_dir!("$QUARTO_PREVIEW_EMBED_DIR")` in lib.rs.

**Acceptance:** smoke test above passes. `cargo build -p quarto-preview`
succeeds with or without `q2-preview-spa/dist/` existing.

### A.3 — Fill in `q2-preview-spa/src/main.tsx`

The placeholder created in bd-hfjj Phase 6 just renders
`<PreviewErrorOverlay>` with static text. It needs to become a real
preview host. Minimum viable shape:

```
main.tsx
├── reads indexDocId + wsUrl from a <meta> tag (server emits these)
├── calls initWasm()  ──── @quarto/preview-runtime
├── connect(wsUrl, indexDocId, ...) ──── @quarto/preview-runtime
│      → loads project files into the VFS via the existing
│        wasmRenderer.vfsAdd* callbacks
├── picks an "active file" from the URL hash or first .qmd in the
│   project
└── renders <Q2PreviewIframe> with the active file's content,
    re-rendering on automerge changes.
```

This is the "engine-less render" path. Code cells appear as source
(matching Phase A's scope). Phase C will wire `EngineCapture` here.

**Tests first:**
- `q2-preview-spa/src/main.integration.test.tsx` (new) — mounts the SPA
  with a mock SyncClient (from `@quarto/preview-runtime/test-utils/mockSyncClient`)
  and a mock WasmRenderer (from `@quarto/preview-runtime/test-utils/mockWasm`),
  verifies it renders a basic .qmd through `<Q2PreviewIframe>`. Mirrors the
  approach hub-client uses for its preview pane integration tests.
- Vitest's `vitest.integration.config.ts` for q2-preview-spa, configured
  with the same WASM aliases that bd-hfjj already proved work in the
  preview-renderer + preview-runtime packages.

**Implementation:**
- New `src/services/connection.ts` in q2-preview-spa that wraps
  `@quarto/preview-runtime`'s `connect()` and exposes file state to
  the React tree (`useSyncedFiles()` hook or similar — the simpler
  shape, not a full Editor-level subscription).
- `main.tsx` mounts a tiny `<PreviewApp>` component:
  - if WASM not ready: render `<FallbackView>` with "Initializing…"
  - if connection error: render `<PreviewErrorOverlay>` with the error
  - otherwise: render `<Q2PreviewIframe astJson={...} currentFilePath={...}>`
    where the AST comes from a `renderPageInProject()` call on every
    relevant automerge change.
- Bridge resolution is taken care of by A.0 — the SPA's
  `vite.config.ts` aliases `/src/wasm-js-bridge` to
  `@quarto/wasm-js-bridge`'s src. No per-consumer copy of the bridge
  files.

**Acceptance:**
- The integration test passes.
- `cd q2-preview-spa && npm run dev` — manually point a browser at
  `localhost:5175` (with a static `<meta>` tag for testing). The SPA
  loads, WASM inits, the placeholder file renders.
- End-to-end via A.5 once the CLI boots.

### A.4 — Build orchestration: `cargo xtask build-q2-preview-spa`

Mirror `cargo xtask build-trace-viewer` (see `crates/xtask/src/build_trace_viewer.rs`).

**Tests first:** none directly — this is plumbing. Manual:
`cargo xtask build-q2-preview-spa` succeeds; `q2-preview-spa/dist/`
exists.

**Implementation:**
- New `crates/xtask/src/build_q2_preview_spa.rs`.
- Extend `cargo xtask build-all` to chain the SPA build before
  `cargo build` so the embedded `include_dir!` picks it up.

**Acceptance:** `cargo xtask build-all` produces a `quarto` binary
that includes the real SPA bundle (verifiable by `quarto preview`
serving the dashed-filename `index-<hash>.js` from the SPA's
prod build).

### A.5 — `q2 preview` boots end-to-end

This is the integration step that ties A.1–A.4 together.

**Tests first:**
- `crates/quarto-preview/tests/boot.rs` — spawns `q2 preview --no-browser
  --port 0` against a fixture project (tempdir with a single `foo.qmd`
  containing `# Hello`); asserts:
  - The server's HTTP endpoint serves the SPA's `index.html`.
  - The hub's websocket endpoint accepts a samod handshake.
  - The temp `data_dir` is created at startup and *deleted* on
    shutdown (drop the runtime; check the dir is gone).
  - The launch URL the CLI computes is shaped `http://<host>:<port>/#/preview/<indexDocId>`
    (URL-fragment carrier, per Q-A3).

**Implementation:**
- `commands/preview.rs::execute()` becomes:
  1. Resolve `path` arg (default = cwd if it has `_quarto.yml`,
     otherwise current file).
  2. Construct a temp `data_dir` via `tempfile::TempDir` (so it's
     `Drop`-deleted on shutdown automatically).
  3. Build `HubConfig` in project mode (or standalone if `--no-project`).
  4. Call `quarto_preview::run(config).await`.
  5. On the first "server listening" log, compute the launch URL as
     `http://<host>:<port>/#/preview/<indexDocId>` and optionally
     open it in the browser (unless `--no-browser`).
  6. Ctrl-C handler that stops the server and lets `TempDir` clean up.
- **No server-side HTML rewriting.** The SPA reads `indexDocId` from
  `window.location.hash` (matching the existing `#/share/...` route
  pattern in `hub-client/src/utils/routing.ts`). The WebSocket URL is
  derived client-side from `window.location` —
  `ws://<host>:<port>/ws`. See Q-A3 below.

**Acceptance:** the boot test passes. Manual run: `quarto preview` in
a Quarto project pops a browser tab, the preview pane renders,
`Ctrl-C` cleans up. Per CLAUDE.md §End-to-end verification, record
the inspection.

**Status (bd-mflk, 2026-05-13): done.** End-to-end verified against
the real binary via Chrome DevTools — `# Hello, q2 preview!` +
paragraph content render inside `<Q2PreviewIframe>` with the
compiled Bootstrap theme applied; on-disk edits propagate to the
iframe within ~2 s.

The original A.5 work expanded once the binary was driven for the
first time. Documented here for posterity (these surfaced gaps the
plan didn't anticipate):

- **A.5.4b — doc-id prefix.** `/health` returns the bare samod doc
  id (e.g. `4ByAxLmG…`); `@quarto/preview-runtime`'s `connect()`
  expects automerge-repo's `automerge:<id>` form. PreviewApp's
  `fetchIndexDocId` now normalizes. Mirrors how hub-client's
  `App.tsx` normalizes `shareRoute.indexDocId` (App.tsx:287-290).
  Plan deviation: dropped URL-fragment indexDocId carrier (Q-A3
  resolution) in favour of `/health` because pre-binding the
  listener to thread the id through the CLI would have required
  refactoring the hub's startup; the `/health` route already
  exposes `index_document_id` and runs without auth in preview
  mode.

- **A.5.4c — `render_page_for_preview` WASM entry.** A bare-
  markdown document with no `format:` key detected as `html`,
  causing the WASM dispatch to take the HTML pipeline and return
  `{ html: … }` instead of `{ ast_json: … }`. The user's
  intent for `quarto preview` is "default `html` → render through
  q2-preview"; added a new `#[wasm_bindgen]` entry point that
  applies that mapping (and refactored `render_*_to_response` to
  take a `prefer_preview_format: bool`). Hub-client's
  `render_page_in_project` path is unchanged so its dispatch on
  YAML-declared format is preserved.

- **A.5.4d — cold-start peer race + multi-entry Vite build.**
  Two bugs uncovered together:
  1. `quarto-sync-client.connect()`'s 1 ms `waitForPeer` is too
     aggressive when there is no IndexedDB cache to fall back to.
     `findDoc()` then fired `handle.request()` before the samod
     handshake completed, the synchronizer saw `#peers` empty,
     and immediately marked the index doc unavailable. Added an
     optional `peerTimeoutMs` parameter (default preserved for
     hub-client); the q2-preview SPA passes 5000 ms.
  2. `Q2PreviewIframe` loads `/q2-preview.html` as a sandboxed
     renderer host. The SPA's fallback was serving `index.html`
     for that path → recursive SPA load. Added a separate Vite
     rollup input + the matching `q2-preview.html` + entry stub,
     mirroring `hub-client/vite.config.ts`'s pattern.

- **A.5.4e — theme styling.** `Q2PreviewIframe` already owns the
  blob-URL + `UPDATE_THEME` plumbing for compiled theme CSS, but
  it depends on the parent passing `themeFingerprint`. PreviewApp
  now threads `result.theme_fingerprint` through, three-way:
  string ⇒ post, absent ⇒ explicit clear (`null`), failed render
  ⇒ leave (`undefined`) so transient errors don't strip styling.
  Also: `q2-preview-spa/index.html` had `min-height: 100vh` on
  `#root` only; the embedded iframe collapsed to its intrinsic
  content height and clipped longer documents. Forced
  `html/body/#root { height: 100% }`.

### A.6 — Manual force-refresh button

Per the epic's resolution #4 (force-refresh invariant): the preview
UI *always* offers a "re-render now" button as the user's escape
hatch when the dependency-graph misses a cross-doc relationship.

**Tests first:**
- `q2-preview-spa/src/components/ForceRefresh.integration.test.tsx`
  (new) — renders the button, clicks it, verifies the mocked
  `renderPageInProject` is called.

**Implementation:**
- A small persistent control in the SPA's chrome (corner of the
  iframe; doesn't intrude on the rendered content).
- Click handler re-invokes the SPA's render path against current
  automerge state. No server roundtrip in Phase A (engines are out of
  scope); Phase C extends to trigger server-side re-execution when
  applicable.

**Acceptance:** test passes; manual button click in `npm run dev`
re-runs the render.

### A.7 — End-to-end smoke test (Playwright)

The integration tests above cover the pieces. A.7 covers the
**human-shaped path**.

**Tests first:**
- `q2-preview-spa/e2e/basic-preview.spec.ts` (new) — Playwright
  config that spawns `quarto preview` against a test fixture, opens
  the served URL in chromium, asserts:
  - The preview renders (DOM contains expected `# Hello` text).
  - Editing the fixture `.qmd` on disk produces a visible content
    change in the iframe within 2s.
  - The force-refresh button works.
  - The Q2-preview format's DOM-stability invariant holds across an
    edit: a `data-stable-id` element keeps the same DOM node identity
    (using Playwright's `evaluate()` to grab the node pre- and
    post-edit and `===` compare them).

**Acceptance:** the e2e test runs in CI (extends to `--include-e2e`
shape on `cargo xtask verify`).

## Tradeoffs / Open questions

### Q-A1 — bridge files duplication vs symlink

**Decided 2026-05-13: option (c)** — lift to `@quarto/wasm-js-bridge`
workspace package now. Captured as sub-task A.0 above.

Wiring detail: each consumer's vite config aliases
`/src/wasm-js-bridge` to the new package's `src/`. The Rust WASM
module's `raw_module = "/src/wasm-js-bridge/sass.js"` annotation
stays unchanged; the alias controls where consumers resolve that
path. No per-consumer shim files needed.

### Q-A2 — How aggressively to extract `<PreviewApp>` shape

main.tsx in A.3 is described as "tiny" with an inline `<PreviewApp>`.
Could grow. Alternative is to ship `<PreviewApp>` as a *reusable
component* in `@quarto/preview-renderer` (so hub-client could
eventually also share this orchestration logic). That's a bigger
move than A.3 needs, but it would mean hub-client and the SPA share
not just the rendering primitives (Phase 4 of bd-hfjj) but also the
orchestration (state, connection, dispatch).

Lean: **keep `<PreviewApp>` inside q2-preview-spa for Phase A.** If
hub-client decides it wants to share orchestration too, that's a
later refactor.

### Q-A3 — How does the SPA learn `indexDocId` + `wsUrl`?

**Resolved 2026-05-13 (after source-code audit invalidated the
original premise):** the SPA reads `indexDocId` from
`window.location.hash`. `wsUrl` is derived client-side from
`window.location` as `ws://<host>:<port>/ws`. The CLI just opens
`http://<host>:<port>/#/preview/<indexDocId>` in the browser.

History: the epic plan (and my original Phase A draft) claimed
"reads from a `<meta>` tag emitted by the server (same trick share
links use)." That was wrong on both counts — hub-client share links
use URL fragments, not meta tags, and there's no meta-injection
anywhere in this codebase. The audit traces:

- `hub-client/src/App.tsx:272` — routes on `route.type === 'share'`
  with `#/share/...` URL fragments.
- `hub-client/src/utils/routing.ts` — fragment parser.
- `crates/quarto-trace-server/src/lib.rs:185-187` — serves the
  embedded `index.html` *unmodified*; SPA reads its config from a
  Vite-build-time env var.

URL-fragment carrier wins because: zero server-side rewriting (no
new axum middleware), matches the existing share-link pattern, and
fragments are the conventional place to carry client-side route
state in SPAs.

### Q-A4 — Browser-open behavior

Default: open `http://localhost:<port>/` in the system default browser
after the server reports "listening." Override with `--no-browser`.

Lean: use the `open` crate (small, cross-platform). The trace-server
does something similar (check).

### Q-A5 — Initial sync time on large projects

For a project with 500+ files, `HubContext::new` pushes them all into
automerge at startup. The blank-screen wait is real. Phase A doesn't
need to solve this, but should *measure* it — a smoke test on a
30-file fixture is fine, but record what 500 looks like.

Lean: time the boot on a representative fixture; if it's >2s,
file a perf follow-up for Phase B.

**Future direction (user note, 2026-05-13):** worth considering
persistent samod storage *inside `.quarto/`* (e.g.
`.quarto/preview/samod/`). On startup, sync the existing
filesystem-backed store against the project (fresh-create when
missing). This turns the "blank screen at boot" into a one-time
cost per project rather than every invocation. Not a Phase-A
concern — note here so the perf numbers from this phase inform the
decision.

## Beads issues to file after plan approval

Sub-issues of bd-kw93, in execution order:
1. **(A.0)** Lift WASM JS bridge to `@quarto/wasm-js-bridge`
   workspace package; switch hub-client + the SPA (and preview-renderer
   tests) to alias-resolve through it.
2. **(A.1 + A.2)** `quarto preview` CLI shim + `crates/quarto-preview/`
   shell crate with the trace-server-shaped `include_dir!` +
   `build.rs` placeholder.
3. **(A.3)** Fill in `q2-preview-spa/src/main.tsx` so the SPA boots
   through samod + WASM and drives a real render.
4. **(A.4)** `cargo xtask build-q2-preview-spa` + wire it into
   `build-all`.
5. **(A.5)** End-to-end boot integration test (`q2 preview`
   spins everything up against a fixture project).
6. **(A.6)** Manual force-refresh button.
7. **(A.7)** Playwright smoke test in `q2-preview-spa/e2e/`.

## Reference

- Epic: `claude-notes/plans/2026-05-11-q2-preview-epic.md`
- Decomposition (just landed): `claude-notes/plans/2026-05-11-hub-client-decomposition.md`
- CLI subcommand model: `crates/quarto/src/commands/hub.rs`
- `include_dir!` precedent: `crates/quarto-trace-server/{build.rs, src/lib.rs}`
- `cargo xtask build-trace-viewer` model: `crates/xtask/src/build_trace_viewer.rs`
- Hub server entry: `crates/quarto-hub/src/server.rs`
