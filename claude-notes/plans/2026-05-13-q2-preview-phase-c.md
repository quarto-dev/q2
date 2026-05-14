# q2 preview — Phase C plan

**Epic:** bd-kw93 (q2 preview)
**Predecessor:** Phases A + B, both fully merged on `feature/q2-preview-command`.
**Date:** 2026-05-13 (sub-task issues filed 2026-05-14)
**Status:** C.3, C.1, C.4, C.2, C.5 landed 2026-05-14. C.6 (preview.engine config) and C.7 (per-doc capture cache) are next; both are parallelisable polish.

## Progress

Sub-task status. Each line tracks one filed bd-issue; check off as merged.

- [x] **C.3** (bd-kw93.1) — IndexDocument capture sidecar schema. Merged 2026-05-14.
  - [x] TS schema (`@quarto/quarto-automerge-schema`): `CaptureRef`, `captures?` on `IndexDocument`, `CURRENT_SCHEMA_VERSION` 1→2, `migrateIndexDocument` staged V0→V1→V2.
  - [x] sync-client (`@quarto/quarto-sync-client`): `CaptureRef` re-export, `onCapturesChange?` callback, `getCapturesFromIndex` / `notifyCapturesIfChanged` wired at connect / index-change / createProject / disconnect.
  - [x] Rust mirror (`crates/quarto-hub/src/index.rs`): `CaptureState`, `CaptureRef`, get/set/has/remove/get_all_captures.
  - [x] WASM signature widen — *moved to C.4* (would have been a half-finished parameter without the dispatch).
- [x] **C.1** (bd-kw93.2) — Server-side first-time eager capture. Merged 2026-05-14.
  - [x] `crates/quarto-core/src/engine/preview_record.rs` exporting `record_capture(path, project, runtime, registry) -> Result<Option<EngineCapture>>`. Sub-pipeline built by truncating `build_html_pipeline_stages_with_options` at the `engine-execution` stage so future pre-engine stages flow in automatically.
  - [x] On-ready callback added to `quarto_hub::server::run_server_with`; quarto-preview drives the new `capture_driver` from it on `tokio::task::spawn_blocking` + `pollster::block_on` (pipeline futures are `?Send` per `.claude/rules/wasm.md`).
  - [x] Sidecar write: `quarto_hub::resource::create_binary_document` envelope (gzipped JSON of `EngineCapture`, MIME `application/x-engine-capture+gzip`) → `IndexDocument::set_capture` (landed in C.3).
  - [x] Unit tests: 4 in `preview_record::tests` + 5 in `capture_driver::tests` cover prose-only → None, passthrough-engine → Some, AST-serialized `input_qmd`, sidecar idempotence.
  - [x] Integration test: `tests/eager_capture.rs` drives `run_with_on_ready` end-to-end, polls the sidecar, and round-trips the binary doc back to a parseable EngineCapture.
  - [x] Binary smoke: `cargo run --bin q2 -- preview /tmp/c1-smoke --no-browser` boots cleanly, eager driver fires (no capture for prose-only fixture, as expected). Real-engine smoke (jupyter/knitr) deferred — no R/Python in CI.
  - bd-ojtq follow-up filed: `xtask create-worktree --base` default.
