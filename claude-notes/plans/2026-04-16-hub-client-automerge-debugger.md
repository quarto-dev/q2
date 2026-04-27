# Port automerge-inspector into hub-client as a debugging view

## Overview

Port the standalone `external-sources/automerge-inspector` application into
`hub-client/` as a second Vite entry point, so that Quarto Hub users/developers
can inspect Automerge document state and sync protocol traffic against the
**auth-enabled** sync server. The standalone inspector assumes a public sync
server (`wss://sync.automerge.org`) and is unusable against `quarto-hub.com`,
which requires an authenticated WebSocket upgrade.

The debug view must:

1. Be accessible to any signed-in user, but "out of general sight" — not linked
   from the main UI and served at a distinct URL.
2. Live on a **separate HTML document** (new Vite entry point).
3. Share hub-client's login credentials — the HttpOnly auth cookie flows
   automatically on same-origin WebSocket upgrades, so the debug page just
   needs to verify the user is logged in and use the same `/ws` path.
4. Be purely read-only with respect to document state — no writes, no
   mutations. The debug page observes; it does not edit.

## Context and constraints

### Entry-point model

- hub-client is a Vite SPA with a single `index.html` → `src/main.tsx` entry.
- Vite supports multiple entry points via `build.rollupOptions.input`. Adding
  `debug.html` → `src/debug/main.tsx` is straightforward.
- The dev server already proxies `/auth/*` and `/ws` to the hub server
  ([hub-client/vite.config.ts:50-64](../../hub-client/vite.config.ts)), so both
  entries automatically share auth routing and the authenticated sync socket.

### Auth sharing

- Auth cookies are HttpOnly + same-origin — they flow to both entries
  automatically. No JS-level coordination is required for cookie-based auth.
- Both entries can call `fetchAuthMe()` from `services/authService.ts` to
  check login status and obtain `{ email, name, picture }`.
- For a given project's per-project actor ID, both can call
  `fetchActorId(projectId)` independently.

### Repo model: separate, ephemeral, independent

Because a different HTML file loads in a separate browsing context with its
own JS heap, the debug view cannot literally share the main app's `Repo`
instance. It will create its own `Repo` and connect independently. This is
the same pattern the standalone inspector uses, and matches how
`quarto-sync-client` builds its Repo internally
([ts-packages/quarto-sync-client/src/client.ts:336-338](../../ts-packages/quarto-sync-client/src/client.ts)).

Design decision: **default to an ephemeral Repo** (`isEphemeral: true`, no
IndexedDB storage). Rationale:

- Debugging should be able to see the latest authoritative state from the
  sync server, not stale local storage.
- Avoids polluting the user's real IndexedDB with duplicate debug state.
- Matches standalone inspector semantics.

**Storage-source toggle** (Phase 6): since some hub-client bugs may live in
the IndexedDB persistence layer, the debugger will additionally offer a
*local-only* mode that mounts a Repo against the same IndexedDB database
hub-client uses, with no network adapter attached. This answers "is this
bug in the synced state, or in the local cache?" — see Phase 6 for
details, including the read-only-storage wrapper needed to avoid
perturbing the system under investigation. Deferred to after the main
port so the server-side debugger can be validated first.

### Network adapter: reuse `LoggingNetworkAdapter` verbatim

`external-sources/automerge-inspector/app/src/services/LoggingNetworkAdapter.ts`
is a clean wrapper around any `NetworkAdapter`. It can be ported as-is. The
wrapped adapter becomes `BrowserWebSocketClientAdapter` (same class
`quarto-sync-client` uses) pointed at the hub's authenticated `/ws` endpoint.

### Sync server URL

- Default: same origin, `/ws` path — inherits auth cookie automatically.
- Allow user to override for advanced debugging (e.g. connect to a different
  hub), mirroring the standalone inspector's URL input. When pointed at a
  non-same-origin host, auth cookies won't flow and the user will get
  whatever the remote server permits.

### Document discovery

Beyond the standalone inspector's freeform "paste an `automerge:...` URL"
box, hub-client knows (via IndexedDB) which projects the user has locally.
The debug page should expose these as a quick-select source. Two layers:

1. **Project quick-pick**: read `quarto-hub` IndexedDB stores
   (`projects`, `projectSet`) to enumerate known projects and pre-fill the
   document ID field when the user picks one.
2. **Index → files drill-down**: once an index doc is loaded and readable,
   list file documents referenced by it and allow subscribing to each.

