# q2 preview — Phase D plan

**Epic:** bd-kw93 (q2 preview)
**Predecessor:** Phases A, B, C all merged on `feature/q2-preview-command`.
**Date:** 2026-05-14
**Status:** All six sub-tasks (D.1 → D.6 + D.5) merged 2026-05-14. Phase D complete.

## Progress

- [x] **D.1** (bd-kw93.8) — Browser-tab-on-startup + port-conflict retry. Merged 2026-05-14.
  - [x] `open = "5"` added to `crates/quarto/Cargo.toml` (shells out to platform-native launcher; no heavy transitive tree).
  - [x] `open_browser_or_log(url, suppress)` in `crates/quarto/src/commands/preview.rs` — fires `open::that(url)` when not suppressed; warns + continues on failure (the URL is already printed for copy-paste). `--no-browser` short-circuits cleanly.
  - [x] `validate_explicit_port(host, port)` pre-probes when the user pinned a specific port; on `AddrInUse`, returns a clean error naming `--port 0` as the escape hatch instead of the raw bind failure from inside `quarto_hub::server::run_server_with`.
  - [x] Side fix: `--port 0` previously printed `http://127.0.0.1:0/` (broken URL). Now `Some(0)` and `None` are equivalent — both probe for an OS-assigned free port so the printed URL is reachable.
  - [x] Tests: 3 new unit tests in `commands::preview::tests` (port-0 probe ok, bound-port error message includes the port number + `--port 0` hint, `open_browser_or_log` no-ops when suppressed). `cargo nextest run --workspace`: 8929/8929 pass (8926 → 8929, +3 D.1 tests).
  - [x] Binary smoke: with a Python listener holding 127.0.0.1:50500, `q2 preview /tmp/q2-d1-smoke --no-browser --port 50500` emits `Error: port 50500 on 127.0.0.1 is already in use; pass --port 0 to let the OS pick a free port, or omit --port for the default probe behaviour`. With `--port 0`, prints `→ http://127.0.0.1:50617/` (real OS-picked port).
- [x] **D.2** (bd-kw93.13) — Initial-path resolution (positional `.qmd` → that page; project mode → project index). Merged 2026-05-14.
  - [x] `resolve_project_and_initial_page` in `crates/quarto/src/commands/preview.rs` walks up from a file path looking for `_quarto.yml`; on hit, project root becomes the ancestor dir and `initial_page` is the path-relative-to-root. For directories, returns `index.qmd` when present, else `None`. Single-file mode (no `_quarto.yml` ancestor) preserves Phase A semantics (project root = the file path itself).
  - [x] `build_boot_url(host, port, initial_page)` appends `?page=<rel>` when present. New `percent_encode_path` helper RFC-3986-encodes the value, leaving literal `/` alone so paths read naturally.
  - [x] SPA: new `q2-preview-spa/src/pickInitialPage.ts` parses `window.location.search`, validates the path is in the file index, rejects `..` traversal, falls through to `firstQmd` on miss. Wired into `PreviewApp.tsx` boot, replacing the unconditional `firstQmd?.path ?? null`.
  - [x] Tests: 9 new Rust unit tests (dir-with-index, dir-without-index, file-in-project, file-at-project-root, file-without-`_quarto.yml`, two `build_boot_url` cases, two `percent_encode_path` cases) + 8 SPA unit tests on `pickInitialPage` (query hit, decode, fall-throughs, traversal rejection, empty-value handling, no-qmd index) + 2 new SPA integration tests in `PreviewApp.integration.test.tsx` (?page= seeds activeFile; ?page= names unknown file falls back to firstQmd). Existing 16 SPA integration tests unaffected.
  - [x] `cargo nextest run --workspace`: 8938/8938 pass (8929 → 8938, +9 D.2 Rust tests). `npm test` (SPA): 8/8 pass. `npm run test:integration` (SPA): 18/18 pass.
  - [x] Binary smoke against `/tmp/q2-d2-smoke`:
    - `q2 preview <dir-with-index>` → `→ http://127.0.0.1:50813/?page=index.qmd`
    - `q2 preview <project>/posts/intro.qmd` → `→ http://127.0.0.1:50814/?page=posts/intro.qmd` + `project_root=/private/tmp/q2-d2-smoke` (walked up to `_quarto.yml`)
    - `q2 preview <dir-without-index>` → `→ http://127.0.0.1:50815/` (no `?page=`)