- [x] **C.4** (bd-kw93.3) — Browser-side replay wiring (also owns the WASM signature widen absorbed from C.3). Merged 2026-05-14.
  - [x] `render_page_for_preview` (WASM entry) takes `capture_gz_json: Option<Vec<u8>>` — the same gzipped-JSON wire format Phase C.1 writes to the capture binary doc.
  - [x] WASM internal `build_replay_registry_from` ungzips + JSON-parses + constructs `EngineRegistry::with_replay`. `render_qmd_to_preview_ast` threads it down to `build_q2_preview_pipeline_stages` → `EngineExecutionStage::with_registry`.
  - [x] TS binding: `renderPageForPreview(path, userGrammars?, captureGzJson?)`. `sync-client.getBinaryDocById(docId)` for fetching the capture binary doc by ID (it isn't in the project file index).
  - [x] SPA: `onCapturesChange` populates `state.captures`; render effect looks up the active page's `captureDocId`, fetches the binary doc, passes bytes to `renderPageForPreview`. Fall-through to default registry when absent.
  - [x] 2 new SPA integration tests pin the seam (with-capture path forwards bytes, no-capture path leaves arg `undefined`).
  - [x] cargo xtask verify all 12 steps green (Rust + WASM build + hub-client tests).
  - [ ] Real-WASM browser smoke (Playwright + live preview + hand-authored capture) — gap noted; not blocking.
- [x] **C.2** (bd-kw93.4) — Staleness detection on doc-content change. Merged 2026-05-14.
  - [x] `compute_input_qmd` in `crates/quarto-core/src/engine/preview_record.rs` — runs the q2-preview pipeline truncated at PreEngineSugaringStage and serializes the AST to QMD. Output matches what EngineExecutionStage hands the engine, so byte-equality vs the recorded capture's `input_qmd` reliably detects staleness.
  - [x] `capture_driver::recompute_staleness(ctx, runtime, rel_path)` — compares + flips `CaptureRef.staleness`. Idempotent. No-ops cleanly for missing captures + standalone-mode contexts.
  - [x] New `quarto_hub::server::OnFileChangedCallback` (fifth param of `run_server_with`). quarto-preview wires it from `run_with_on_ready`; dispatches via `spawn_blocking` + `pollster::block_on` since pipeline futures are `?Send`. Canonicalizes both project_root and the watcher path to survive macOS `/tmp` vs `/private/tmp`.
  - [x] 9 new unit tests (5 staleness + 4 canonicalization). All `cargo xtask verify --skip-hub-build` 12 steps green.
  - [x] Binary smoke against /tmp/c2-smoke confirms watcher path fires through to `on_file_changed`. Real-engine path (jupyter/knitr) not exercised in smoke; unit tests cover the toggle exhaustively against the test-passthrough engine.
  - [ ] In-process Rust integration test for the full watcher→staleness loop deferred to bd-u3ze (flaky under `cargo nextest run` on macOS; passes standalone + under `cargo test`; suspected notify-rs/FSEvents + nextest capture interaction).
- [x] **C.5** (bd-kw93.5) — Stale-capture UX overlay + `/api/preview/re-execute`. Merged 2026-05-14.
  - [x] Server: new `POST /api/preview/re-execute` in `crates/quarto-preview/src/re_execute.rs`. Validates path (400), claims in-flight slot (409), kicks off `record_capture` on a blocking worker, writes new capture binary doc + sidecar update on success. Sidecar `state: error` + `lastError` on failure.
  - [x] Hub router refactor: `build_router_with_state` returns `Router<SharedContext>` so `extend_router` can register routes that consume `State<SharedContext>`. Final `with_state(ctx)` moves into `run_server_with`.
  - [x] SPA: `<StaleCaptureOverlay />` component (top-left of preview pane). Shows when sidecar `staleness: true` / `state: running` / `state: error`. POSTs to the new endpoint; surfaces 409 + non-202 errors inline.
  - [x] Tests: 3 new Rust unit tests (200/400/409 cases), 6 new SPA integration tests (button states, click behaviour, error surfacing).
  - [x] cargo nextest run -p quarto-preview -p quarto-hub -p quarto-core: 2118/2118 pass. SPA: 16/16 integration tests pass.
  - [ ] Playwright e2e (edit cell → overlay → click → new capture renders) deferred — needs the broader e2e harness Phase D/E will introduce.
- [ ] **C.6** (bd-kw93.6) — `preview.engine: manual | auto | off` config (blocked by C.5).
- [ ] **C.7** (bd-kw93.7) — Per-doc capture filesystem cache (blocked by C.5).

Friction-related follow-ups filed during Phase C setup:
- bd-ojtq — `xtask create-worktree --base` should auto-detect/warn instead of defaulting to `main` (P3).

## Goal

Phase C is the load-bearing phase: it bridges from "the preview re-renders on file edits with no engine execution" (the state after Phase B) to "the preview correctly renders code-cell output, eagerly the first time and on explicit user request thereafter." The server runs engines; the browser replays captures via the existing WASM `ReplayEngine` machinery.

The epic plan (`2026-05-11-q2-preview-epic.md`) named seven sub-tasks (C.1–C.7); this document expands each one with seams, TDD test plans, acceptance criteria, and an explicit dependency order. All seven Open Questions (Q1–Q7) from the epic are settled — see the "What's already true" section for the standing decisions.

## What's already true

Phase A + B + the replay engine (`bd-45yw`, fully landed) give Phase C a substantial head start:

- **`EngineCapture` is a stable, serializable type** in `quarto_trace::EngineCapture { engine_name, input_qmd, result: serde_json::Value }`. `result` is a serialized `ExecuteResult` (the engine's output: markdown, supporting_files, filters, includes, needs_postprocess). Both ends — server and browser — already serialize and deserialize this type.
- **`ReplayEngine`** in `crates/quarto-core/src/engine/replay.rs` is a fully-tested `ExecutionEngine` impl that holds an `Arc<EngineCapture>`, validates input via byte-equality, and returns the recorded `ExecuteResult` on match or a hard `ExecutionError` on miss.
- **`EngineRegistry::with_replay(capture)`** is the substitution helper. It returns a default registry with the replay engine registered under whichever engine name the capture recorded. Last-write-wins replacement makes the substitution transparent.
- **`EngineExecutionStage`** already emits `EngineCapture` via the observer channel: `ctx.observer.on_auxiliary_data(..., ENGINE_CAPTURE_KIND, ...)` at `crates/quarto-core/src/stage/stages/engine_execution.rs:256`. `JsonTraceObserver` routes the payload into `TraceDocument.engine_capture`. We can re-use this seam by either reading the trace artifact or installing a new in-memory observer that captures the payload into a channel.
- **The preview SPA already calls `render_page_for_preview(path)`** (the WASM entry point) on every `contentTick` bump. The signature widening to accept an optional capture is the only WASM-side surface change C.4 needs.
- **The server-side change-handling path** (`run_file_watcher` → `ctx.sync_file(&path)` in `crates/quarto-hub/src/server.rs:1040`) is the natural hook for staleness detection and (under `preview.engine: auto`) re-execution.
- **The axum router is extensible** via `extend_with_spa` in `crates/quarto-preview/src/lib.rs:116`. The new `/api/preview/re-execute` endpoint plugs in alongside.

## Settled decisions (carried forward from the epic)

These are folded into the Phase C work below; recorded here for cross-referencing.

- **Q1 (format remap).** `RenderMode::Preview` flows through pipeline config. Already wired for Phase A's `render_page_for_preview` entry point; Phase C extends it so engines run server-side under preview mode rather than in-WASM.
- **Q2 (browser vs server).** Server runs engines only; the browser runs the full q2-preview pipeline via WASM, using `EngineRegistry::with_replay`.
- **Q3 (invalidation).** Per-document staleness *detection* only. No automatic re-execution under the default (`preview.engine: manual`).
- **Q4 (hub-client decomposition).** Already landed under bd-hfjj (Phase A pre-epic). The preview SPA imports only what it needs.
- **Q5 (project mode).** Auto-discover project; `--no-project` escape hatch exists from Phase A.
- **Q6 (formats).** HTML-only for MVP.
- **Q7 (sandboxing).** Loopback-only bind; `--insecure-allow-network` escape hatch from Phase A.
- **WASM signature (resolved 2026-05-11 review #2):** widen `render_page_for_preview` (and `render_page_in_project`) to take an optional `EngineCapture`; WASM constructs `EngineRegistry::with_replay` internally.

## Open questions (resolve before implementing)

Phase-C-specific design choices that the epic deferred.

### Q-C1 — Capture transport: schema migration or sidecar map?

The epic plan describes "add `engine_capture_id: Option<DocumentId>` to each text doc's index entry." The current `IndexDocument` schema is:

```ts
files: Record<string, string>;        // path → docId
version?: number;
identities?: Record<string, ActorIdentity>;
```

`files` values are strings, not objects. Two options:

- **(a) Migrate `files` to object values:**
  ```ts
  files: Record<string, FileEntry>;
  interface FileEntry { docId: string; captureDocId?: string; staleness?: boolean; executing?: boolean; }
  ```
  Plus a `CURRENT_SCHEMA_VERSION` bump (1 → 2) and migration in `migrateIndexDocument`. Requires changes in both Rust (`crates/quarto-hub/src/index.rs`) and TS schema. Existing automerge documents that pre-date v2 need to be re-written on load.

- **(b) Sidecar map:**
  ```ts
  files: Record<string, string>;        // unchanged
  captures?: Record<string, CaptureRef>; // new
  interface CaptureRef { captureDocId: string; staleness?: boolean; executing?: boolean; }
  ```
  Additive. No migration of `files`. The sidecar is keyed by path so it stays aligned with `files`. Pre-v2 docs simply have no `captures` key, which Phase C interprets as "no captures recorded yet."

**Recommendation:** (b) sidecar. Lower blast radius (no breaking change to consumers that already type `files` as `Record<string, string>`), additive in both schemas, easier to roll back if we want to remove the feature. The cost is one extra map lookup per file at read time; negligible. We do still bump `CURRENT_SCHEMA_VERSION` to 2 so older clients can detect that captures are *possible* and don't see absence-of-key as proof of absence-of-capture.

### Q-C2 — Engine driver: full pipeline or extracted engine-only step?

To produce a capture server-side, we need to invoke the engine. Two shapes:

- **(a) Run the full render pipeline server-side, discard HTML, keep capture.** Re-uses `EngineExecutionStage` and the existing pipeline build path. The observer harvests the capture via `ENGINE_CAPTURE_KIND`. The HTML output is thrown away — it'll be re-rendered by WASM in the browser.

- **(b) Extract a thin "engine driver" function** that takes a `.qmd` path + project context, parses, detects + runs the engine, and returns `EngineCapture` directly. New code path, less work per invocation, but bypasses MetadataMergeStage / IncludeExpansionStage which the engine may need (e.g. for engine config inheritance from `_quarto.yml`, or for included sub-docs containing code cells).

**Recommendation:** (a) for MVP. It's slower but correct — the engine runs under the same pipeline conditions as `q2 render`. The extracted-driver optimisation can wait for performance evidence. Note that this implies the server runs `MetadataMergeStage`, `IncludeExpansionStage`, `PreEngineSugaringStage`, `EngineExecutionStage` and then *stops* — we do not want the server running render-side stages because those duplicate work the WASM will do.

Implementation note: build a "preview-record" sub-pipeline using the existing stage builder, capped at `EngineExecutionStage`. This is the smallest possible reuse path.

### Q-C3 — Code-cell detection and staleness signal

The epic says "byte-for-byte cell content vs last capture's `input_qmd`." Two follow-on questions:

- **What if the document has no code cells?** Then there's nothing to execute. We should detect this *before* invoking the engine driver — saves an engine spawn cost on prose-only docs. Use `detect_engine(meta)` plus a "has any code cell" walk over parsed blocks; if no code cells, write a "no engine needed" marker (or simply omit the capture entry) and skip both eager-run and staleness logic.

- **What counts as "the cell content"?** Two interpretations:
  - The whole serialized QMD (matches the input `EngineExecutionStage` already feeds to engines).
  - Just the code-cell content, ignoring prose changes.

  The first matches `ReplayEngine`'s validation (byte-equality on input), but it makes *prose-only edits* appear to invalidate the engine capture. That's wrong — prose edits don't affect cell output, but they'd flip `staleness: true` and force the user to re-execute for no reason.

  The second requires a more careful parse-and-canonicalize step but matches the user's intuition.

**Recommendation:** **whole-QMD byte-equality for v1**, with a known limitation documented. The "smarter" cell-only diff is a Phase-C polish item (or Phase D follow-up). Rationale: matches the existing `ReplayEngine` miss policy exactly, so the server's staleness signal is the same byte-equality the browser-side replay would hit on a miss. Drift between these two checks is worse than the false-positive cost. A subsequent issue can refine the canonicalization.

### Q-C4 — "Executing code…" signal: explicit `executing` flag vs. inferred?

C.1 wants the browser to show "Executing code…" while the server runs the first eager capture. Three signal designs:

- **(a) Inferred — no flag.** Browser shows the overlay when "doc has code cells AND no capture exists." Problem: cannot distinguish "engine running" from "engine errored, no capture produced" or "engine disabled (`preview.engine: off`)."
- **(b) Explicit `executing: boolean` flag** on the sidecar entry. Server flips it to `true` before starting the engine and to `false` after the capture write (or after the error). Cleanest signal; browser polls the index doc as it already does (`onFilesChange` callback) and reacts.
- **(c) Status enum** (`Idle | Running | Error | Done`) with optional `lastError: string`. Most expressive; lets the SPA show error overlays without an additional channel.

**Recommendation:** start with (b), but reserve the field name `state` (not `executing`) for the eventual upgrade to (c). The first user-visible difference between "running" and "errored" is small; we ship the simpler signal and refine if a real failure mode shows up.

### Q-C5 — Re-execute endpoint surface

`/api/preview/re-execute` is the affordance C.5 wires up. Open shape questions:

- **HTTP method + body.** POST with JSON body `{ path: "posts/post1.qmd" }`. Server validates path is in the project, kicks off the engine driver, returns 202 Accepted with `{ captureDocId, eta_ms? }` — *not* the capture inline, because the capture flows back via samod sync, not HTTP response.
- **Concurrency.** What if the user clicks Re-execute twice rapidly? Reject the second with 409 Conflict, or merge into the in-flight run. **Recommendation:** reject with 409 + a "currently executing" message; UI's button stays disabled while `state == Running` anyway.
- **Auth.** Phase A's loopback-only posture means anyone-on-localhost can hit the endpoint. Acceptable for v1 given the existing posture; document in security notes.

### Q-C6 — Cache key for `<tempdir>/captures/` (C.7)

The epic says "keyed by content hash." Open: hash what?

- The full `input_qmd` bytes (matches `ReplayEngine`'s replay check).
- A canonicalized "engine input" (would let prose-only edits keep using the cache without a re-execute prompt).

**Recommendation:** **hash the full `input_qmd`** for v1, matching Q-C3's whole-QMD policy. Keep the cache key and the staleness check using the *same* canonicalization function so they never drift. Refinements come in a follow-up.

## Dependency order (read this before picking up any sub-task)

Even though the epic numbering is C.1–C.7, the safe **implementation order** is:

```
C.3 (transport schema + WASM signature)
   ↓
C.1 (server eager-run + capture write)        C.4 (browser-side replay wiring)
                       ↓
                   C.2 (staleness detection)
                       ↓
                   C.5 (stale-capture UX)
                       ↓
                   C.6 (preview.engine config)   C.7 (per-doc capture cache)
```

- C.3 is foundational: every later piece reads or writes the new schema. Land it first behind a feature flag if needed.
- C.1 and C.4 can land in either order once C.3 ships; they touch disjoint surfaces (server-side capture write vs. browser-side replay use). C.4 has no observable effect until C.1 produces real captures, so test it against a hand-authored capture first.
- C.5 needs both C.1 (so there *is* a previous capture to render with) and C.2 (so `staleness: true` is computable).
- C.6 and C.7 are parallelisable polish.

## Work breakdown

### C.1 — First-time eager capture

When `q2 preview` opens a doc with code cells and no existing capture, the server runs the engine once and writes the resulting `EngineCapture` into automerge.

**Affects:** `crates/quarto-preview/src/lib.rs` (new server-side capture driver), `crates/quarto-hub/src/server.rs` (extending `sync_file` or a new sibling step), `crates/quarto-core/src/engine/preview_record.rs` (new — sub-pipeline that stops at `EngineExecutionStage`).

**Test plan (TDD):**

1. Unit: a new `record_capture(path, project, runtime) -> Result<Option<EngineCapture>>` function returns `Some(capture)` for a `.qmd` containing a `{python}` code cell (using the markdown engine substituted for `python` via the existing `EngineRegistry` test seam), and `None` for a prose-only doc.
2. Unit: the capture's `engine_name` matches the detected engine; `input_qmd` matches the serialized post-include-expansion QMD; `result.markdown` matches the engine's output.
3. Integration (server): start `q2 preview` against a fixture with one code cell, assert the index doc's sidecar entry gets `captureDocId` populated within 30 s and that the linked binary doc contains a parseable `EngineCapture`.
4. Integration (server): same fixture without code cells — assert no `captureDocId` is written.
5. Smoke (end-to-end through `q2 preview` binary): assert the SPA shows "Executing code…" overlay between the time the page mounts and the time the capture lands; assert the post-capture render contains the engine's output.

**Acceptance:** all unit + integration tests pass, plus the end-to-end smoke against a markdown-engine fixture (no R/Python required in CI).

### C.2 — Staleness detection

On every doc-content change (server-side), if a capture exists for that doc, compare the freshly-serialized `input_qmd` byte-for-byte against the capture's `input_qmd`. If different, set `staleness: true` on the sidecar entry. Do **not** re-execute (unless `preview.engine: auto` — see C.6).

**Affects:** `crates/quarto-hub/src/server.rs:1040` (`run_file_watcher` → `ctx.sync_file`), `crates/quarto-core/src/engine/preview_record.rs` (the canonicalization function exported from C.1).

**Test plan:**

1. Unit: the canonicalization function returns identical bytes for two equal QMDs; differing bytes for edits.
2. Integration: load a fixture with an existing capture; edit a code cell on disk; assert `staleness: true` is written to the sidecar within the watcher debounce window (~600 ms after the edit).
3. Integration: edit *prose* in a fixture with a capture. **Expected behaviour for v1 (per Q-C3):** `staleness: true` is also written, because we use whole-QMD byte-equality. Document this in the spec; a follow-up issue will refine.

**Acceptance:** unit + both integration tests; the prose-staleness behaviour is documented as a known v1 limitation (see Q-C3).

### C.3 — Capture transport (schema)

Land the IndexDocument schema extension that C.1 / C.2 / C.4 / C.5 all consume.

**Affects:**
- `ts-packages/quarto-automerge-schema/src/index.ts` (schema types + `migrateIndexDocument`).
- `crates/quarto-hub/src/index.rs` (Rust-side reader/writer for the sidecar).
- `ts-packages/quarto-sync-client/src/types.ts` and `client.ts` (so SPA consumers can observe the new sidecar).
- Bump `CURRENT_SCHEMA_VERSION` to 2.

**Per Q-C1 decision: sidecar map**, not value migration:

```ts
interface IndexDocument {
  files: Record<string, string>;        // unchanged
  captures?: Record<string, CaptureRef>; // new
  version?: number;                     // bumped to 2
  identities?: Record<string, ActorIdentity>;
}

interface CaptureRef {
  captureDocId: string;
  staleness?: boolean;                  // default false
  state?: 'idle' | 'running' | 'error'; // per Q-C4
  lastError?: string;                   // set when state === 'error'
}
```

**Test plan:**

1. Schema roundtrip: write an IndexDocument with a sidecar entry, serialize via automerge, deserialize, assert equality.
2. Migration: a v1 IndexDocument (no `captures` key) survives `migrateIndexDocument` without crash; new version is 2.
3. Reader tolerance: a v2 IndexDocument with the sidecar present but with unexpected extra keys (forward-compat) deserializes cleanly in TS.

**Acceptance:** all schema-level tests pass; the rest of the suite is unaffected by the migration (TS + Rust workspace tests green).

### C.4 — Browser-side replay

Widen `render_page_for_preview` to accept an optional `EngineCapture` argument; route through `EngineRegistry::with_replay` inside WASM.

**Affects:**
- `crates/wasm-quarto-hub-client/src/lib.rs:1171` (`render_page_for_preview` signature + dispatch).
- `ts-packages/preview-runtime/src/wasmRenderer.ts:436` (TS binding for the new arg).
- `q2-preview-spa/src/PreviewApp.tsx:202` (read sidecar `captureDocId` → fetch binary doc → pass to renderer).

**Wire shape.**

The capture flows from the binary doc (a samod text doc with the gzipped JSON serialized `EngineCapture`) through the SPA, into `render_page_for_preview(path, user_grammars, capture?)`, where the WASM does:

```rust
let registry = match capture {
    Some(c) => EngineRegistry::with_replay(c),
    None    => EngineRegistry::default(),
};
// build the q2-preview pipeline using `registry`
```

**Test plan:**

1. WASM unit: hand-author an `EngineCapture` for a `python` cell; call `render_page_for_preview(path, None, Some(capture))`; assert the rendered AST contains the captured `result.markdown`.
2. WASM unit: same call with `capture: None`; assert no error (falls through to default registry; markdown engine for a prose doc behaves as today).
3. Integration: SPA test that reads a hand-authored capture binary doc and routes it through the renderer; same assertion shape.

**Acceptance:** WASM tests pass; the SPA's render path doesn't regress for the (no capture) case.

### C.5 — Stale-capture UX

When `staleness: true`, the SPA still renders the page using the *previous* capture (so the preview remains responsive) and shows a fixed-position overlay: "Code has changed. Re-execute?" with a button that POSTs to `/api/preview/re-execute`.

**Affects:**
- `q2-preview-spa/src/PreviewApp.tsx` + a new `StaleCaptureOverlay` component (modelled on the existing `PreviewErrorOverlay` from Phase A).
- `crates/quarto-preview/src/lib.rs` — register `/api/preview/re-execute` route (axum router extension; mirrors `extend_with_spa`).
- Server-side: the route handler validates the path and invokes the same capture driver from C.1 (synchronously to acquire a 202 response, async to do the actual work).

**Test plan:**

1. SPA unit: given `staleness: true` and a valid `captureDocId`, the overlay renders, the button is enabled, and clicking it issues a POST.
2. SPA unit: given `staleness: false`, the overlay is not rendered.
3. Integration: end-to-end via Playwright — fixture with a capture, edit code cell, expect overlay; click the button, expect new capture + overlay dismissed.
4. API unit: POST with a non-project path returns 400; POST while `state: running` returns 409; POST when `preview.engine: off` returns 403.

**Acceptance:** all unit + the Playwright e2e pass.

### C.6 — Configuration knob

Read `preview.engine` from merged metadata. Three values:

- `manual` (default): C.5 behaviour. Server detects staleness; user must opt in.
- `auto`: server re-executes on every code-cell change. (Same as Q1 default.)
- `off`: server never executes. Code cells render as inert source. C.1's eager run is also skipped.

**Affects:**
- `crates/quarto-core/src/stage/stages/metadata_merge.rs` (no change — `preview.engine` flows through metadata like any other key; the consumer is the new preview-record driver, not the pipeline).
- `crates/quarto-preview/src/config.rs` or similar — read the merged metadata once at session start (and on `_quarto.yml` changes, à la Phase B.4) to drive the driver's behaviour.

**Test plan:**

1. Unit: parse a fixture with `preview.engine: auto`; assert the config struct reflects it.
2. Integration: fixture with `preview.engine: auto` + a capture; edit a code cell; assert a *new* capture is written automatically (no staleness flag visible to the SPA).
3. Integration: fixture with `preview.engine: off`; assert no eager capture is written; SPA renders code cells as source.

**Acceptance:** unit + 3 integration tests; the existing C.1/C.5 tests continue to pass under the default (`manual`).

### C.7 — Per-doc capture cache

Filesystem cache at `<tempdir>/captures/<content-hash>.bin` (gzipped `EngineCapture`). When the server is about to run the engine, check the cache; if present, skip the engine and write the cached capture directly to automerge.

**Affects:**
- `crates/quarto-preview/src/cache.rs` (new module).
- The capture driver from C.1 — wrap the engine invocation with cache lookup/insert.

**Test plan:**

1. Unit: round-trip a cached capture through `<tempdir>/captures/`.
2. Integration: edit a code cell back to its original content; assert the second run uses the cache (no engine invocation — verified by registry inspection).
3. Smoke: closing and re-opening `q2 preview` against the same fixture reuses the cache from the prior session (cache lives in a stable per-project location? or per-session tempdir? Q-C6 implies content-hash so per-session is fine but per-project would be friendlier — note for review).

**Acceptance:** unit + 2 integration tests.

## Sub-task issues (filed 2026-05-14)

One bd-issue per sub-task, parent-child to bd-kw93, blocks-edges per the dependency graph above.

| Sub-task | bd-id        | Blocked by                |
|----------|--------------|---------------------------|
| C.3      | bd-kw93.1    | — (foundational)          |
| C.1      | bd-kw93.2    | bd-kw93.1                 |
| C.4      | bd-kw93.3    | bd-kw93.1                 |
| C.2      | bd-kw93.4    | bd-kw93.2                 |
| C.5      | bd-kw93.5    | bd-kw93.3, bd-kw93.4      |
| C.6      | bd-kw93.6    | bd-kw93.5                 |
| C.7      | bd-kw93.7    | bd-kw93.5                 |

Each sub-task lives on a `beads/<id>-<slug>` topic branch off `feature/q2-preview-command`, merged with `--no-ff` per the worktree workflow (same pattern as B.1 / B.3 / B.4).

## Out of scope for Phase C

- **Engine runtime discovery / kernelspec resolution.** Phase A planned for this (Risk 4); kernelspec discovery work is tracked separately under bd-vt0w / `claude-notes/plans/2026-05-04-jupyter-kernelspec-discovery-and-errors.md`. C.1's eager run assumes the kernelspec resolution machinery is in place by the time real Jupyter docs land in tests; Markdown-engine fixtures (which Phase C uses for CI) don't depend on it.
- **Shiny / observable / interactive runtimes.** Replay doesn't apply.
- **PDF preview.** Q6 keeps Phase C HTML-only.
- **Freeze integration.** Phase C captures live in samod + tempdir cache; honouring `freeze` is a future epic.
- **Cross-doc capture invalidation.** A code-cell edit in `helper.qmd` doesn't currently invalidate `index.qmd`'s capture even if `index.qmd` includes `helper.qmd`. The dep-graph machinery for this exists (used by `q2 render` Phase 8) but isn't wired into Phase C's staleness check. Worth a Phase D follow-up; the simplest interpretation in v1 is "each doc's capture invalidates only on its own content changes."
- **Phase D dep-graph filter for re-renders.** Tracked under bd-0mji from the Phase B follow-up. Independent of Phase C.

## Risks

1. **`EngineExecutionStage` re-entrancy under server-side use.** The stage was designed for the per-document render pipeline. We're now invoking it from the server's file-watcher loop; multiple concurrent engine runs (one per changed doc) could thrash the engine's `temp_dir` if shared. The capture driver should give each invocation its own scratch dir.

2. **Eager-run blocking server startup.** If a project opens with five docs that all have code cells, eager-running them all at once will block the index from settling. Phase A's startup path already lets the SPA mount before the server is done indexing files; the eager runs should be enqueued **after** the index settles and run sequentially (or at low concurrency), surfacing `state: running` per doc as it progresses. Otherwise the first user interaction sees five overlapping "Executing code…" overlays. Document the cap.

3. **Schema sidecar drift.** Q-C1's sidecar approach means `files` and `captures` are independent keys in the index. A bug that adds a `captures` entry for a non-existent path (or fails to delete one when a doc is removed) is a silent garbage-accumulation problem. The Rust + TS index code should both delete sidecar entries on doc removal, and the schema test plan should pin this.

4. **Capture size in automerge.** A `EngineCapture` for a non-trivial Jupyter notebook can be hundreds of KB (large `result.markdown` from a multi-plot doc). Each capture is a separate binary doc, so individual doc sizes stay bounded — but the *total* automerge transfer cost on initial sync grows linearly with the number of cached docs. For MVP we accept this; bd-5qnj (trace-size investigation, sibling of bd-45yw) has the size-control machinery for the trace artefact and could be adapted.

5. **WASM signature back-compat.** Widening `render_page_for_preview` is straightforward (optional arg, default None preserves behaviour). But `wasm-bindgen` generates TS bindings that the SPA consumes; the SPA must build cleanly against both the old and new bindings during the transition. Stage the WASM change first, land + verify; then update the SPA to start passing captures.

## Pre-flight investigation receipts (2026-05-13)

Recorded so future sessions don't re-derive these:

- `EngineExecutionStage` already emits captures via the observer channel — see line 256 of `crates/quarto-core/src/stage/stages/engine_execution.rs`.
- `EngineRegistry::with_replay(capture)` is the registry helper — `crates/quarto-core/src/engine/registry.rs:157`.
- WASM render entry: `render_page_for_preview(path, user_grammars)` at `crates/wasm-quarto-hub-client/src/lib.rs:1171`. Signature widens to add an optional `EngineCapture`.
- TS entry: `wasmRenderer.ts:436` — `JSON.parse(await wasm.render_page_for_preview(path, userGrammars))`. The new arg flows here.
- IndexDocument Rust shape: `crates/quarto-hub/src/index.rs:28` — currently a thin wrapper around a flat `files: Map<String, String>`.
- IndexDocument TS shape: `ts-packages/quarto-automerge-schema/src/index.ts` — `CURRENT_SCHEMA_VERSION = 1`, `migrateIndexDocument` is the migration entry point.
- Server-side change handling: `crates/quarto-hub/src/server.rs:1040` — `run_file_watcher` loop → `ctx.sync_file(&path)`. New Phase C logic hangs off here.
- Router extension: `crates/quarto-preview/src/lib.rs:116` — `extend_with_spa`. The `/api/preview/re-execute` route plugs in alongside.
- `RenderToFileOptions` already accepts `EngineCapture` for the native `q2 render --replay` path — `crates/quarto/src/commands/render.rs:61`. The preview-record driver doesn't need this exact surface but it confirms the capture is a first-class concept in the renderer.
