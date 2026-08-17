# Editor boot URL for `q2 preview --join` guests (skip project-set setup)

Strand: bd-7htq16rx

## Overview

`q2 preview --join <ticket>` against a host running `q2 preview --ui
editor --share` opens the browser at `http://<guest-proxy>/` — the
hub-client **root route**. Two failures follow:

1. A fresh browser profile (the common case: the guest's proxy port is
   ephemeral, so the origin is new) hits the `ProjectSetSetup` gate in
   `App.tsx` (`needs-setup` / `needs-migration`).
2. Even past the gate, the app lands on `ProjectsHome`: the root route
   carries no document coordinates, so the share handler
   (`App.tsx:443`) never runs and the guest **never joins the
   document**.

The host's own boot URL is the share route with `ephemeral=true`
(`build_editor_boot_url`, bd-zf4ryvuq — see
[2026-08-07-preview-editor-skip-project-setup.md](2026-08-07-preview-editor-skip-project-setup.md)).
The guest needs the same URL shape, built against its local proxy
origin.

**Approach:**

1. **Host side** (`crates/quarto-preview`): the CLI's editor-mode
   `on_ready` already computes the boot params (index doc id, file,
   project name) for its own boot URL. It now also stashes them into a
   `quarto_preview` OnceLock via `set_editor_boot(...)`, and
   `GET /api/preview/config` gains an `editorBoot` field when stashed:
   `{ "allowEdit": …, "editorBoot": { "indexDocId", "file", "name" } }`.
   The stash happens in `on_ready`, which fires before the listener
   binds, so no guest can fetch config before it is set.
2. **Guest side** (`crates/quarto/src/commands/preview.rs`,
   `run_join`): after the tunnel binds, wait for `/health` through the
   tunnel (the existing probe, now unconditional), then fetch
   `/api/preview/config` once. If it carries `editorBoot`, build
   `http://<guest>/#/share/<docId>?server=%2Fws&file=…&name=…&ephemeral=true`
   (same route helper as the host builder) and print/open that;
   otherwise fall back to `http://<guest>/` (viewer hosts, older hosts,
   `--no-project` editor boots — all unchanged from today).

The `ephemeral=true` flag makes the guest's hub-client capture the
flag at mount, silently establish the project-set root against `/ws`
(tunneled to the host), and skip the setup/migration/error gates — the
bd-zf4ryvuq machinery, now reached by guests too. No hub-client change
is needed; that support already shipped.

**Why the config endpoint, not the ticket:** the ticket is minted
before the hub boots (so the banner can print ahead of the first
accept — `share.rs`), hence cannot carry the index doc id. The guest
already does HTTP probes through the tunnel for readiness, and
`/api/preview/config` is the established boot-time session-info
channel (bd-ov4gqk3m). Every `--share`-capable host already serves it,
and the doc id is already exposed to guests via `/health` — no new
trust surface. The viewer SPA reads only `allowEdit`; unknown fields
are ignored.

## Work Items

### Phase 1 — Tests first (TDD)

