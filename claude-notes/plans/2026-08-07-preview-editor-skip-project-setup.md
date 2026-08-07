# Skip project-set setup for `q2 preview --ui editor` (ephemeral hub)

Strand: bd-zf4ryvuq

## Overview

`q2 preview --ui editor` boots the browser to a hub-client share URL
(`#/share/<docId>?server=%2Fws&file=…&name=…`, built by
`build_editor_boot_url` in
[preview.rs](../../crates/quarto/src/commands/preview.rs)). Before the
share handler can land the user in the editor, `App.tsx` gates rendering
on `projectSetState.status`: a fresh browser profile (`needs-setup`) or
one with legacy IDB projects (`needs-migration`) gets the
`ProjectSetSetup` create/migrate page instead of the preview. Preview
binds an ephemeral port by default, so IndexedDB is almost always fresh
— every preview boot shows onboarding for a project set the user does
not need (the hub is a throwaway per-session server; see the sibling
plan
[2026-08-07-ephemeral-hub-secrets-for-preview.md](2026-08-07-ephemeral-hub-secrets-for-preview.md)).

**Approach:** the preview server's boot URL carries `ephemeral=true` on
the share route. hub-client captures the flag once at mount (before the
share handler clears the URL), then:

1. **Silently establishes the personal root set**, mirroring the
   existing invite-first onboarding effect for `join-collection`
   (`App.tsx`): `needs-setup` → `createProjectSet(DEFAULT_SYNC_SERVER)`;
   `needs-migration` → `migrateProjects(DEFAULT_SYNC_SERVER)`. In the
   preview-embed build `DEFAULT_SYNC_SERVER` is `/ws` (hub-client
   `package.json` `build:preview-embed`) — the ephemeral hub itself.
   Fire-once ref guard, same retry-loop rationale as join-collection.
2. **Bypasses the `ProjectSetSetup` gates** (`needs-setup` /
   `needs-migration` / `error`) when the flag is set, so the user goes
   straight to the connecting state and then the editor.

Production is unaffected: `buildShareableUrl` / `buildHashRoute` never
emit the param, so only preview-generated boot URLs carry it.

## Work Items

### Phase 1 — Tests first (TDD)

- [x] `crates/quarto/src/commands/preview.rs`: update
  `build_editor_boot_url_emits_share_route_in_hash` and
  `build_editor_boot_url_strips_automerge_prefix` to expect
  `&ephemeral=true`; add a test asserting the flag is present and is the
  last param. Confirm fail. (Confirmed: both failed pre-implementation.)
- [x] `hub-client/src/utils/routing.test.ts`: share-route parse tests —
  `&ephemeral=true` → `ephemeral: true` on the route; absent param →
  field absent (conditional spread, mirrors `anchor`); non-`true` value
  → absent. Confirm fail. (Confirmed: parse test failed
  pre-implementation.)

### Phase 2 — Rust implementation

- [x] `build_editor_boot_url`: append `&ephemeral=true`; extend the doc
  comment (the param marks the serving hub as ephemeral; client skips
  project-set onboarding).
- [x] `cargo nextest run -p quarto build_editor_boot_url` — 3 passed.

### Phase 3 — hub-client implementation

- [x] `routing.ts`: `ShareRoute` gains optional `ephemeral?: boolean`
  (doc comment: only preview boot URLs set it; never emitted by
  `buildHashRoute`); parse via conditional spread, mirroring the
  `anchor` pattern. `routesEqual` unchanged (flag is not a location
  discriminator).
- [x] `App.tsx`:
  - Capture once at mount: `useState(() => { const r =
    parseHashRoute(window.location.hash); return r.type === 'share' &&
    r.ephemeral === true; })` (mirrors the `authErrorReason` pattern).
  - Silent-establish effect after the join-collection one, with a
    fire-once ref (`ephemeralRootInitiatedRef`).
  - Gate skips: `!ephemeralHub && …` on the needs-setup/needs-migration
    gate and the error gate.
- [x] `npx vitest run src/utils/routing.test.ts` (85 passed); `tsc -b`
  clean.

### Phase 4 — End-to-end verification

