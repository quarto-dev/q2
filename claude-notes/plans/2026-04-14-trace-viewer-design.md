# Trace Viewer & Analysis Tooling — Design Plan

## Overview

Quarto 2 already writes pipeline traces to `.quarto/trace/<filename>/latest.json`
(see `2026-04-13-pipeline-tracing.md`, Phase 3 complete). This plan covers
**Phase 4 and beyond**: how users and coding agents actually **consume** those
traces.

The design problem is organized along a 2×2 matrix of entry points. After the
decisions below:

|                | **Viewer (interactive)**                             | **Analyzer (programmatic/CLI)**                                    |
|----------------|------------------------------------------------------|--------------------------------------------------------------------|
| **Native**     | `quarto trace view` → SPA in browser (Phase 4.3)     | `quarto trace list` / `quarto trace show` — JSON-only (Phase 4.2)  |
| **hub-client** | SPA embedded in hub-client panel (Phase 4.4)         | **Deferred** — MCP blocked on sync-server auth (Phase 4.5)         |

The shape: build the viewer as a **TypeScript SPA** vendored into the `quarto`
binary via `include_dir!` and served by a local `axum` server; hub-client
reuses the same SPA components, reading traces from its Automerge-backed VFS.
The CLI analyzer is a separate code path in Rust that reads the same on-disk
JSON through the shared `quarto-trace` crate.

## Goals

1. First-iteration viewer for a single `(document, stage)` snapshot — no diffs.
2. CLI-centric analyzer that coding agents and power users can script against.
3. Shared trace-consumption code between native and hub-client targets.
4. A forward path to the Quarto 1 diff experiences (across-stage, across-document)
   without designing ourselves into a corner.

## Non-Goals (first iteration)

- JSON diffs in any UI surface.
- History browsing across time (only `latest.json`).
- Editing/replaying traces.
- Trace viewing for multi-document project renders (single-doc first).

## What We Have

- `JsonTraceObserver` writes `{pipeline: [{stage, index, data_kind, data, duration_ms}], total_duration_ms}` to `.quarto/trace/<doc>/latest.json` (`crates/quarto-core/src/stage/trace.rs`).
- `data` includes full Pandoc AST JSON for `DocumentAst` entries, full HTML for `RenderedOutput`, markdown for `DocumentSource` / `ExecutedDocument`, etc.
- Transforms inside `AstTransformsStage` are recorded as `transform:<name>` entries.
- Activation is metadata-driven (`trace: true` or `trace: "summary"`).

## Design Questions (all resolved)

### D1. Trace schema (resolved)