## Design decisions (confirmed with user 2026-04-16)

1. **URL scheme**: `/debug.html` (empty state) and `/debug.html#doc=<url>`
   (preload a single initial document). The hash is only a convenience for
   seeding the *first* document; the running UI must offer the same
   affordances as the standalone automerge-inspector — add/remove
   documents at runtime via the `DocumentList` input, inspect state, watch
   protocol traffic, etc.
2. **Unauthenticated behavior**: gate screen that says "sign in to Quarto
   Hub first" with a link to the main app. No auto-redirect.
3. **Mutations**: read-only. No writes, no forced syncs, no deletes.
4. **Styling**: attempt to visually match hub-client (fonts, color tokens,
   spacing). **Do not refactor hub-client's styles** to support this —
   consume existing CSS variables if they're already exported globally,
   otherwise mimic values locally. Zero changes to the main app's CSS.
5. **Discoverability**: no links from the main app. No About-box mention,
   no settings-menu entry. Intended audience is developers and bug
   reporters who will be told the URL out-of-band. Document the feature
   only in `hub-client/changelog.md` per the normal commit workflow.

## Testing strategy (TDD-friendly pieces)

This is primarily a UI port, so TDD applies to the mechanical pieces:

- **`LoggingNetworkAdapter` behavior**: unit tests that wrap a stub
  `NetworkAdapter`, send/receive messages, and assert the log callback fires
  with correct `direction`, `type`, `documentId`, `dataSize`. Port the
  pattern from the standalone inspector if it has tests; otherwise write
  new vitest specs.
- **Auth gate**: hook test that mocks `fetchAuthMe()` returning 401 /
  200 / network-error, asserts the correct UI state (gate screen vs.
  inspector UI).
- **Project quick-pick**: test that reads a seeded `quarto-hub` IndexedDB
  (via fake-indexeddb) and lists the stored projects.
- **Build smoke test**: `npm run build:all` must succeed with the new
  entry point. `tsc -b` in project-references mode will catch type errors
  the dev server misses. CI is what will actually validate this.
- **Manual verification**: per CLAUDE.md, UI features must be exercised in
  a real browser. Checklist in the last phase.

## Phased work items

### Phase 1 — Scaffolding (no behavior yet)

- [x] Add `debug.html` at `hub-client/debug.html` with `<div id="root">` and
      `<script type="module" src="/src/debug/main.tsx">`.
- [x] Create `hub-client/src/debug/` directory for the new entry.
- [x] Add `hub-client/src/debug/main.tsx` that mounts a minimal `<DebugApp />`
      placeholder.
- [x] Update `hub-client/vite.config.ts` to declare multiple inputs via
      `build.rollupOptions.input` (keep `index.html` as the default).
- [x] Verify `npm run dev` serves `/debug.html` and `npm run build`
      produces both bundles. (Dev server HTTP 200, prod build emits
      `dist/debug.html` + `dist/assets/debug-*.js` alongside main bundle.)

### Phase 2 — Auth gate

- [x] Write a vitest for a new `useDebugAuthGate` hook that calls
      `fetchAuthMe()` and returns `{ state: 'checking' | 'authed' | 'anon'
      | 'error', user? }`. Mock responses for each state. (5 cases:
      initial checking, authed, anon/401, error/throw, single-fetch.)
- [x] Implement `useDebugAuthGate` in `src/debug/hooks/useDebugAuthGate.ts`
      reusing `fetchAuthMe` from `services/authService.ts`.
- [x] Render a minimal "Please sign in to Quarto Hub to use the debugger"
      screen with a link to `index.html` when the state is `anon`. Also
      handles `checking` (spinner-ish message) and `error` (retry link)
      states.

### Phase 3 — Port inspector core

- [x] Copy `LoggingNetworkAdapter.ts` from the external app into
      `src/debug/services/`. Adapt imports if needed. Add vitest specs.
      (6 specs covering outgoing-send logging, incoming-message
      interception, peer tracking via `peer-candidate`/`peer-disconnected`,
      mismatched-peer filtering, StrictMode disconnect guard, and
      post-connect disconnect forwarding.)
- [x] Copy `types/messages.ts`, `hooks/useMessageLog.ts`, and
      `services/repo.ts` into `src/debug/`. Added
      `@automerge/automerge-repo-react-hooks@2.5.1` to hub-client
      (exact pin — this package hard-pins its own automerge-repo, so a
      caret range would nest and break type identity for `Repo`).
