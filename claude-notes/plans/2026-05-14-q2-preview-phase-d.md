# q2 preview — Phase D plan

**Epic:** bd-kw93 (q2 preview)
**Predecessor:** Phases A, B, C all merged on `feature/q2-preview-command`.
**Date:** 2026-05-14
**Status:** Sub-task issues filed bd-kw93.8 through bd-kw93.13; D.1–D.6 ready to pick up (mostly parallelisable).

## Progress

- [x] **D.1** (bd-kw93.8) — Browser-tab-on-startup + port-conflict retry. Merged 2026-05-14.
  - [x] `open = "5"` added to `crates/quarto/Cargo.toml` (shells out to platform-native launcher; no heavy transitive tree).
  - [x] `open_browser_or_log(url, suppress)` in `crates/quarto/src/commands/preview.rs` — fires `open::that(url)` when not suppressed; warns + continues on failure (the URL is already printed for copy-paste). `--no-browser` short-circuits cleanly.
  - [x] `validate_explicit_port(host, port)` pre-probes when the user pinned a specific port; on `AddrInUse`, returns a clean error naming `--port 0` as the escape hatch instead of the raw bind failure from inside `quarto_hub::server::run_server_with`.
  - [x] Side fix: `--port 0` previously printed `http://127.0.0.1:0/` (broken URL). Now `Some(0)` and `None` are equivalent — both probe for an OS-assigned free port so the printed URL is reachable.
  - [x] Tests: 3 new unit tests in `commands::preview::tests` (port-0 probe ok, bound-port error message includes the port number + `--port 0` hint, `open_browser_or_log` no-ops when suppressed). `cargo nextest run --workspace`: 8929/8929 pass (8926 → 8929, +3 D.1 tests).
  - [x] Binary smoke: with a Python listener holding 127.0.0.1:50500, `q2 preview /tmp/q2-d1-smoke --no-browser --port 50500` emits `Error: port 50500 on 127.0.0.1 is already in use; pass --port 0 to let the OS pick a free port, or omit --port for the default probe behaviour`. With `--port 0`, prints `→ http://127.0.0.1:50617/` (real OS-picked port).
