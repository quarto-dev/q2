---
date: 2026-05-11
branch: feature/q2-preview
status: v3 — all open items resolved (2026-05-11 review #2). Ready to
        spin up the hub-client decomposition sub-epic, after which
        Phase A planning can begin.
beads: bd-kw93 (epic).
---

# `q2 preview` — Feasibility & Architecture Plan (Epic)

## Goal

Build `q2 preview` as a native CLI that wraps an ephemeral local
hub-client instance, using `feature/q2-preview`'s React-driven
incremental renderer as the in-browser view. The user runs
`q2 preview [path]` from a Quarto project directory, a browser tab
opens, and edits to local files appear in the rendered page within a
few hundred milliseconds — *without rebuilding the DOM*, so stateful JS
(Bootstrap menus, MathJax, reveal.js, listings filters) survives every
edit.

This is the Q2 replacement for Q1's `quarto preview`. The design
explicitly leans on three Q2-specific assets that did not exist in Q1:

1. **`quarto-hub` + samod + automerge** — already provides
   ephemeral-server, file-watching, and websocket reload for free.
2. **The `q2-preview` format (this branch)** — already produces a
   post-pipeline AST that the React renderer consumes without
   touching the DOM tree on edit.
3. **`engine: replay` + `quarto-trace`** — already captures and
   replays engine output deterministically; gives us a clean
   "record once on the server, replay everywhere" story for code
   execution.

This plan asserts a *feasible-and-aligned* path, not a final
implementation. Several open questions are listed in §"Open
questions"; they need decisions before phase plans are drafted.

## Why this is feasible (today's substrate)

Everything below is in `main` (or in this branch, where called out):

| Component | Status | Reuse |
|---|---|---|
| `quarto-hub` samod-based sync server | shipped | Drop-in for the ephemeral preview server. Already has `--no-project` / standalone / project modes (`bd-3aga`, `crates/quarto-hub/src/{server,context}.rs`). |
| `FileWatcher` (`crates/quarto-hub/src/watch.rs`) | shipped (`.qmd` only) | Reuse; extend to cover `_quarto.yml`, `_metadata.yml`, `_extensions/`, images, `.tsx`. |
| `StorageManager::new_standalone` + `default_standalone_data_dir` | shipped | Use with an ephemeral temp dir so storage is wipe-on-exit. |
| `quarto-trace-server`'s `include_dir!("$QUARTO_TRACE_VIEWER_EMBED_DIR")` pattern | shipped | Direct precedent for embedding the pared-down hub-client bundle into the `q2` binary. Same `<env>_DIR` override for live UI iteration. |
| `q2-preview` format | this branch | Pipeline + `render_qmd_to_preview_ast` already produce the AST the React renderer consumes (`crates/quarto-core/src/{format,pipeline}.rs`, `ast_transforms.rs:139`). |
| `PreviewRouter` + `Q2PreviewIframe` + `ReactRenderer` | this branch | The React-in-iframe path that survives DOM-stateful JS across edits. Already wired into `hub-client/src/components/render/`. |
| `DocumentProfile` + `ProjectDependencyGraph` (Phase 8) | shipped | Forward + reverse `edges` between docs. Phase 8 gives us *exactly* the "when X changes, re-render Y" mapping we need for cross-doc preview invalidation. |
| `ReplayEngine` + `EngineCapture` in `TraceDocument` | shipped (`bd-45yw`) | The mechanism for "execute once on server, replay forever in WASM". `RenderToFileOptions.replay_capture` and `EngineRegistry::with_replay` are the seams. |
| Initial project → automerge upload | shipped | `HubContext::new` already calls `reconcile_files_with_index` + `sync_all_documents` against a real project. |
| `quarto hub` subcommand | shipped | Confirms the CLI integration pattern we'd mirror for `quarto preview`. |

What is **missing** is the glue, summarized in §"Phases" below.

## High-level architecture

```
┌──── q2 preview foo.qmd ──────────────────────────────────────────────┐
│                                                                      │
│  ┌──────────────────────┐                                            │
│  │ 1. CLI bootstrap     │  Create temp dir; canonicalize project.    │
│  └─────────┬────────────┘                                            │
│            ▼                                                         │
│  ┌──────────────────────┐                                            │
│  │ 2. samod sync server │  via quarto-hub::server::run_server         │
│  │    (ephemeral data)  │  with standalone storage in temp dir.       │
│  └─────────┬────────────┘                                            │
│            ▼                                                         │
│  ┌──────────────────────┐                                            │
│  │ 3. FileWatcher       │  Extended for all preview-relevant files.   │
│  │    + qmd→automerge   │  Existing sync.rs handles the diff/merge.   │
│  └─────────┬────────────┘                                            │
│            ▼                                                         │
│  ┌──────────────────────┐                                            │
│  │ 4. Engine executor   │  Server-side native run on .qmd change.    │
│  │    + replay capture  │  Writes EngineCapture into automerge.       │
│  │    + invalidation    │  Hub-client replays via ReplayEngine.       │
│  └─────────┬────────────┘                                            │
│            ▼                                                         │
│  ┌──────────────────────┐                                            │
│  │ 5. Static SPA route  │  Axum fallback serves the pared-down        │
│  │    (embedded bundle) │  hub-client bundle (include_dir!).          │
│  └─────────┬────────────┘                                            │
│            ▼                                                         │
│  ┌──────────────────────┐                                            │
│  │ 6. Browser SPA       │  Loads → connects samod ws → renders        │
│  │    (pared-down)      │  q2-preview React. Updates incrementally.   │
│  └──────────────────────┘                                            │
│                                                                      │
│  ┌──────────────────────┐                                            │
│  │ 7. Shutdown          │  On Ctrl-C: stop watcher, drop samod repo,  │
│  │    (wipe temp dir)   │  delete the temp data dir.                  │
│  └──────────────────────┘                                            │
└──────────────────────────────────────────────────────────────────────┘
```

### Data flow on a single edit

1. User saves `posts/foo.qmd`.
2. `FileWatcher` fires `Modified(posts/foo.qmd)`.
3. `HubContext::sync_file` reads bytes, forks-at-checkpoint, merges
   into the file's automerge doc. This already exists.
4. **(new)** If `foo.qmd` is the currently-previewed page or in its
   forward dep-graph closure, the server compares the new file's
   code-cell content against the last `EngineCapture`'s
   `input_qmd`:
   - if code-equal, the existing capture is still valid; nothing
     to do (the qmd-text change re-renders via replay).
   - if different, the capture is now **stale-but-not-replaced**.
     The server records a `staleness: true` marker but **does not
     re-execute by default** (per user decision — see Q3 below).
     The browser overlays a "Code changed — re-execute?" affordance
     and the user opts in explicitly.
   New captures only get written when (a) no capture exists yet
   for the doc, or (b) the user triggers re-execution via the
   affordance / a CLI flag / `_quarto.yml` setting.
5. Hub-client `useAutomergeSync` receives both the qmd-text patch
   and the trace patch.
6. Hub-client triggers WASM `render_page_in_project(path)` with the
   engine registry overridden to `EngineRegistry::with_replay(cap)`.
   The pipeline runs end-to-end *in the browser* without spawning
   any subprocesses. The post-pipeline AST flows into
   `Q2PreviewIframe`, which morphs only the React subtree that
   changed. Bootstrap/MathJax/reveal state persists.

The crucial detail: **engines run on the server (where they can
shell out); rendering runs in the browser (where the AST→DOM
incrementality lives).** The bridge is the `EngineCapture` carried
through automerge.

## Phasing

Phases below are checkpoints, not full sub-plans. Each phase will get
its own dated plan document (and a beads sub-issue under the epic)
once this top-level plan is reviewed.

### Phase A — Skeleton CLI + standalone serving (no engines)

Smallest end-to-end vertical slice that proves the architecture.

- [ ] **A.1** Add `quarto preview` clap subcommand (mirror
  `commands/hub.rs`). Args: `[path]` (default: project index),
  `--port`, `--no-browser`, `--data-dir <override>`,
  `--preview-dir <override>` (the SPA-from-disk override, same
  pattern as `QUARTO_TRACE_VIEWER_DIR`).
- [ ] **A.2** Create `crates/quarto-preview/` (new). Owns the
  CLI wiring + the embedded SPA + the preview-specific axum routes
  on top of the hub server's router. Depends on `quarto-hub` and
  the new `quarto-preview-client` build.
- [ ] **A.3** Add `hub-client` preview-mode build target. A second
  Vite entry (`preview.html`) bundles the same code with a
  build-time flag `__QUARTO_PREVIEW__` that:
  - skips `LoginScreen` / `useAuth`,
  - skips `ProjectSelector` / `ProjectSetSetup`,
  - skips `FileSidebar` / `Editor` (renders only `Preview`),
  - reads `indexDocId` + `wsUrl` from a `<meta>` tag emitted by
    the server (same trick the hub-client uses for share links).
- [ ] **A.4** Wire `include_dir!("$QUARTO_PREVIEW_EMBED_DIR")` into
  `quarto-preview` with the build.rs the same shape as
  `quarto-trace-server/build.rs`. Confirm `cargo xtask build_all`
  builds the preview bundle into `hub-client/dist-preview/` first.
- [ ] **A.5** `q2 preview` boots: creates a temp `data_dir`,
  spawns `quarto-hub::server::run_server` with project mode,
  emits the URL, opens the browser, blocks on Ctrl-C. On
  shutdown, deletes the temp `data_dir`.
- [ ] **A.6** Manual force-refresh button. A persistent UI
  affordance in the preview SPA that re-runs the WASM render
  pipeline against the current automerge state. Always visible,
  regardless of staleness — this is the user's escape hatch for
  cross-doc dependency channels the dep graph doesn't (yet)
  encode. Phase C wires the same button to also trigger
  server-side engine re-execution when applicable.
- [ ] **A.7** End-to-end smoke test: `q2 preview` on a 2-page
  Quarto site with no code cells; editing markdown in either
  file updates the preview within 1s; no DOM rebuild
  (asserted by a Playwright stable-element-id check); the
  manual force-refresh button works.

**Acceptance:** the loop from §"Data flow" works for documents
*without* engine execution. The force-refresh button works. The
replay/engine work is Phase C.

### Phase B — File-watcher and remap broadening

- [ ] **B.1** Extend `FileWatcher`'s `is_qmd_file` filter to a
  policy that includes `.qmd`, `_quarto.yml`, `_metadata.yml`,
  everything under `_extensions/`, image extensions, and `.tsx`
  custom-component files. Keep `.qmd`-only as a feature gate so
  the hub binary's existing semantics don't change.
- [ ] **B.2** Decide how `format: html` → `q2-preview` happens in
  preview mode (see Open Q1). Add a new `PreviewFormatRemap` knob
  on the pipeline config; default off for `render`, on for
  `preview`.
- [ ] **B.3** Ensure cross-doc edges from `ProjectDependencyGraph`
  invalidate the right pages in the browser. The hub-client
  already re-runs `render_page_in_project` when the index doc
  changes; we just need to verify that the dep-graph reverse
  edges drive that re-run for the *currently-displayed* page
  even when a sibling page changes. (May be a no-op if Phase 8's
  cache invalidation already covers this; needs investigation.)
- [ ] **B.4** Acceptance: editing `_quarto.yml` re-renders all
  open pages; editing `posts/_metadata.yml` re-renders only its
  siblings; editing an unrelated sibling re-renders the active
  page only when there's a dep edge.

### Phase C — Engine execution (record-on-demand, replay-otherwise)

This is the load-bearing phase. The contract is the bridge from
§"Data flow" item 4.

**Default behavior (per user 2026-05-11):** the server does *not*
automatically re-execute on code-cell change. It detects staleness
and surfaces an affordance. The user opts in.

- [ ] **C.1** First-time capture trigger. When `q2 preview` opens a
  doc with code cells and no existing capture, the server runs
  the engine once *eagerly* and writes the resulting
  `EngineCapture` into automerge. Until that finishes, the
  browser shows a "Executing code…" overlay rendered from the
  parse-only AST (code cells appear as their source).
- [ ] **C.2** Staleness detection. On every doc change, the server
  parses the .qmd, extracts the code-cell content, and compares
  byte-for-byte against the last capture's `input_qmd`. If
  different, write a `staleness: true` marker on the doc's index
  entry. Do **not** re-execute.
- [ ] **C.3** Capture transport. Add an `engine_capture_id:
  Option<DocumentId>` field to each text doc's index entry,
  pointing at a sibling *binary* doc that stores the gzipped
  capture, plus a `staleness: bool`. Both survive reconnect and
  sync naturally to the browser via samod.
- [ ] **C.4** Browser-side replay. When `useAutomergeSync` sees a
  fresh capture for the active doc, it calls
  `render_page_in_project` with the capture surfaced as a new
  optional parameter (see Risk 1 below). The render runs the
  pipeline with replay; engines in WASM otherwise pass through.
- [ ] **C.5** Stale-capture UX. When `staleness: true`, render
  the page *with* the still-valid capture (so the prose/preview
  remains live) and overlay a fixed-position "Code has changed —
  re-execute?" affordance. The affordance lists which cells
  changed (cell IDs from parse). On click, the browser POSTs to
  `/api/preview/re-execute`; the server runs the engine,
  replaces the capture, clears `staleness`.
- [ ] **C.6** Configuration. `preview.engine: manual | auto | off`
  in `_quarto.yml`, with `manual` as the **default**.
  - `manual` (default): C.5 behavior. No automatic re-execution.
  - `auto`: re-execute on every code-cell change. For users who
    want the Q1-style behavior on small docs.
  - `off`: never execute. Code cells render as inert source. The
    first-time eager run from C.1 is also skipped.
- [ ] **C.7** Per-doc capture cache (in `<tempdir>/captures/`)
  keyed by content hash, so swap-and-restore of an open file
  doesn't re-execute. Also serves as the "warm cache" when the
  user resumes a preview session shortly after closing one.

**Acceptance:** end-to-end test:
- A `.qmd` with a jupyter cell renders correctly after `q2
  preview` (eager first-time run).
- Editing the prose re-renders without touching the engine.
- Editing the code cell shows the staleness affordance; the
  preview keeps rendering with the previous capture.
- Clicking the affordance triggers re-execution and the new
  capture renders.
- Setting `preview.engine: auto` reproduces Q1-style behavior.

### Phase D — Polish & parity

- [ ] **D.1** Browser-tab-on-startup, `--no-browser`,
  port-conflict retry.
- [ ] **D.2** Initial-path resolution: `q2 preview` → project
  index; `q2 preview foo.qmd` → that file; with a website
  project, resolve the index page via `ProjectIndex`.
- [ ] **D.3** Static-file resources (e.g. CSS in `_extensions/`):
  confirm they round-trip through binary-doc sync.
- [ ] **D.4** Diagnostics surface: render errors should overlay
  on the preview, not silently fail. (`PreviewErrorOverlay`
  already exists; verify its wiring under preview mode.)
- [ ] **D.5** Documentation in `docs/` for user-facing preview
  command.

### Phase E — Stretch (post-MVP)

- Hot-reload of `.tsx` custom components from disk (not just
  through automerge); useful for component developers.
- Multi-window: opening two browser tabs at the same preview URL
  should stay in sync via samod (essentially free; verify).
- `q2 preview --share <url>` to broadcast to a remote sync
  server, turning the local preview into a temporary
  collaboration session.

## Open questions

These are the ones the design intentionally defers. Resolutions
from the 2026-05-11 review are noted inline. Each "**Decision**"
line is settled; the remaining text below explains why.

### Q1 — Where does `format: html` → `q2-preview` remap happen?

**Decision: option (c)** — explicit `RenderMode::Preview` threaded
through pipeline config.

**Options:**
- **(a)** At the orchestrator level: when the preview CLI builds
  `HtmlRenderConfig` / `RenderToFileOptions`, substitute the
  format-id before the pipeline is built. Surgical; doesn't touch
  format.rs.
- **(b)** At `format.rs::Format::from_format_string` time, gated
  on a thread-local or context flag set by the preview entry
  point. Risk: invisible global state.
- **(c)** Introduce an explicit `RenderMode::Preview` on the
  pipeline config, threaded through `build_html_pipeline_stages`,
  so each stage knows it's running in preview mode and can pick
  the right sub-pipeline. Matches the existing
  `ApplyConfig::Single | Project` axis.

**Recommendation:** (c). It generalizes — the same flag gates
"server should run engines and emit captures", "remap html →
q2-preview", and any future preview-only stages we add. It also
matches how Phase 8 already threads a mode through the pipeline
(Mode A vs Mode B).

### Q2 — Does the server *also* run the q2-preview pipeline?

**Decision: option (a)** — server runs engines only; the browser
runs the full q2-preview pipeline via WASM.

Additional motivation from the user (2026-05-11): a future version
of the q2-preview format will communicate AST changes *back* into
the .qmd source to offer a WYSIWYG-like authoring experience. That
must work in both hub-client and `q2 preview`, which requires the
two surfaces to share the *same* render code path. (b) would fork
that path and break the WYSIWYG round-trip for `q2 preview`.

This raises the importance of Risk 1 below: the WASM
`render_page_in_project` signature must accept the capture so the
browser-side pipeline can actually run with replay.

### Q3 — Engine invalidation policy

**Decision: per-document staleness *detection*, no automatic
re-execution.** The server detects staleness and surfaces an
affordance; the user explicitly opts into re-execution
(`preview.engine: manual` is the default — see Phase C.6).

Rationale from the user (2026-05-11): code execution can take a
long time. Automatic triggering on every code-cell save is too
disruptive. *Knowing* the capture is stale is valuable — the user
gets a visible cue to re-run when they're ready — but the
automatic path is reserved for users who explicitly opt in
(`preview.engine: auto`).

Open sub-question (not blocking the epic): what counts as a "code
cell change"? The current proposal is byte-equality of cell
content (matching `ReplayEngine`'s miss policy). Whitespace-only
diffs would trip staleness; that may be acceptable, or it may want
to be smarter later. Defer to Phase C planning.

### Q4 — Hub-client visibility gating in preview mode

**Decision: option (c)** — refactor hub-client so the preview app
imports only the components it needs.

User rationale (2026-05-11): this dovetails with the build-time
concerns in §"Build-time concerns / artifact ordering" below.
Shipping a second Vite-bundled entry from inside hub-client
preserves the current monolithic dependency situation where
"build `q2 preview` Rust" implies "build the full hub-client
SPA." Decomposing hub-client into reusable libraries lets the
preview SPA depend only on what it needs and gives us a cleaner
bootstrapping story.

This is a larger refactor than originally scoped — it likely
predates the rest of Phase A, or runs in parallel as its own
sub-epic. See §"Build-time concerns" for the implications.

### Q5 — Project vs single-doc mode

**Decision: confirmed** — auto-discover project from cwd; allow
`q2 preview --no-project file.qmd` as an escape hatch. Mirrors
`quarto hub` / `quarto render`.

### Q6 — What about output formats other than html?

**Decision: HTML-only for MVP.** Q2 is currently HTML-only across
the board (q2-preview, q2-debug, q2-slides all build on HTML).
PDF preview will eventually need a separate mechanism (probably
watching the artifact dir and reloading a PDF viewer iframe), but
that's a future epic and not blocking.

### Q7 — Security & sandboxing

**Decision: strict by default.** Bind to 127.0.0.1; refuse
non-loopback hosts unless `--insecure-allow-network` is passed
explicitly; print a stern warning when it is.

User rationale (2026-05-11): the security posture is more
important in Q2 than in Q1 because the q2-preview UI will
eventually let users *retrigger code execution from the
webpage* (the staleness affordance — Phase C.5) and, further
out, mutate the .qmd source via the WYSIWYG mode mentioned in
Q2. Both turn the preview port into a remote-code-execution
endpoint that anyone-on-the-network could trigger if we bind
beyond loopback. The stricter posture isn't paranoid — it's
matching the actual capability surface.

## Build-time concerns / artifact ordering

This concern surfaced in the 2026-05-11 review and is large
enough to deserve its own section. It interacts with Q4
(hub-client visibility gating) and Risk 1 (WASM signature).

### The problem

The preview SPA shares code with hub-client and with the rest of
the Quarto pipeline. The build graph is:

```
1. cargo build --target wasm32-unknown-unknown
       -p wasm-quarto-hub-client          # produces .wasm
2. wasm-bindgen                            # produces JS+TS glue
3. cd hub-client && npm run build:all      # bundles SPA (uses .wasm)
4. cargo build -p quarto-preview \
       (with QUARTO_PREVIEW_EMBED_DIR set) # embeds the SPA
5. cargo build -p quarto                   # links quarto-preview
```

There is **no Cargo-level cycle**. `quarto-preview` (native)
depends on `quarto-hub`; the SPA depends on
`wasm-quarto-hub-client` (wasm32). The two never link against
each other. But the build *order* is:
`wasm32 cargo → wasm-bindgen → npm → native cargo`.

The user's worry was that "building `q2 preview` requires
building `q2` first" — which is only true if the embedded SPA's
WASM ends up depending transitively on so much of the workspace
that any Rust change forces the whole cascade to re-run.
Bluntly: if a change to `pampa` or `quarto-core` requires
rebuilding the WASM, *then* the SPA, *then* the native binary
that embeds the SPA, iteration cycles are painful.

### How `quarto-trace-server` handles it (precedent)

We already solved a smaller version of this for the trace
viewer. `crates/quarto-trace-server/build.rs`:

- Looks for `trace-viewer/dist/index.html`.
- If present, embeds that directory via `cargo:rustc-env`.
- If absent, generates a *placeholder* `index.html` in `OUT_DIR`
  that tells the user to run `cargo xtask build-trace-viewer`,
  and embeds the placeholder.
- Always emits `cargo:rerun-if-changed=` for every file under
  the real dist (so `cargo build` re-embeds when the SPA
  rebuilds, but doesn't *fail* if it's missing).

`cargo xtask build-trace-viewer` then runs `npm run build` in
`trace-viewer/`. `cargo xtask build-all` chains the full
sequence. Iteration: dev runs the Vite dev server and points the
binary at it via `QUARTO_TRACE_VIEWER_DIR=...`; release builds
do the full cascade.

This pattern works because `trace-viewer/` is a *standalone* TS
project — no shared Rust code. The SPA bundle never needs to be
rebuilt because of a Rust change.

### Why preview is harder

The preview SPA *does* share Rust code (via
`wasm-quarto-hub-client`). A change to `pampa` requires:

- rebuild wasm-bindgen output → rebuild SPA → re-embed → relink
  `quarto-preview` → relink `quarto`.

Every step is slow on a fresh build. Caching helps in the steady
state but the dev-cycle penalty for cross-cutting Rust changes
is real.

### Proposed bootstrap strategy

1. **`quarto-preview/build.rs` mirrors `quarto-trace-server/`.**
   Placeholder fallback when the SPA isn't built. `cargo build`
   always succeeds; binaries built without the SPA cascade carry
   a "preview SPA not built" placeholder and refuse to start the
   server with a helpful error.

2. **Two-tier dev iteration:**
   - **UI iteration:** Vite dev server at `localhost:5173` with
     `QUARTO_PREVIEW_DIR=...` pointing the running `q2 preview`
     at the dev server. No Rust rebuild needed.
   - **Rust iteration on preview-specific code:** placeholder
     bundle, runtime override fallback. No npm rebuild needed.
   - **Cross-cutting Rust changes that affect the WASM
     surface:** full `cargo xtask build-preview` rebuild. This
     is the slow path, but it's slow *for a reason* —
     correctness across the WASM boundary.

3. **`cargo xtask build-preview` chains the sequence:**
   ```
   1. cargo xtask build-wasm   # wasm-quarto-hub-client → pkg/
   2. (cd hub-client && npm run build:preview)
   3. QUARTO_PREVIEW_EMBED_DIR=… cargo build -p quarto-preview
   ```
   And `cargo xtask build-all` extends to include the preview
   chain (similar to how it currently extends to include
   hub-client + trace-viewer).

4. **Decompose hub-client (resolves Q4 + this concern).** Split
   the parts the preview SPA needs (`<Preview>`,
   `<PreviewRouter>`, `Q2PreviewIframe`, the render-time
   services like `wasmRenderer`, `automergeSync`,
   `assetWalker`) into their own npm workspace package(s) that
   *both* hub-client and the preview SPA import. Two
   consequences:
   - hub-client (the editor) and the preview SPA each have a
     `build:preview` / `build:hub` script that produces a
     *separate* dist. Touching editor code never rebuilds the
     preview SPA, and vice versa.
   - Changes to `wasm-quarto-hub-client` invalidate both
     bundles, but that's correct — they both consume it.

   This is a non-trivial hub-client refactor and is properly
   tracked as its own pre-epic, since the decomposition is also
   useful independent of `q2 preview` (clearer code ownership,
   smaller hub-client editor bundle, separable test surfaces).

### Implications for phasing

- **Phase A** still ships first, but its first task is
  "decompose hub-client + define preview SPA package boundary",
  which is itself a substantial sub-epic. Phase A as originally
  written assumed a single new build target inside hub-client;
  this update changes it to a workspace-level reshape.
- The placeholder-bundle pattern from `quarto-trace-server` is
  copied wholesale; this is mechanical work.
- Q4 stops being "build flag vs runtime flag vs library refactor"
  — it's the library-refactor option as the *enabling* step for
  the rest of the plan.

### What this is *not*

This isn't a proposal to publish hub-client pieces to npm. The
shared packages live in the workspace (already-existing
`@quarto/quarto-sync-client` is the model). The reshape is
about boundaries inside the monorepo, not about external
distribution.

## Things explicitly out of scope (for the epic)

- **PDF preview** — separate epic; see Q6.
- **Shiny / observable runtime preview** — Q1 has special-cased
  paths for these. Q2's engine model doesn't yet, and replay
  doesn't apply to interactive runtimes. Future work.
- **Multi-user collaborative preview** — phase E mentions
  `--share`, but real collaboration belongs in `quarto hub`, not
  `quarto preview`.
- **`freeze` integration** — once `freeze` lands (it shares the
  profile-checkpoint substrate per the website epic), preview
  should honor frozen captures. Out of scope for the MVP because
  `freeze` itself isn't shipped.
- **Hot-reload of Lua filters in `_extensions/`** — phase B covers
  watching, but actually re-running them on demand may surface
  subtle ordering issues with the running pipeline. Treat as a
  D-phase polish item.

## Risks

1. **WASM pipeline ↔ EngineCapture wiring depth.** Phase C requires
   the WASM entry point `render_page_in_project` to accept an
   `EngineRegistry` override. Today it doesn't. The override has
   to be plumbed through `wasm-quarto-hub-client`'s
   `RenderToHtmlRenderer` / `Pass2Renderer` chain. Phase 2C of the
   q2-preview plans already plumbed configuration through these,
   so the seam is reachable, but the registry type isn't
   `Serialize` — we'd have to either lift the capture (which is
   serializable) and reconstruct the registry browser-side, or
   widen the WASM signature to take the capture directly. The
   latter is preferred.

2. **Cross-doc invalidation completeness.** Phase 8's dependency
   graph handles sidebar/prev-next/body-link/nav-dependency edges
   today. It does *not* know about, e.g., `include:` shortcodes,
   which are an `IncludeEntry` channel on the profile but not
   currently wired into the dependency-graph builder.

   User clarification (2026-05-11): the audit here is
   *feature-based*, not Q1-parity-based. Q1's preview is itself
   limited / best-effort; the goal is to enumerate all the kinds
   of cross-document dependencies Q2 *should* track (given
   DocumentProfile + edges) and decide which the MVP covers
   versus defers. The list at minimum needs to consider:
   `include` shortcodes, listing content globs (already partly
   in the graph), `bibliography`/`csl` paths, `theme` SCSS imports,
   shared resources, and `_extensions/` Lua filters. Each is a
   separate edge channel; some belong in the dep-graph builder,
   some are file-watch-only.

3. **DOM-stability under React's reconciler.** The whole pitch of
   q2-preview is that stateful JS survives edits. Phase 2C tests
   this for callouts/theorems, but the matrix of state-preserving
   widgets (Bootstrap dropdowns, MathJax, reveal slides, Leaflet
   maps) is larger and not yet exhaustively tested. Likely
   surfaces only when real users try it.

4. **Engine runtime discovery.** Today's `q2 render` discovers
   Jupyter kernelspecs via plan `2026-05-04-jupyter-kernelspec-
   discovery-and-errors.md`. The preview command needs the same
   plumbing — running engines requires resolved kernelspecs.
   Shouldn't be net-new work but is a hard dep.

5. **Initial sync time on large projects.** `HubContext::new`
   does an initial scan + push of *every* file in the project to
   automerge. For a 500-page project this is non-trivial and the
   user is waiting at a blank screen. Phase A ought to surface
   progress (or page-by-page lazy-loading) before the first
   real-world test.

## Resolved review items (2026-05-11)

The original draft asked the user to sign off on a series of
questions. The results are folded into the Open Questions
section above and into the body of the plan, but for the
historical record:

- **Q1** — pipeline-mode flag: option (c) (explicit `RenderMode`).
- **Q2** — server runs engines only; browser runs full pipeline.
  Confirmed; additional WYSIWYG motivation noted.
- **Q3** — per-document staleness *detection*; no automatic
  re-execution. Default `preview.engine: manual`.
- **Q4** — decompose hub-client (option c). Larger than originally
  scoped; tied to build-time concerns.
- **Q5** — project mode with `--no-project` escape: confirmed.
- **Q6** — HTML-only for MVP: confirmed.
- **Q7** — strict bind-to-loopback default: confirmed; rationale
  strengthened by the future code-execution + WYSIWYG surfaces.
- **Phasing A→B→C→D**: confirmed.
- **Crate layout** (separate `quarto-preview` crate): accepted in
  principle, but the hub-client decomposition (Q4) may force a
  larger reshape than originally envisioned. The "separate crate"
  remains the target; the path to get there has more shape.
- **Embed strategy** (`include_dir!` precedent): confirmed; the
  full design of how this interacts with build ordering is now
  the §"Build-time concerns" section.

## Resolutions from 2026-05-11 review #2

1. **WASM signature**: option (a). Widen
   `render_page_in_project` to take an optional `EngineCapture`;
   WASM constructs `EngineRegistry::with_replay` internally.
   This plan is already complex enough without the more general
   override seam.

2. **First-time eager run**: option (i). Eager engine run on
   first open of a never-previewed doc; subsequent code-cell
   changes surface the staleness affordance, never auto-execute.

   Rationale (user, 2026-05-11): Quarto already offers controls
   for users who want fast first-render — they can mark
   individual code cells `execute: false` (or set
   `execute: false` at the doc level). Documents that don't opt
   out get a real preview on first open. The pain point this
   plan avoids is *repeated* re-execution on every edit, not
   one-off first-render cost.

3. **Cross-doc dep audit**: deferred. Phase B covers the
   channels already encoded on `DocumentProfile` (the cheap
   ones). A full audit becomes a follow-up issue. This implies
   a new permanent affordance — see "Force-refresh invariant"
   below.

4. **Force-refresh invariant** (emerged from #3). Because we are
   knowingly deferring some cross-doc dependency channels, the
   preview UI **must always offer a manual "force re-render"
   button**, independent of staleness state. This is the user's
   escape hatch when our dep graph misses something. It also
   composes naturally with the staleness affordance — both end
   up rendering the same "click here to re-do work" surface,
   just with different copy.

   This is a new requirement, not a Phase-D nice-to-have. Add
   to Phase A's acceptance criteria.

5. **Crate / SPA layout**: the *physical* naming is bikesheddy,
   but there is a load-bearing **invariant** the layout must
   enforce:

   > The components that render the preview pane inside
   > hub-client and the components in the preview SPA must be
   > the *same* React components — same source files, same
   > imports, same tests. New preview-pane features landing in
   > hub-client land in `q2 preview` for free, and vice versa.

   This is the *reason* for the hub-client decomposition. The
   package boundary should be drawn so that violating this
   invariant requires a deliberate refactor, not an oversight.
   Concretely: any time someone wants to add code to "the
   preview pane in hub-client," they should be editing the
   shared package, not hub-client itself.

   Phase A's hub-client decomposition step will draw boundaries
   that enforce this — it's not just about build performance,
   it's about *feature parity by construction* between
   hub-client's preview pane and the SPA.

## Recommended next steps

Three pieces of work hand off cleanly from this epic:

1. **Hub-client decomposition sub-epic** (separate beads issue,
   blocked-by relationship: the q2 preview epic `bd-kw93`
   blocks-on this one). Owns the design sprint for package
   boundaries, the actual code reshape, and the invariant from
   §"Crate / SPA layout" above. This is the longest pole — it
   should land before Phase A picks up the SPA-embed mechanics.

2. **Phase A plan document** (`claude-notes/plans/2026-05-XX-
   q2-preview-phase-a.md`, written after item 1's design sprint
   sets the package boundaries). Focused on the CLI skeleton,
   build.rs placeholder, embedded SPA serving, and the
   force-refresh button. Engine-less, so it's a tight first
   slice once the decomposition is settled.

3. **Cross-doc dep audit follow-up** (separate beads issue,
   `related` to the epic). Tracks Phase B's Quarto-feature
   enumeration: which dependency channels are encoded on
   `DocumentProfile` today, which want to be added, and which
   stay manual-refresh-only.

Items 1 and 3 can be filed now without committing to phase-plan
content. Item 2 waits on item 1's outputs.

## Out-of-band: post-merge cleanup

This branch (`feature/q2-preview`) carries the `q2-preview` format
plus its phase 1–8 work. Once the preview command lands, the
`format: q2-preview` literal becomes an implementation detail — end
users shouldn't write it themselves. The format identifier should
probably stay accessible (it's useful for debugging and for the
hub-client editor view), but it should be advertised as
preview-internal in docs.

## Reference material

- `claude-notes/plans/2026-05-04-q2-preview-plan-{1..8}.md` — the
  q2-preview format itself (this branch).
- `claude-notes/plans/2026-04-23-website-project-epic.md` —
  established that `quarto preview` would be a local hub-client
  instance ("design decision 5"). This plan is the realization of
  that promise.
- `claude-notes/plans/2026-04-27-websites-phase-8.md` — Mode A vs
  Mode B + dependency graph, the substrate for cross-doc
  invalidation.
- `claude-notes/plans/2026-05-03-replay-engine.md` — replay engine
  + capture format.
- `claude-notes/plans/2026-02-02-quarto-hub-subcommand.md` —
  pattern for adding a new `quarto <name>` subcommand.
- `claude-notes/plans/2026-03-03-hub-no-local-watch.md` —
  standalone-mode and `--data-dir` were added for exactly this
  kind of ephemeral-server use case.
- `crates/quarto-trace-server/{build.rs,src/lib.rs}` — the
  `include_dir!` SPA-embedding precedent.
- `crates/quarto-hub/src/{server,context,watch,sync}.rs` — the
  pieces we wrap.
- `external-sources/quarto-cli/src/command/preview/preview.ts` —
  Q1 preview for behavioral parity reference (not a copy target).