- [x] **D.3** (bd-kw93.9) — Static-file resource verification (CSS in `_extensions/`, images, theme files round-trip through samod binary-doc sync). Merged 2026-05-14.
  - [x] Two bugs surfaced + fixed in scope:
    - The watcher allow-list (`is_preview_relevant` in `crates/quarto-hub/src/watch.rs`) didn't include `.css` — B.1 covered `.tsx` + images but stopped short. Added `"css"` to the extension allowlist; new unit test asserts `styles.css`, `_extensions/foo/foo.css`, and `THEME.CSS` are accepted. `.scss` / `.sass` / `.less` deliberately omitted — preview-pipeline support for editing them is unverified, tracked as a follow-up if user demand surfaces.
    - The SPA's `setSyncHandlers` call wired `onFilesChange` / `onFileContent` / `onCapturesChange` but not `onBinaryContent`. Binary doc edits (images, SVGs) landed in samod without bumping `contentTick`, leaving the SPA blind to them. Added the missing handler.
  - [x] Added `window.__renderTicks` counter to `PreviewApp.tsx` — increments on every completed render attempt (success path + caught failure path), letting Playwright assert "this edit triggered a re-render" without inferring through DOM diffs. Also addresses item #1 of bd-0mji ("SPA render-event hook").
  - [x] New Playwright spec `q2-preview-spa/e2e/static-resources.spec.ts`: edits `_extensions/sentinel/sentinel.css` (proves D.3's allow-list fix), then edits `assets/logo.svg` (proves D.3's binary-handler fix). Each test asserts `__renderTicks` increments within 5s of the on-disk write.
  - [x] DOM-level "the CSS rule is actually applied to the rendered iframe" is deferred — the preview-pipeline pickup of `_extensions/`-supplied CSS is a separate question and will need its own scope if the watcher/sync round-trip alone isn't enough. The pin here is the watcher → sync → notify contract.
  - [x] `cargo nextest run --workspace`: 8938/8938 pass. SPA integration: 21/21. Playwright (all specs): 11/11 (9 prior + 2 new).
- [x] **D.4** (bd-kw93.10) — Diagnostics surface: render errors / engine errors / parse errors render through `PreviewErrorOverlay` instead of failing silently. Merged 2026-05-14.
  - [x] New `renderError: Error | null` state slot in `PreviewApp.tsx`, distinct from the boot-time `error` slot. Render-pipeline failures (WASM throw or `result.success === false`) populate `renderError` *without* flipping `boot: 'error'` — the iframe keeps the last-good `astJson` mounted and `<PreviewErrorOverlay collapsed>` overlays on top. A successful render clears `renderError`.
  - [x] First-render failure (no good `astJson` yet) takes a dedicated branch that shows the overlay terminal-style — there's no underlying render to fall back to.
  - [x] Engine errors via `CaptureRef.lastError` continue to surface through `StaleCaptureOverlay` (already wired in C.5); D.4 pins this with a new integration test so a future refactor doesn't regress it.
  - [x] Drive-by fix: `StaleCaptureOverlay.integration.test.tsx` had a pre-existing TS error (`mock.calls[0] as [string, RequestInit]` against a zero-param mock) that broke `npm run build`. Fixed by giving the fetch mock explicit parameter types. The production build (`tsc -b && vite build`) is now clean again.
  - [x] Tests: 3 new SPA integration tests (last-good render stays visible when render fails; overlay clears on next successful render; engine `lastError` surfaces via `StaleCaptureOverlay`). All 21 integration + 8 unit + workspace nextest 8938/8938 pass.