- [x] Default the sync URL builder to `{wss|ws}://{location.host}/ws` via
      `defaultSyncServerUrl()` in `services/repo.ts`, so the HttpOnly
      auth cookie flows on the WebSocket upgrade. Leaves the UI override
      field intact.
- [x] Port `ConnectionStatus`, `DocumentList`, `DocumentViewer`,
      `MessageLog` React components from the standalone app into
      `src/debug/components/`.
- [x] Port the standalone `App.tsx` structure into `src/debug/DebugApp.tsx`,
      composed with the auth gate from Phase 2 (Inspector renders only in
      the `authed` state).
- [x] Port styles into `src/debug/debug.css`. Used a local light-mode
      palette with Posit-adjacent accents (orange = `#d44000`, slate
      neutrals) — variables hardcoded inside the debug CSS so the page
      stays self-contained. Root selector renamed from `.app` to
      `.debug-app` to prevent any theoretical collision with main-app
      classes. **No changes to any CSS outside `src/debug/`.**

### Phase 4 — Hub-specific enhancements

- [x] Add a "Projects on this device" quick-pick panel that reads local
      IndexedDB (`quarto-hub.projects` and `projectSet`) and lists project
      description + index doc ID. Clicking an item subscribes directly
      (no copy-paste into input). Implementation: `localProjects.ts`
      opens `quarto-hub` with no version (so no upgrades/migrations),
      guards reads with `objectStoreNames.contains`, and never writes.
      `useLocalProjects` hook loads once on mount; `QuickPick` renders
      the groups. vitest with fake-indexeddb: 5 tests in
      `localProjects.test.ts` (empty DB, seeded projects, missing
      pointer, seeded pointer, no-store-creation contract) + 2 tests
      in `useLocalProjects.test.ts`.
- [x] When an index doc loads, detect the Quarto index-document shape
      (`{ files: Record<string, string> }` from
      `@quarto/quarto-automerge-schema`) and surface file-doc IDs as
      one-click subscribe buttons under the index doc's panel. Shows
      file path + truncated docId + state-aware button ("open" →
      "subscribed" → "…" during load). Rendered inside `DocumentViewer`
      via `IndexFilesSubscribe` subcomponent.
- [x] Support `#doc=<automerge-url>` hash on `debug.html` to preload a
      single initial document. The hash is seed-only — once the UI is
      running, the user adds/removes additional docs via the DocumentList
      input, identical to standalone inspector affordances. Parser is
      `parseDebugHashSeed()` with 7 unit tests; seed application is
      gated by a `hashSeedApplied` ref so it runs once per mount and is
      ignored on reconnect.

### Phase 5 — Local IndexedDB storage mode

Lets the user inspect what hub-client has actually persisted to disk,
independent of the sync server. Same-origin IndexedDB is shared between
`index.html` and `debug.html`, so this is feasible with no coordination
between the two pages.

- [x] Read the actual IndexedDB database name / store layout: confirmed
      both `quarto-sync-client/src/client.ts:339,725` and
      `hub-client/src/services/projectSetService.ts:127,176` construct
      `IndexedDBStorageAdapter()` with defaults — so all Automerge doc
      chunks live in DB `automerge`, store `documents`, keyed by
      `[docId, kind, ...]` arrays per
      `node_modules/@automerge/automerge-repo-storage-indexeddb/dist/index.js`.
      Documented inline in `services/localStoredDocs.ts` and
      `services/repo.ts`.
- [x] Implement `ReadOnlyStorageAdapter` (wraps any
      `StorageAdapterInterface`, forwards `load`/`loadRange`, no-ops on
      `save`/`remove`/`removeRange`). 5 vitest specs verifying the
      pass-through and drop-write contract.
- [x] Add `@automerge/automerge-repo-storage-indexeddb@2.5.1` as a direct
      hub-client dep (exact pin to match automerge-repo@2.5.1; a caret
      range would have resolved to 2.5.4 and nested a second
      automerge-repo copy, breaking `Repo` type identity — same issue as
      react-hooks in Phase 3).
- [x] Add a storage-source toggle to the debug UI. Modes:
      - **Server (live)** — Phase 3 default (ephemeral + network +
        MessageLog).
      - **Local IndexedDB** — Repo with ReadOnlyStorageAdapter(
        IndexedDBStorageAdapter()), no network. Header swaps
        ConnectionStatus for a "Local IndexedDB (read-only, never
        writes)" banner; MessageLog panel is hidden entirely (no
        network = no traffic).