- [x] `crates/quarto-preview/tests/integration/config_endpoint.rs`:
  new test — server booted with `ui: PreviewUi::Editor`, on_ready
  calls `quarto_preview::set_editor_boot(...)` (mirroring the CLI),
  then `GET /api/preview/config` carries
  `editorBoot { indexDocId, file, name }`. Also assert the field is
  absent in the existing no-stash tests. Confirm fail (compile error —
  the type/setter don't exist yet). Confirmed:
  `error[E0425]: cannot find function set_editor_boot in crate
  quarto_preview` + `E0422 EditorBootInfo`.
- [x] `crates/quarto/src/commands/preview.rs` tests:
  - Guest URL builder: share-route shape, `ephemeral=true` last param,
    `automerge:` prefix stripped, percent-encoding of file/name.
  - `parse_editor_boot`: config body with `editorBoot` → `Some`;
    without → `None`; malformed JSON → `None`; empty `indexDocId` /
    `file` → `None`.
  - Confirm fail. Confirmed: `E0425 build_guest_editor_url /
    parse_editor_boot not found in this scope`.

### Phase 2 — `quarto-preview` implementation

- [x] `crates/quarto-preview/src/lib.rs`: public `EditorBootInfo`
  (Serialize + Deserialize, camelCase wire names), crate OnceLock +
  public `set_editor_boot` (first-writer-wins, same pattern as
  `ALLOW_EDIT` / `PREVIEW_UI`), and `preview_config_handler` includes
  `editorBoot` when stashed.
- [x] `cargo nextest run -p quarto-preview config_endpoint` — 3 passed
  (including the new `config_reports_editor_boot_when_stashed`).

### Phase 3 — CLI implementation (`crates/quarto/src/commands/preview.rs`)

- [x] Extract `editor_share_route(index_doc_id, file, name)` (the hash
  route, incl. `ephemeral=true`); `build_editor_boot_url` delegates so
  host and guest share one route-shape source.
- [x] `build_guest_editor_url(SocketAddr, &EditorBootInfo)`.
- [x] `fetch_editor_boot(addr)` — hand-rolled one-shot
  `GET /api/preview/config` (same no-new-deps style as
  `health_get_ok`) + pure `parse_editor_boot(&[u8])`.
- [x] `run_join` restructure: probe `/health` unconditionally (15 s
  budget, warn-on-timeout as today), then the config fetch; print the
  final URL once; open the browser inline (the spawned probe task
  goes away — the probe has already happened by print time).
- [x] Host editor-mode `on_ready`: stash `EditorBootInfo` when a share
  file was picked (the `None` arm — `--no-project` — stashes nothing).
- [x] `cargo nextest run -p quarto preview` — 49 passed, including the
  5 new tests.

### Phase 4 — Verification

- [x] `cargo nextest run --workspace` (monorepo rule) — **10939
  passed, 0 failed**, 197 skipped.
- [x] `cargo xtask verify --skip-hub-build` (Rust-only change;
  hub-client's ephemeral handling shipped with bd-zf4ryvuq) — all 14
  steps passed.
- [x] End-to-end through the real binaries — evidence below.

#### End-to-end evidence

Host (background, direct PID):

```
$ target/debug/q2 preview examples/websites/01-minimal --ui editor --share --no-browser
  q2 preview — editor UI
  session edits are ephemeral — pass --allow-edit to persist edits to disk

Sharing this preview session (end-to-end encrypted via iroh).
…
q2 preview --join q2previewad762hujnc7b5xmlmlee6pysgdex65yzkkfq763sjch5wmfvgokaa…

  → http://127.0.0.1:51216/#/share/3gNGe3PqRHQ4EjyQFmNS2P94PMxC?server=%2Fws&file=index.qmd&name=01-minimal&ephemeral=true
```

Guest (same machine, `--no-browser`):

```
$ target/debug/q2 preview --join q2previewad762… --no-browser

  q2 preview — joining a shared session (end-to-end encrypted via iroh)
  → http://127.0.0.1:51233/#/share/3gNGe3PqRHQ4EjyQFmNS2P94PMxC?server=%2Fws&file=index.qmd&name=01-minimal&ephemeral=true

  Press Ctrl-C to leave the session.

  ● connected via direct connection
```

The guest prints the **share route** on its own proxy port with the
**same doc id** as the host's boot URL and `ephemeral=true` — before
this change it printed the bare root URL `http://127.0.0.1:51233/`.

Browser e2e (Playwright, headless Chromium, fresh profile — no
IDB/localStorage, i.e. the `needs-setup` path), driven at the guest's
printed URL:

```
PASS: .editor-container became visible
PASS: ProjectSetSetup never rendered
final url: http://127.0.0.1:51233/#/p/<uuid>/file/index.qmd
```

The final `#/p/<uuid>/file/index.qmd` URL proves the share handler
connected and loaded the document through the tunnel (it only
navigates after `connectAndLoadContents` succeeds).

Control run against the same live guest proxy at the bare root URL
(the "before" behavior): `ProjectSetSetup` rendered, editor never
appeared (script exit 1) — confirming the fix, not the client build,
is what skips the gate.

### Phase 5 — Bookkeeping

- [x] Close bd-7htq16rx; keep this plan current.

(No hub-client changelog entry: hub-client is untouched by this
change.)

## Details

### Design decisions

1. **Boot info rides `/api/preview/config`, not a new endpoint.** The
   endpoint already exists for boot-time session info, is served by
   every preview server (viewer and editor), and is reachable through
   the tunnel. Backward compatible in both directions: old hosts omit
   the field (guest falls back to `/`, today's behavior); old guests
   never fetch it.
2. **The CLI stashes, the server serves.** `pick_editor_file` and the
   project-name derivation live in the CLI (they need `initial_page`,
   which `PreviewConfig` deliberately doesn't carry); the server crate
   just exposes the stash. `on_ready` fires before the listener binds
   (`server.rs` contract), so the OnceLock is always set before any
   guest can fetch.
3. **Guest prints one URL, after the probe.** Today the guest prints
   `/` immediately and a spawned task gates the browser-open on
   health. Printing before the probe would print the wrong (root) URL
   for editor hosts; so the probe becomes sequential, the final URL
   prints once, and the browser opens inline. Common case adds one
   tunnel roundtrip (~ms); worst case (host still booting) waits out
   the same 15 s budget the spawned probe had.
4. **`EditorBootInfo` is defined in `quarto-preview`** with both
   `Serialize` and `Deserialize`; the guest (`quarto` crate) reuses
   the same type — one wire-shape definition, no drift.
5. **`--no-project` editor hosts stash nothing** (there is no document
   to join); guests land on `/` exactly as the host does.

### Explicitly out of scope

- `--no-project --share` editor hosts (degenerate: sharing an empty
  editor; guest sees the same project selector the host sees).
- Changing the ticket wire format to carry boot params (impossible
  without delaying the banner: the doc id doesn't exist at mint time).
- IDB accumulation on the guest for pinned-port join workflows (same
  note as bd-zf4ryvuq; default guest ports are ephemeral).