- [x] Rebuild the embedded editor: `cargo xtask build-hub-client-embed`
  (runs hub-client `build:preview-embed`), then `cargo build --bin q2`.
- [x] Boot `target/debug/q2 preview examples/websites/01-minimal
  --ui editor --no-browser`, confirm the printed URL carries
  `ephemeral=true`. Observed:

  ```
  → http://127.0.0.1:53411/#/share/qe5rdRWvXH5eVGY5azrZ5bMZQtE?server=%2Fws&file=index.qmd&name=01-minimal&ephemeral=true
  ```

- [x] Drive a real browser (Playwright script, fresh profile) against
  the printed URL: assert the editor loads the file and the
  ProjectSetSetup page ("Create New Project Set" / migration) never
  appears. Evidence (script drove headless Chromium with a fresh
  profile — no IDB/localStorage):

  ```
  PASS: .editor-container became visible
  PASS: ProjectSetSetup never rendered
  PASS: editor shows index.qmd
  final url: http://127.0.0.1:53411/#/p/<uuid>/file/index.qmd
  ```

  Control run against the same server with the param removed (the
  "before" behavior): `ProjectSetSetup visible: true`, editor not
  visible. Two console errors appear in BOTH runs and are pre-existing,
  unrelated to this change: `401 /auth/me` (expected in auth-less
  builds — projectSetService.ts documents it always 401s without
  VITE_GOOGLE_CLIENT_ID) and `WASM module not initialized` from
  `disconnect()`'s `vfsClear` (automergeSync.ts:169) firing on the
  share handler's URL-clearing route change before `connect()`'s
  `await initWasm()` — present without the ephemeral flag too.
- [ ] Permanent Playwright spec spawning the q2 binary: deferred — the
  e2e suite's globalSetup boots a hub on :3031 for all specs and
  nothing in the suite builds/runs the q2 binary (the embed is baked at
  cargo-build time), so a preview-boot spec needs its own harness.
  Filed as follow-up on bd-zf4ryvuq.

### Phase 5 — Full verification

- [x] `cargo xtask verify` (full — hub-client changed): all 14 steps
  passed. This covers `cargo build --workspace`, `cargo nextest run
  --workspace`, the ts-packages builds, `cd hub-client && npm run
  build:all` (the CLAUDE.md-required strict hub-client build), and
  hub-client `test:ci`. The embedded editor bundle was rebuilt
  separately via `cargo xtask build-hub-client-embed` (Phase 4) and
  `cargo build --bin q2` re-embedded it.

### Phase 6 — Bookkeeping

- [x] Close bd-zf4ryvuq.

(The hub-client changelog entry is waived for this change — user
decision, 2026-08-07. The CLAUDE.md two-commit changelog workflow does
not apply here.)

## Details

### Design decisions

1. **Signal rides the boot URL, not `/api/preview/config`.** The
   endpoint would need a new boot-time fetch in hub-client (404 in
   production) and races with the render gates; the hash param travels
   with the one URL only the preview server generates, alongside the
   existing `server=%2Fws` signal, and is captured before the share
   handler's SECURITY URL-clearing.
2. **Silent auto-setup, not a bare gate skip.** A bare skip leaves
   `status` stuck at `needs-setup`: ProjectsHome would flash during the
   share connect and degrade if the user navigates home. Silently
   establishing the root (the join-collection pattern) makes the whole
   app coherent — add-to-set, reconciliation, and ProjectsHome all work
   — for the cost of one empty root doc per fresh origin. Default
   preview ports are ephemeral, so each boot is a fresh origin with no
   accumulation.
3. **`ephemeral?: boolean` via conditional spread** (omitted when
   absent), matching `FileRoute.anchor`; existing parse-test objects
   stay valid, `buildShareableUrl` needs no change.
4. **Skip the `error` gate too** in ephemeral mode: a project-set
   failure must not block the preview — the share connect proceeds
   without the set.

### Explicitly out of scope

- `--no-project` editor boots (no share URL; lands on the project
  selector, still gated). Would need a different signal channel.
- IDB accumulation of dead per-session project entries on pinned-port
  preview workflows (pre-existing; the silent root set actually absorbs
  them via migration).
- `routesEqual` ignores `ephemeral` (not a location discriminator).