- [x] **D.5** (bd-kw93.11) — User-facing documentation in `docs/` for `q2 preview`. Merged 2026-05-14.
  - [x] New `docs/q2-preview.qmd` covers: quick-start examples, what "live" means (DOM-stable re-renders + the dep-graph filter), the three `preview.engine` values (manual / auto / off), error-overlay UX, flag reference, known limitations (HTML-only, loopback-only, per-session cache, single-hop includes, conservative non-qmd filter). Written in user voice with no `claude-notes/` or `bd-…` references.
  - [x] Added to `docs/_quarto.yml` navbar between `Quarto Hub` and `Bug reports`. Verified `quarto render docs/q2-preview.qmd` produces clean HTML.
  - [x] Tightened the clap `///` doc-comments in `crates/quarto/src/main.rs` so `q2 preview --help` reads as user-voice documentation (was leaning developer with `claude-notes/plans/...` references, "Q1 flag set" inventory, and internal-architecture mentions of samod and SPA bundles). Verified rendered `--help` against the binary.
  - [x] Filed `bd-9ofu` (P3) to track the broader question: `docs/` is titled "Quarto-markdown" and is pitched as a reference for the dialect + library internals, not as a front door for Q2 CLI commands. As more Q2 commands gain user-facing surface area they'll face the same placement question. D.5 ships docs at the most natural existing home; the IA decision is deferred.
- [x] **D.6** (bd-kw93.12) — Dep-graph filter for re-renders (unblocks bd-0mji's regression tests). Merged 2026-05-14.
  - [x] New `crates/quarto-preview/src/deps.rs` module with a `GET /api/preview/deps?page=<rel>` endpoint registered in `extend_with_preview`. The handler reads the page's qmd source and runs a single-hop `{{< include … >}}` shortcode extractor (regex-based; the full `ProjectDependencyGraph` requires running per-page render pipelines and is much heavier than D.6 needs). Returns `{ "deps": [...] }` as project-relative forward-slash paths. Errors downgrade to "no deps" + a log line — a filter that misses a dep is worse than one that over-broadcasts, so fail-open.
  - [x] SPA: `PreviewApp.tsx` gets a new `deps: Set<string> | null` state slot. A `useEffect` fetches the active page's deps from the new endpoint on activeFile change and after every contentTick bump (so a newly-added include shortcode in the active page becomes visible to the filter immediately). `onFileContent(path)` now calls `shouldRerenderForTextChange(path, activeFile, deps)` — drops sibling `.qmd` edits when they're not in the active page's dep set, passes everything else through. Non-qmd edits (CSS, `_quarto.yml`, `_metadata.yml`, `.tsx`, images) always pass; only `.qmd` files get filtered. `deps === null` = unknown ⇒ fail-open. `onBinaryContent` stays unfiltered (image-ref extraction is deferred — tracked as a follow-up).
  - [x] Tests: 11 unit tests in `deps::tests` (unquoted/double-quoted/single-quoted include, page-dir-relative resolution, parent-dir resolution, multi-include dedup+sort, named-arg form, single-hop pin, path-normalize). 2 new Playwright specs in `q2-preview-spa/e2e/dep-graph-filter.spec.ts` (sibling-edit drops; self-edit re-renders). Phase B.3's `include-shortcode.spec.ts` (positive: edit included file → re-render) continues to pass — confirms the filter doesn't false-block real deps.
  - [x] `cargo nextest run --workspace`: 8949/8949 pass. SPA: 21/21 integration + 8/8 unit. Playwright: 13/13.
  - [x] Closes bd-0mji item #2 (negative-case regression test). bd-0mji item #1 (SPA render-event hook) was already closed by D.3's `window.__renderTicks`. bd-0mji can be closed.

  **Follow-ups (out of scope for the D.6 MVP):**
  - Transitive include traversal. Today the dep set is single-hop only. Documented in `deps.rs` module docs.
  - Image / bibliography / theme-CSS dep extraction. Non-qmd edits all pass the filter today; tightening to "only edits actually referenced by the active page" requires extracting these channels.
  - Cross-doc channels per bd-56b0's audit.

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