- [x] Document enumeration: `listLocalStoredDocumentIds()` opens the
      `automerge` DB read-only, iterates key cursor, extracts unique
      first-elements. Rendered in the `StoredLocalDocs` sidebar panel
      with per-doc one-click subscribe + a Reload-list button. 4
      vitest specs covering empty DB, multi-doc enumeration, missing
      store, no-side-effects contract.
- [ ] Manual QA: reproduce a simple "local-only divergence" scenario (e.g.
      edit offline, then open debug tab in local mode and confirm the
      local state is visible; open in server mode and confirm the server
      state differs if nothing has synced yet). Document the repro steps
      in the changelog entry so support can use it. — **Deferred to
      Phase 6**, where all manual browser QA is gathered.

### Phase 6 — Polish & verification

- [x] Ensure no main-app code imports from `src/debug/` and vice versa
      beyond the pure services/utilities shared intentionally. Verified:
      zero imports of `src/debug/*` from outside `src/debug/`; imports
      from `src/debug/` into the main tree are limited to
      `services/storage/types` (constants + types), `types/project`
      (types), and `services/authService` (the `fetchAuthMe` HTTP
      helper). Pure shared points, no behavioural coupling.
- [x] Run `npm run build:all` and `npm run test:ci` in `hub-client/`.
      `test:ci` exits 0: 451 unit + 12 integration + 52 wasm = 515 tests
      pass across 34 files. `build:all` exits 0; debug bundle is 44 kB
      JS (16 kB gzipped) + 10 kB CSS (2.2 kB gzipped).
- [x] Skipped `cargo xtask verify` — no changes to `quarto-core` or
      `quarto-pandoc-types`. All changes are confined to `hub-client/`
      and `hub-client/package.json`.
- [x] Shell-level dev-server smoke: `/debug.html` serves HTTP 200 with
      the correct title and module script; `/auth/me` proxies (returns
      500 when no hub server is up, which is the expected path into the
      debug "error" state); `#doc=...` fragment URLs serve correctly.
- [ ] **Manual browser QA checklist** (requires user action — the
      Chrome MCP extension is not connected in this session, and the
      authed flows require real Google sign-in that cannot be driven
      autonomously):
      - [ ] Navigate to `/debug.html` while signed out → gate screen
            saying "Go to Quarto Hub and sign in".
      - [ ] Sign in via main app, reload `/debug.html` → inspector UI
            with "Connecting…" / "Connected" indicator.
      - [ ] Quick-pick a local project in the sidebar → subscribes,
            renders JSON, message log populates with sync traffic.
      - [ ] Paste an `automerge:...` URL for a non-local document →
            subscribes as expected; shows "unavailable" if the server
            doesn't have it.
      - [ ] Switch to "Local IndexedDB" mode → banner appears,
            MessageLog disappears, "Stored locally" panel lists doc IDs
            from disk; clicking one loads without any network traffic.
      - [ ] Open `/debug.html#doc=automerge:<any-known-id>` in a fresh
            tab → the doc is auto-subscribed on first load.
      - [ ] Disconnect / reconnect cleanly (no stuck "Connecting…").
      - [ ] No console errors; message-log filters (type / direction /
            document) work.
- [ ] **Update `hub-client/changelog.md`** — requires the first commit's
      hash. Per the project's two-commit workflow in CLAUDE.md, this is
      added in a second commit once the feature commit is merged.
      Suggested text: `Add Automerge debugger at /debug.html (separate
      entry, shares auth cookie, Server/Local IndexedDB modes, read-only).`

## Non-goals (explicitly out of scope for this plan)

- Write / mutation operations on documents.
- History browsing (change log per doc, heads navigation, time travel).
- Multi-user presence display (already handled in main app).
- Integration with hub server admin APIs.
- Sharing the main app's in-memory `Repo` (architecturally impossible
  across HTML entry points).
- **Instrumenting the main app with a `LoggingStorageAdapter`.** Could be
  useful for capturing storage traffic as it happens in the real app, but
  requires changing `quarto-sync-client` or the main app wiring and
  contradicts the "no refactor of hub-client" constraint. Revisit only
  if Phase 5's static snapshot view proves insufficient.
- **Raw IndexedDB byte dump.** Chrome DevTools' Application tab already
  handles the "is something even there?" case. We could decode Automerge
  binary blobs for a richer view, but that's marginal value on top of
  Phase 5's real-Repo-against-real-storage approach.