- **New crate `quarto-trace`** holds the typed schema with `serde` derives. Thin dependencies (only `serde`, `serde_json`, and what's needed to reference pandoc AST types if we type `data` rather than leaving it as `serde_json::Value`). Both the writer (`JsonTraceObserver` in `quarto-core`) and readers (CLI analyzer, viewer backend, MCP tools) depend on this crate.
- **`schema_version: 1`** as a top-level integer field.
- **Top-level shape**:
  ```json
  {
    "schema_version": 1,
    "render": {
      "input_path": "...",
      "output_path": "...",
      "format_target": "html",
      "started_at_unix_ms": 1799200496000.0,  // ms since Unix epoch
      "git_hash": "abc1234",      // or "abc1234-dirty"
      "total_duration_ms": 123.4
    },
    "pipeline": [
      { "stage": "parse", "index": 0, "data_kind": "...", "data": {...}, "duration_ms": 1.2 },
      { "stage": "engine-execution", "index": 1, "status": "error", "error": {...} },
      { "stage": "render-html-body", "index": 2, "status": "skipped" }
    ]
  }
  ```
  - The `pipeline` array *is* the pipeline description — no separate `pipeline_stages` descriptor.
  - Each entry carries an optional `status` (`"ok"` default, `"error"`, `"skipped"`, reserved for future `"conditional-skipped"` etc.). On error, the previous stage's `data` is still in the trace, so the state immediately before the failure is recoverable.
  - Errored / skipped stages still appear so the pipeline shape stays observable even on failure.
- **Git hash via a hand-rolled `build.rs`** in `quarto-trace`, exposing `QUARTO_GIT_HASH` via `env!`. Implementation: a \~15-line script invoking `git rev-parse --short=7 HEAD` and `git status --porcelain`, with a `-dirty` suffix when the working tree is dirty and `unknown` when `git` is not available. Revisited from the original "use vergen" intent after observing that the script has zero build-time dependencies, matches the spec exactly, and behaves identically in CI (`.git` is present). Caveat unchanged: tarball / `cargo package` builds without `.git` fall back to `unknown`; a cached fallback can be added later if needed.
- **Single git hash for native and WASM.** Same source tree → same hash. No need to distinguish build artifacts at the schema level.

### D2. Viewer architecture (resolved)

**Decision: `quarto trace view` only.** No `quarto preview` integration (Q2 doesn't have `preview` yet). Static-export (Q1-style base64 HTML) and TUI are both deferred — can be added later without schema/UX changes.

**Shape:**
- New `trace-viewer/` directory, sibling to `hub-client/`. Vite SPA, no collaborative/sync concerns.
- SPA assets vendored into the `quarto` binary at build time via `include_dir!` (pointed at `trace-viewer/dist/`).
- `quarto trace view` starts a localhost HTTP server with two kinds of routes:
  - `GET /*` → serves the vendored SPA bundle from the embedded `include_dir` tree.
  - `GET /api/traces` → lists `.quarto/trace/*/latest.json` on the filesystem.
  - `GET /api/trace/<doc>` → serves the trace JSON for a given doc.
- Opens the default browser on the server URL.

**Build ordering — the real design problem.** `include_dir!` is a proc macro evaluated when the Rust crate compiles, so `trace-viewer/dist/` must exist first. Options:
  - **(preferred)** Extend `cargo xtask verify` and add a `cargo xtask build-trace-viewer` that runs `npm run build` in `trace-viewer/` before any Rust build that embeds it. Same pattern as hub-client.
  - **Dev-mode escape hatch**: an env var (e.g. `QUARTO_TRACE_VIEWER_DIR=/path/to/trace-viewer/dist`) or feature flag that swaps `include_dir!` for on-disk serving, so UI iteration doesn't require Rust rebuilds.
  - **Fresh-clone safety**: the crate that embeds the SPA must fail a clear error if `trace-viewer/dist/` is missing, pointing the user to the xtask. `include_dir!` on an empty directory silently produces nothing, which would be confusing.

**Server crate choice — decided: `axum` 0.8.** Already in the workspace via `quarto-hub` (`crates/quarto-hub/Cargo.toml:24`), along with full tokio + tower ecosystem. Using anything else would add a second HTTP stack for no benefit. `include_dir` is also already a workspace dep (`Cargo.toml:47`, used by `quarto-core`, `quarto-sass`, `wasm-quarto-hub-client`, `qmd-syntax-helper`) — no new dependencies needed for the embed story either.

**Watchability**: out of scope. Traces are treated as immutable snapshots; `quarto preview` will eventually own the file-watching / incremental-update story for rendering.

### D3. CLI analyzer surface (resolved)

Minimum viable surface, three subcommands only:

```
quarto trace list                     # list available traces under .quarto/trace/
quarto trace show [--doc X] [--stage Y]  # print a trace or a single stage entry
quarto trace view [--doc X] [--port N]   # launch the SPA
```

- **All console output is JSON**, always. No separate `--json` flag. Output is meant for machine consumption and LLM prompts; humans who want a pretty view use `quarto trace view` or pipe to `jq`/`fx`.
- **No `trace export`**: the trace files under `.quarto/trace/` already *are* the shareable artifacts.
- **No `--summary` flag**: we can instruct agents about the JSON schema directly, or build summarization tooling after real-world usage tells us what summaries are useful.
- **No filtering flags, no `stages`, no `extract`**: defer until actual usage demonstrates the need. `jq` covers most of this against the raw output of `trace show`.
- **No diff subcommand**: deferred to Phase 4.6 (see below).

This keeps the first CLI ship minimal. Any pressure to grow the surface waits on real-life usage of the viewer and traces.

### D4. Agent-oriented entry points (resolved)

1. **CLI** — covered by `trace list` / `trace show` (D3). JSON-by-default output is directly consumable by agents that shell out. This is the entire agent surface for the first ship.
2. **MCP tools on `quarto-hub`** — deferred. Blocked on a separate issue: MCP can't be used on hub-client's main deployment right now because the sync server is behind an auth check. Revisit after that's resolved.
3. **LSP custom requests** — deferred. Hard to test right now because the extension has very little content; not worth designing without a real consumer.

### D5. First-iteration viewer UX (resolved)

Minimum viable viewer screens:

1. **Trace list** — picker if multiple `.quarto/trace/*/latest.json` exist.
2. **Pipeline timeline** — horizontal strip of stages with timings, `data_kind` badges, click to inspect. Chrome-DevTools-"Performance"-style but per-stage.
3. **Stage detail** — for the selected stage, render the `data` payload:
   - `DocumentAst` → interactive AST tree (collapsible blocks/inlines, search, copy-as-JSON, copy-as-markdown-fragment).
   - `DocumentSource` / `ExecutedDocument` → plain markdown in a monospaced pane. **No syntax highlighting** (we don't have a good `.qmd` highlighter yet). **No side-by-side diffing** — all diff UI is deferred to Phase 4.6.
   - `RenderedOutput` → **syntax-highlighted HTML source only**, no rendered preview. A real preview would need to resolve CSS / JS / supporting-file paths, which is out of scope; when this lives inside `quarto preview` later, the real rendered output is available there.
   - `LoadedSource` → metadata only (path, size, type).

   Every stage detail view includes a **"copy JSON" button** that puts that entry's payload on the clipboard. No dedicated raw-JSON tab — users who need the full trace for `jq` read `.quarto/trace/<doc>/latest.json` directly.

**Component sharing**: resolved by D6 — build trace-viewer-specific components inside the top-level `trace-viewer/` workspace. Shared extraction (hub-client's AST rendering ↔ trace-viewer's AST tree) is left to the future TS-monorepo reorganization, once actual shared surface materializes.

### D6. Code sharing between native and hub-client (resolved)

**Data-loading abstraction.** Define a `TraceSource` interface in TS with two implementations:
- `HttpTraceSource` — `fetch('/api/trace/...')` against the local `quarto trace view` server.
- `VfsTraceSource` — reads from hub-client's VFS / Automerge document (Phase 4.4).

The SPA components take a `TraceSource` as a prop, so the UI is identical across native and hub-client.

**SPA location: top-level `trace-viewer/` directory, sibling to `hub-client/`.** Reasoning:

1. **The repo is already an npm-workspaces monorepo** (root `package.json` declares workspaces; CLAUDE.md mandates `npm install` from the root). The choice isn't "monorepo vs not" — it's "sibling workspace vs nested workspace."
2. **Build DAG is acyclic either way.** `wasm-quarto-hub-client` (cargo, `wasm32` target) → hub-client TS build (needs WASM) + trace-viewer TS build (independent) → `cargo build --workspace` (native; `include_dir!` embeds `trace-viewer/dist/`) → `quarto` binary. The two cargo invocations target different architectures, so there's no cargo-level circularity — only orchestration, which `cargo xtask build-all` (Phase 4.0) already owns.
3. **Sibling layout decouples failure domains.** A broken hub-client TS build (its production build is stricter than `tsc --noEmit` per CLAUDE.md) shouldn't block `trace-viewer/dist/` and therefore the Rust build of the `quarto` binary. Nesting under `hub-client/` couples them.
4. **Workspace config is unambiguous.** Sibling = one entry in the root `package.json` workspaces array. Nesting requires either declaring `hub-client/trace-viewer` from the root (a layout that suggests a hierarchy that doesn't exist semantically) or nested workspaces inside `hub-client/package.json` (works, but indirection).
5. **Forces deliberate sharing.** A nested `trace-viewer` would tempt accidental imports from hub-client's WASM-coupled modules, creating a hidden dependency on WASM rebuilds. A sibling forces shared code to go through an explicit shared package when (and only when) it actually materializes.
6. **Repo shape consistency.** Top-level siblings already include `crates/`, `hub-client/`, `docs/`. Adding `trace-viewer/` matches the existing pattern.

The TS-monorepo reorganization tracked under the "Future" section handles shared component extraction (e.g., a `packages/shared-trace-ui/`) *if and when* real shared code materializes — not speculatively.

### D7. hub-client analyzer (deferred, with one safety invariant)

Full design deferred — we'll revisit once MCP on hub-client is unblocked (D4) and the native viewer (Phase 4.3) has real-world usage to inform what surface matters.

**One hard requirement we must respect now**, so that deferring doesn't paint us into a corner:

> **Setting `trace: true` (or `trace: "summary"`) in a document's metadata must not crash hub-client.**

Concretely, the current `JsonTraceObserver` writes to `.quarto/trace/<doc>/latest.json` via `std::fs` (see `crates/quarto-core/src/stage/trace.rs:140`). In the hub-client WASM build there's no OS filesystem — `std::fs::create_dir_all` / `File::create` will fail at runtime. We need any of:

- A WASM-safe no-op observer swap when running in the WASM target (simplest; traces are silently not captured in hub-client for now).
- A VFS-backed writer that stores traces into hub-client's Automerge-backed file tree (preferred longer-term; natural fit with `VfsTraceSource` from D6).
- An in-memory observer whose output can be fetched by the JS bridge and handed to the `TraceSource`.

**Phase 4.1 must pick one of these** (likely the no-op, with a `tracing::warn!` breadcrumb) so that WASM renders with `trace: true` remain correct. Anything richer is part of the deferred Phase 4.4 / 4.5 design.

### D8. Write amplification / trace size (resolved)

`JsonTraceObserver` dumps full AST at every stage — MBs per render for moderate documents. Schema concerns are already addressed by D1's `schema_version` field: any future entry-shape change (e.g. delta-encoded entries) bumps to `schema_version: 2` and readers gate on it cleanly. No work needed now to keep that door open.

Planned progression, in order of when we'll reach for it:

1. **Gzip `latest.json` → `latest.json.gz`** when raw file size becomes noticeable. Cheap; schema-neutral.
2. **JSON-Patch delta encoding** between adjacent stages for `DocumentAst` entries, introduced under `schema_version: 2`. This storage shape parallels what we'll want for the diff UI in Phase 4.6, so design synergy.

Gzip + JSON-patch deltas should cover us for a long while before anything more exotic (structural sharing, columnar, etc.) becomes worth considering.

**No opt-in heavy stages.** `trace: true` captures everything by default. Eventually we'll expose a structured form in metadata for selective capture (e.g. `trace: { stages: [ast-transforms, render-html-body] }`, sketched in the tracing plan); that's a separate future feature, not a prerequisite for shipping.

## Ideas Worth Considering (from related systems)

- **React DevTools "Profiler"** — commit-by-commit diffs where each commit is like a stage; highlighting what subtree changed is the key UX affordance. Strongly suggests we eventually want node-level change highlighting in the AST tree, not just JSON-patch dumps. Shapes the Phase 4.6 diff UI.
- **Language-server / `quarto preview` inline inspection** — "hover any block in my document to see which pipeline stage last modified it" (or last touched its source range). Natural home inside hub-client and the future `quarto preview`, since both have a live document to hover over. Requires propagating source locations through transforms, which we're already investing in — so there's genuine synergy rather than speculative design. Captured here as the target experience we're building toward.
- **Performance-oriented tracing** (consolidated). Fine-grained timing per stage and per transform is straightforwardly addable on native builds (`std::time::Instant` already used in `trace.rs`); the WASM build is rate-limited by browser `performance.now()` resolution (quantized to \~100µs for security, depending on the site's isolation headers) so we only promise coarse timings there. Presentation ideas worth revisiting once we have real data: Chrome DevTools-style per-stage performance timeline (already the basis for the pipeline-timeline view in D5); flame-chart inspection of engine execution à la Tracy / Perfetto if the engine step ever becomes complex enough to warrant it.

## Proposed Iteration Plan

### Phase 4.0 — Build infrastructure (prerequisite)
- [x] Add `cargo xtask build-all` that runs the full fresh-build sequence in dependency order. Sources of truth: it should match what CI does on a clean checkout. Initially covers hub-client (WASM + TS) and the existing Rust workspace; extended in 4.3 to include `trace-viewer`. *(`crates/xtask/src/build_all.rs`)*

### Phase 4.1 — Foundations (no UI)
- [x] Create `quarto-trace` crate with typed `TraceDocument`, `RenderInfo`, `TraceEntry`, `StageStatus` (`Ok`/`Error`/`Skipped`/`Unknown` for forward compat), `schema_version: 1`.
- [x] Hand-rolled `build.rs` in `quarto-trace` (no build-deps) exposing `QUARTO_GIT_HASH` (with `-dirty` suffix when applicable). Pivoted from `vergen` after realizing the script is 15 lines and avoids a new build-time dep tree.
- [x] Migrate `JsonTraceObserver` in `quarto-core` to emit via the typed schema. Top-level `render` object and per-entry `status` added. Module gated to native targets via `#![cfg(not(target_arch = "wasm32"))]`.
- [x] Record errored stages via `on_stage_error` (`status: "error"` + `error: {message: ...}`). `Skipped` reserved for future conditional stages but not yet emitted by any stage.
- [x] Populate `RenderInfo`: `input_path` from initial input, `format_target`/`git_hash` at activation, `output_path` from FinalOutput, `started_at_unix_ms` at pipeline start, `total_duration_ms` at completion/error.
- [x] `quarto-trace` exposes a reader API (`read_trace`, `list_traces`) used by both CLI and viewer backend.
- [x] Tests: round-trip through disk, forward-compat for unknown status/fields, legacy-default-to-Ok, build-hash populated. All 5 pass.
- [x] **WASM safety invariant**: `JsonTraceObserver`/`SummaryTraceObserver` cfg-gated to native; WASM branch of `activate_trace_from_metadata` emits a `Warn` event and retains the no-op observer. Confirmed via successful `npm run build:wasm` in hub-client.

Schema tweak during implementation: `started_at` changed from RFC3339 string to `started_at_unix_ms: f64` to avoid pulling a date-formatting crate; viewers format via `new Date(ms).toISOString()`.

### Phase 4.2 — CLI analyzer (`quarto trace`)
- [x] `quarto trace list` — JSON output of available traces under `.quarto/trace/`, with `--trace-dir` override.
- [x] `quarto trace show [--doc X] [--stage Y]` — JSON output of the full trace or a single stage entry. Ambiguity errors guide users to `--doc`.
- [x] `quarto trace view` — stub subcommand that bails with a clear message (Phase 4.3 will implement).
- [x] `list_value` / `show_value` helpers return `serde_json::Value` so tests and future MCP tools can consume the logic without parsing stdout.
- [x] Integration tests in `crates/quarto/tests/trace_cli.rs` — 8 tests covering list, show (full + single stage + errored stage), ambiguity, unknown stage, and empty-root cases. All pass.
- [x] End-to-end smoke test against the real `q2` binary confirmed.

### Phase 4.3 — Native viewer SPA (single doc, single stage)
- [x] Create `trace-viewer/` Vite SPA (sibling of `hub-client/`, no collaborative/sync code). Added as root npm workspace.
- [x] `TraceSource` abstraction in TS (`trace-viewer/src/trace-source.ts`); `HttpTraceSource` impl talking to the local server. 5 vitest tests.
- [x] Trace list + pipeline timeline + stage detail views (`src/components/*.tsx`, `src/App.tsx`). 3 vitest tests for App.
- [x] AST tree component (collapsible, searchable) — see `src/components/AstTree.tsx`.
- [x] Plain monospaced text view for `DocumentSource` / `ExecutedDocument` (`TextView.tsx`).
- [x] Syntax-highlighted HTML source view for `RenderedOutput` (`HtmlSourceView.tsx`, highlight.js with xml/html).
- [x] Metadata panel for `LoadedSource` (via `StageDetail` switch on `data_kind`).
- [x] Per-stage "copy JSON" button (`CopyJsonButton.tsx`).
- [x] `cargo xtask build-trace-viewer` orchestrating `npm run build`; wired into `cargo xtask verify` (steps 8–9) and Phase 4.0 `build-all` picks up `trace-viewer/` automatically.
- [x] New `quarto-trace-server` crate (`crates/quarto-trace-server/`) that uses `include_dir!("$QUARTO_TRACE_VIEWER_EMBED_DIR")` to embed `trace-viewer/dist/` and exposes the `axum` routes. 6 integration tests covering `/api/traces`, `/api/trace/<doc>`, path traversal rejection, SPA index serving, and SPA fallback.
- [x] `QUARTO_TRACE_VIEWER_DIR` dev-mode escape hatch: when set, serves SPA assets from disk instead of the embedded bundle.
- [x] Fresh-clone safety: `build.rs` generates a placeholder `index.html` in `OUT_DIR` when `trace-viewer/dist/` is missing and emits a `cargo:warning=...` directing users to `cargo xtask build-trace-viewer`. The build still succeeds so downstream work isn't blocked.
- [x] `quarto trace view` subcommand: starts the server on `127.0.0.1:<port>` (port `0` picks OS-assigned), serves traces from `./.quarto/trace/` with `--trace-dir` override. Verified end-to-end via `curl` against a live server: `/api/traces`, `/api/trace/<doc>`, `/`, and SPA fallback routes all return the expected content.
- [x] `--no-browser` flag accepted; auto-open not yet wired (browser can be opened manually at the printed URL).
- [ ] (optional 4.3b) SSE endpoint for auto-refresh on `.quarto/trace/` changes — out of scope (D2 decision).

Workspace state after 4.3: `cargo build --workspace` clean, `cargo nextest run --workspace` → 7265 passed, `hub-client npm run build:wasm` clean, `trace-viewer npm run build` → 226 KB JS / 72 KB gzipped.

### Phase 4.4 — hub-client viewer
- [ ] `VfsTraceSource` impl reading from hub-client VFS.
- [ ] Wire WASM pipeline to write traces into the VFS under `.quarto/trace/`.
- [ ] Mount the SPA components in a hub-client panel.
- [ ] Command-palette entry.

### Phase 4.5 — Analyzer for agents (deferred)
Blocked on a separate auth concern: MCP can't run against hub-client's main deployment because the sync server is gated behind an auth check. Revisit once that's unblocked. Candidate tools when we return: `trace_list`, `trace_show`, plus diff tools if 4.6 has shipped.

### Future — TS monorepo reorganization (tracked, not scheduled)

Once `trace-viewer/` ships alongside `hub-client/`, we'll have two independent Vite/React apps with overlapping dependencies (and likely overlapping components — AST tree views, JSON viewers, etc., per D6 `TraceSource` abstraction). Convert the `hub-client/` + `trace-viewer/` pair into a proper TS monorepo (npm workspaces — already in use at the repo root — or pnpm/turbo if we want a build graph). Benefits: shared component library, single `node_modules`, coordinated dep bumps. Tracked here as a follow-up to Phase 4.4; defer until the shared-surface-area story is concrete.

### Phase 4.6 — Diffs (deferred, explicitly out of scope for first ship)
- [ ] Stage-to-stage diff (JSON Patch).
- [ ] Document-to-document diff at same stage.
- [ ] UI: side-by-side + inline-on-tree.
- [ ] CLI: `quarto trace diff ...`.

## Decisions Summary

All originally-open questions resolved:

1. **Schema crate placement** (D1) → new `quarto-trace` crate; `schema_version: 1`; typed `TraceDocument` / `RenderInfo` / `TraceEntry` / `StageStatus`; git hash via `vergen` with `-dirty` suffix.
2. **Viewer architecture** (D2) → `quarto trace view` only (no preview integration, no static export, no TUI); `axum` server + `include_dir!`-embedded SPA; immutable traces, no file-watching.
3. **CLI surface** (D3) → `trace list` / `trace show` / `trace view`, JSON-only output, no `export` / `summary` / `stages` / `extract` / `diff` subcommands in first ship.
4. **Agent entry points** (D4) → CLI only for now; MCP deferred on sync-server auth; LSP deferred on lack of consumer.
5. **Viewer UX** (D5) → pipeline timeline + stage detail; plain text for markdown-ish stages, syntax-highlighted HTML source for rendered output, metadata panel for loaded sources; per-entry "copy JSON" button; no rendered HTML preview, no raw-JSON tab.
6. **SPA location** (D6) → top-level `trace-viewer/` sibling to `hub-client/`; `TraceSource` interface with `HttpTraceSource` and `VfsTraceSource` implementations; shared components extracted later via future TS-monorepo reorganization.
7. **hub-client analyzer** (D7) → full design deferred; must uphold one invariant: `trace: true` in hub-client does not crash the render (WASM-target no-op observer with `tracing::warn!`).
8. **Trace size** (D8) → no action now beyond versioned schema; gzip when it starts to matter; JSON-Patch deltas under `schema_version: 2` co-designed with Phase 4.6 diff UI; `trace: true` captures everything by default.

Remaining open work is sequencing rather than design: the phased plan below (4.0 → 4.6) is the source of truth.

## References

- Prior plan: `claude-notes/plans/2026-04-13-pipeline-tracing.md`
- Current implementation: `crates/quarto-core/src/stage/trace.rs`, `crates/quarto-core/src/stage/observer.rs`
- Q1 viewer (for reference, not reuse):
  - `external-sources/quarto-cli/src/resources/tools/ast-tracing/trace-viewer.qmd`
  - `external-sources/quarto-cli/src/resources/tools/ast-tracing/edit-distance.ts`
  - `external-sources/quarto-cli/src/command/dev-call/show-ast-trace/cmd.ts`
- Subcommand registration: `crates/quarto/src/commands/mod.rs`
- hub-client MCP server: `crates/quarto-hub/`