- [ ] **D.2** (bd-kw93.13) — Initial-path resolution (positional `.qmd` → that page; project mode → project index).
- [ ] **D.3** (bd-kw93.9) — Static-file resource verification (CSS in `_extensions/`, images, theme files round-trip through samod binary-doc sync).
- [ ] **D.4** (bd-kw93.10) — Diagnostics surface: render errors / engine errors / parse errors render through `PreviewErrorOverlay` instead of failing silently.
- [ ] **D.5** (bd-kw93.11) — User-facing documentation in `docs/` for `q2 preview`.
- [ ] **D.6** (bd-kw93.12) — Dep-graph filter for re-renders (unblocks bd-0mji's regression tests).

> Note: the sub-task IDs are out of D-numeric order because of a re-filing during issue creation. The plan section ordering (D.1 → D.6) reflects the logical sequence; the bd-IDs are stable identifiers.

## Goal

Phase A–C delivered the load-bearing engineering: serve the SPA, watch the project, sync edits, run engines server-side, replay in WASM, surface staleness, cache captures. Phase D is the difference between "the thing works if you read the code" and "the thing is a CLI a colleague can pick up." Five out of six items are user-visible polish; D.6 is a performance correctness item (avoiding redundant re-renders) that also unblocks a regression-test gap left in Phase B.

Phase D is **not** about adding features. Each item below has a concrete acceptance criterion. If a sub-task starts to grow, that's a signal we're sliding into Phase E and should stop.

## Settled decisions

Captured here so they don't need re-litigating in sub-task plans.

- **Browser-open mechanism (D.1):** use the `open` crate (already in the wider Rust ecosystem; no transitive baggage). Cross-platform; treats failure as "log + continue" rather than fatal. See `crates/quarto-trace-server/` for precedent if it's there.
- **Port-conflict policy (D.1):** when `--port N` is explicitly requested and `N` is bound, error out with a clear message — don't silently bind elsewhere. When `--port` is unspecified, the existing `probe_free_port` already finds a free OS-assigned port; that path is unchanged.
- **Initial path (D.2):** carry the requested path from CLI → server (via a query string on the boot URL, e.g. `/?page=posts/intro.qmd`) → SPA, which seeds `activeFile` from the query rather than falling back to `firstQmd`. Project mode resolves to `index.qmd` if present, else `firstQmd`.
- **Static-file scope (D.3):** within scope are CSS/SCSS in `_extensions/`, images referenced from `.qmd` files, project-scoped `resources:` declarations, and theme files under the project root. Out of scope: any resource that lives outside the project root (preview already refuses these).
- **PreviewErrorOverlay scope (D.4):** the overlay is already wired for boot/connection errors (see `PreviewApp.tsx:291`). D.4 extends it to surface *render-pipeline* errors emitted by the WASM renderer (e.g. a malformed `.qmd` that fails parse), and *engine* errors propagated through Phase C.5's `CaptureRef.lastError` field for captures whose `state === 'error'`. The visual treatment can be the same component; the message source differs.
- **Docs language (D.5):** write in user-facing tone (not implementation detail). Mirror the structure of TypeScript Quarto's `quarto preview` docs where applicable; cross-reference settled `preview.engine` config and the staleness affordance.
- **Dep-graph filter (D.6):** use the existing `ProjectDependencyGraph` infrastructure (already used by `q2 render` for incremental rebuilds, per `claude-notes/designs/document-profile-contract.md`). Filter happens at the SPA's content-change listener — only bump `contentTick` when the edited file is the active page **or** a dependency of the active page. This restores the strict criterion deferred from Phase B.4.

## Open questions (resolve in sub-task plans)

- **Q-D1:** Should `--no-browser` also suppress the printed URL? *No* — the URL print is the user's primary affordance to copy/paste. `--no-browser` only suppresses the auto-open.
- **Q-D2:** When the requested file is *not* in the project, error vs. fall back? *Error* — single-file mode is the established escape hatch; if the user passed a path that doesn't exist or isn't tracked, that's a real error.
- **Q-D4:** When *both* a connection error and a render error are in flight, which overlay wins? *Connection* — render errors imply a working connection. If connection drops mid-render, the connection overlay supersedes.
- **Q-D6:** Should the dep-graph filter apply to the *eager re-render trigger* (every change) or also to the *first paint*? *Re-renders only* — the first paint always shows the active page regardless of dep-graph state.

## Dependency order

D.1, D.2, D.3, D.4, D.5, D.6 are largely independent. Suggested implementation order (most user-visible first):

```
D.1 (browser-open)  ────┐
D.2 (initial path)  ────┤
D.4 (error overlay) ────┼──→ D.5 (docs, references all of the above)
D.3 (static files)  ────┤
D.6 (dep-graph)     ────┘
```

D.5 (docs) should be last so it reflects the final UX. D.6 can land in parallel with anything — it's a performance correctness fix with no surface implications.

## Sub-task details

### D.1 — Browser-open + port-conflict retry

**Affects:** `crates/quarto/src/commands/preview.rs`.

**Today:**
- `--no-browser` is parsed and `args.no_browser` is read, but the auto-open branch just prints "open the URL manually" (line 107).
- Port-conflict behaviour: with `--port N`, if `N` is bound, the hub-server bind fails with an opaque error inside `quarto_hub::server::run_server_with`. Without `--port`, `probe_free_port` finds a free port — works today.

**Changes:**

1. Add a dependency on the `open` crate (or `webbrowser`; pick one). When `!args.no_browser`, fire `open::that(&url)` after the server prints its banner. On failure, log a warning and continue — never fatal.
2. When `--port N` is explicitly set and binding fails because the port is in use, return a clear error: `port N is already in use; pass --port 0 to let the OS pick a free port` rather than the raw `bind` error.

**Test plan:**

1. Unit: a helper `open_browser_or_log(url, no_browser)` is testable; mock the open call (or skip it on `no_browser`). Assert it doesn't panic on a non-existent URL.
2. Integration: `q2 preview --no-browser` produces the URL but doesn't try to open. (Existing tests cover the no-browser path; adding an explicit assertion is enough.)
3. Manual smoke (no automated test): `q2 preview` on a real project opens the default browser to the boot URL.

**Acceptance:** unit + integration tests pass; manual smoke recorded in the commit body.

---

### D.2 — Initial-path resolution

**Affects:** `crates/quarto/src/commands/preview.rs`, `q2-preview-spa/src/PreviewApp.tsx`.

**Today:**
- The CLI accepts `args.path: Option<PathBuf>` which is treated as the project root (or a single-file pseudo-project). It does not distinguish "file the user wants to view" from "project root."
- The SPA picks `firstQmd` from the discovered file index (`PreviewApp.tsx:199-203`).

**Changes:**

1. CLI: if `args.path` resolves to a *file* (not directory), compute its project-relative path and include it in the URL printed/opened (`/?page=<rel-path>`). If `args.path` is a directory or unset, omit the query.
2. SPA: parse `?page=` from the URL on boot; seed `activeFile` from it if present (after validating the path is in the index). Fall through to `firstQmd` or `index.qmd` otherwise.
3. CLI: when no `args.path` is set, look for `index.qmd` at the project root and prefer it over `firstQmd` for the URL hint.

**Test plan:**

1. Unit (Rust): a `resolve_initial_page(args, project_root)` function returns `Some("posts/intro.qmd")` for `q2 preview /proj/posts/intro.qmd` (file-mode), `Some("index.qmd")` for `q2 preview /proj` (project-mode with index), `None` for `q2 preview /proj` (project-mode without index).
2. Unit (TS): a `pickInitialPage(queryString, files)` helper returns the queried path on hit, falls through to `firstQmd` on miss/unset.
3. Integration: the existing `tests/boot.rs` adds a positive case — start with a fixture whose `firstQmd` is `b.qmd` but ask for `a.qmd` via positional arg; assert the SPA's active-file state reflects `a.qmd` (poll via `samod` or via Playwright if needed).
4. SPA integration test: pre-set `window.location.search` to `?page=foo.qmd`, mount `PreviewApp`, assert `activeFile === 'foo.qmd'` once boot completes.

**Acceptance:** unit + 1 integration test per surface; existing tests unaffected.

---

### D.3 — Static-file resource verification

**Affects:** tests only (no production code changes expected unless a bug is found).

**Today:** hub already syncs binary docs through samod. Phase A.4 verified the SPA bundle ships; Phase B.1 broadened the watcher allow-list to `.tsx`, images, `_extensions/`, etc. What's *not* verified: end-to-end "edit `_extensions/foo/foo.css` on disk → the running preview reflects the change."

**Changes:**

1. Add a Playwright e2e test fixture with `_extensions/foo/foo.css` containing a sentinel selector (`.q2-test-sentinel { color: rgb(123, 45, 67); }`) and a `.qmd` that references the class.
2. Edit `foo.css` while the preview is running (change the colour to a new sentinel); assert the rendered DOM reflects the new colour within 2 s.
3. Repeat for an image asset: replace `assets/logo.png` with a different file, assert the `<img>` element re-fetches.

**Test plan:**

1. e2e (Playwright): `q2-preview-spa/e2e/static-resources.spec.ts` covers both cases.
2. If the test reveals a bug (e.g. CSS isn't being re-applied after a write), file as a follow-up and fix in scope of D.3.

**Acceptance:** at least one new Playwright spec passes under `cargo xtask verify --e2e`. Any bugs surfaced get either fixed or filed.

---

### D.4 — Diagnostics surface

**Affects:** `q2-preview-spa/src/PreviewApp.tsx`, possibly `PreviewErrorOverlay`.

**Today:**
- `PreviewErrorOverlay` is wired for connection errors and `state.error` (boot-time failures). See `PreviewApp.tsx:291`.
- Render errors from the WASM renderer currently throw; the active page slot goes blank.
- Engine errors (from C.5's re-execute path) populate `CaptureRef.state = 'error'` + `lastError`. The current stale-capture overlay shows the error state but the error message itself isn't surfaced.

**Changes:**

1. Catch WASM-renderer exceptions in the active-page render `useEffect` (PreviewApp.tsx:222+); route into a render-error `state` slot that the overlay reads.
2. Wire `CaptureRef.lastError` into the stale-capture overlay or a sibling error overlay — show the engine's error message to the user, with a Re-execute button to retry.
3. Visual treatment: prefer reusing `PreviewErrorOverlay` (single component) with a `kind: 'render' | 'connection' | 'engine'` discriminator if differentiation helps.

**Test plan:**

1. Unit (SPA): mount `PreviewApp` with a doc that throws on render (e.g. a synthetic invalid AST). Assert the overlay appears within one render tick.
2. Unit (SPA): mount `PreviewApp` with a `CaptureRef.state = 'error'` sidecar entry; assert the error message text is in the DOM.
3. Integration (e2e): edit a `.qmd` to introduce a parse error; assert the overlay shows before the next valid edit hides it.

**Acceptance:** 2 unit + 1 e2e; existing overlay tests unaffected.

---

### D.5 — Documentation

**Affects:** `docs/` (Quarto website).

**Changes:**

1. New `docs/q2-preview.qmd` (or under a sub-folder if the user prefers). Cover:
   - What `q2 preview` does (one-paragraph elevator pitch).
   - Basic usage (`q2 preview`, `q2 preview foo.qmd`).
   - Flags (`--no-browser`, `--port`, `--data-dir`, `--no-project`).
   - `preview.engine` config (`manual` / `auto` / `off`) — link to Phase C.6 settled decisions.
   - Stale-capture affordance — link to C.5.
   - Limitations / known gaps (single-format HTML, per-session cache, etc.).
2. Add an entry to `docs/_quarto.yml` navigation.
3. No new claude-notes/ — this is *user-facing*.

**Test plan:** docs build cleanly: `cd docs && quarto render` (or whatever the existing workflow is).

**Acceptance:** docs build succeeds; manual review of the rendered page reads well.

---

### D.6 — Dep-graph filter for re-renders

**Affects:** `q2-preview-spa/src/PreviewApp.tsx`, possibly `quarto-preview-renderer`.

**Today:**
- Every content change bumps `contentTick`, which fires the render `useEffect` for the active page — even when the edit is in an unrelated sibling file. This is the relaxed criterion from Phase B.4 (criterion 3).
- The dep graph exists server-side (`crates/quarto-core/src/project/dependency_graph.rs`) but isn't exposed to the SPA.

**Changes:**

1. Decide where the filter runs: server-side (only forward content changes to the SPA that the active page actually depends on) or client-side (SPA holds a dep-graph snapshot, filters its own `contentTick` bumps). Settled in the sub-task plan; current preference is **server-side** because the dep graph is already there and SPA-side caching introduces cache-invalidation problems.
2. Implementation: emit a new sync-client signal (or extend an existing one) carrying "the active page is X; only notify content changes for X and its dependencies." The active page is known by the SPA, communicated to the server via a `WebSocket` message or a small HTTP endpoint.
3. The SPA's `onTextChange` handler filters incoming bumps against the dep set.

**Test plan:**

1. Unit (server): given a dep graph (`a.qmd` includes `b.qmd`), a content change to `c.qmd` should NOT signal `a.qmd`'s SPA listener. Use a fake `ProjectDependencyGraph`.
2. Unit (SPA): same, client-side filter variant if we go with that.
3. Integration (e2e, lifts bd-0mji acceptance #2): edit an unrelated sibling, assert `__renderTicks` doesn't increment.
4. Integration (e2e, bd-0mji acceptance #1): edit a true dependency, assert `__renderTicks` increments.

**Acceptance:** unit + 2 e2e; bd-0mji can close after D.6 lands.

---

## Out of scope for Phase D

- **Hot-reload of `.tsx` from disk** (epic Phase E.1).
- **Multi-window sync** via samod (Phase E.2 — likely free; defer verification).
- **`--share <url>`** remote-sync mode (Phase E.3).
- **Cross-doc dep channel audit** (bd-56b0) — research task; informs *future* dep-graph expansion but doesn't block D.6 (which uses today's `ProjectDependencyGraph` as-is).
- **Per-project cache location** (bd-wo59) — Phase C.7 follow-up.

## Risks

1. **`open` crate transitive deps.** If the crate pulls in too much weight, an inline implementation (`std::process::Command` per-platform) is a viable fallback. Decide in D.1 sub-task plan.
2. **Initial-path query strings vs samod URL routing.** The SPA currently treats `/` as the entry. Adding `?page=...` mustn't break the existing routing. Unit-test the query parsing in isolation.
3. **Dep-graph filter timing.** A filter that misses a real dependency edge (false negative) is worse than no filter — the user sees stale output and has no warning. D.6 must include a regression test that exhaustively covers the cross-doc edge types from bd-56b0's audit (include shortcodes, listings, bibliography, theme imports, project-scoped resources).
4. **Static-file e2e flakiness.** Browser CSS/image cache may interfere; the test may need explicit cache-bust query strings or a hard-reload.

## Pre-flight investigation receipts (2026-05-14)

- `PreviewErrorOverlay` is at `ts-packages/preview-renderer/src/overlays/PreviewErrorOverlay.tsx` and re-exported from `@quarto/preview-renderer`. Already imported by `PreviewApp.tsx:49`; mounted at `PreviewApp.tsx:291`.
- Current `firstQmd` selection: `q2-preview-spa/src/PreviewApp.tsx:199-203`.
- Browser-auto-open placeholder: `crates/quarto/src/commands/preview.rs:107`.
- Phase B watcher allow-list (Phase B.1, bd-z529): already covers `.tsx`, images, `_extensions/`, etc. — D.3 should not need watcher work.
- `ProjectDependencyGraph` lives in `crates/quarto-core/src/project/dependency_graph.rs`; the analyzer is exposed via `ProjectContext` after `discover`. Reach into it from `quarto-preview` via the existing `ProjectContext::discover` path the capture driver already uses.
- Existing Playwright suite: `q2-preview-spa/e2e/basic-preview.spec.ts` (4 specs from bd-vpsy/A.7). New D.3 / D.4 / D.6 specs slot into the same folder.
- `docs/` is a Quarto website with `_quarto.yml` driving navigation; new pages added to `docs/q2-preview.qmd` need a navigation entry.
