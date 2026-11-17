---
title: "q2 preview against an existing sync server (--hub-url)"
strand: bd-yq3awhmw
date: 2026-11-13
description: Assessment and spike plan for serving the preview viewer or hub-client editor against a remote q2 hub instead of the preview's own ephemeral hub.
---

**Strand:** bd-yq3awhmw. **Status:** spike implemented and working in all three UI modes (see Spike results). Not yet a PR.

## Goal

Let `q2 preview` serve its embedded UI against an existing automerge sync
server (a `q2 hub`, later quarto-hub.com) instead of its own ephemeral hub. It
should work in every UI mode:

- `--ui viewer` (default): follow a remote project's preview, read-only.
- `--ui viewer --allow-edit`: block edits in the WYSIWYG viewer go into the
  remote project's automerge documents.
- `--ui editor`: the full hub-client, working against the remote hub.

Sketch of the command line:

```
q2 preview [--ui viewer|editor] [--allow-edit] --hub-url ws://host/ws --project <index-doc-id | share URL>
```

## What exists today

- `q2 preview --ui editor` serves the embedded hub-client, but its boot URL is
  hard-wired to `server=%2Fws&ephemeral=true`, the preview's own hub
  (`editor_share_route` in `crates/quarto/src/commands/preview.rs`).
- `q2 preview --join` only reaches a `q2 preview --share` host, over an iroh
  tunnel. It is not a general sync-server client.
- `q2 hub` is a sync server only; the SPA is deployed separately.
- `q2 provide-hub` joins an existing hub as a code-execution provider, with no
  web UI. It does solve authentication against a hub (Node auth bridge plus
  `TokenSource`).
- hub-client already accepts any sync server through
  `#/share/<id>?server=...`.

## How the viewer SPA connects

The viewer (`q2-preview-spa`) does not take a server parameter. It reads the
index document id from its own server's `/health` and syncs over same-origin
`/ws` (`PreviewApp.tsx`, `serverUrl.ts`). Rendering is WASM in the browser over
the synced automerge documents, so following a remote project needs only the
documents.

With `--allow-edit`, the viewer writes block edits into automerge through
`applyEditorOperations`. Against a remote hub the edits reach that hub, and its
disk if it runs in project mode. The local write-back-to-disk policy is moot.

Several `/api/preview/*` routes read the project from local disk through
`ctx.storage().project_root()`:

| Route | Without a local project |
| --- | --- |
| `/api/preview/deps` | Returns an empty set. Dependency-filtered re-renders become "re-render on every change". Acceptable. |
| `/api/preview/re-execute`, captures | Need a project root. Disable or hide. |
| `/api/preview/render`, `/api/preview/pdf` | Native renders from disk. Disable or hide. |
| `/api/preview/diagnostics` | Server-side diagnostics come from the routes above, so it stays empty. |
| `/.quarto/project-artifacts/...` | Embedded decks served from disk. Not available. |

## Chosen design: a local hub that peers with the remote

The preview runs its own hub in standalone mode (no project folder) and peers
it with the remote hub. Both UIs keep talking to same-origin `/ws` and
`/health`, so neither needs code changes, and the browser never runs into
cross-origin or mixed http/https problems. quarto-hub already supports peering
(`peers` in `HubConfig`, `spawn_peer_connection` in
`crates/quarto-hub/src/peer.rs`).

The alternative is to reverse-proxy `/ws` to the remote, as the `--join` tunnel
proxy does. It is simpler, but it loses the local samod cache, and `/health`
would still need to report the remote index id.

## Does peering pull the project documents? Not as configured today

Sources: samod-core 0.16 (`share_config.rs`,
`actors/document/phase/request.rs`, `actors/document/phase/ready.rs`,
`actors/document/peer_doc_connection.rs`), and quarto-hub `context.rs` and
`index.rs`.

### 1. Blocker: `announce` also controls requesting

In samod, `ShareConfig.announce` controls both proactive announcement and
requesting missing documents:

```rust
pub(crate) fn can_request(self) -> bool {
    self.can_write() && self.announce
}
```

quarto-hub gives every peer `ShareConfig::read_write().with_announce(false)`
(`hub_share_policy` in `context.rs`, the security fix from bd-jvag5d4w). A
local preview hub that lacks a document therefore never asks the upstream for
it. The request phase marks that peer `BlockedByPolicy`, so this fails silently
rather than with an error.

The same setting blocks the other direction. With `announce` off, the local hub
never pushes documents created in the browser (new files) upstream, and the
remote hub, also announce-off, never requests them.

**Fix.** samod's `SharePolicy` is decided per document and peer
(`share(doc_id, peer_id)`). In remote mode, give the upstream connection
`read_write()` with announce on, and keep announce off for local browser
clients. Announcing to a loopback, single-user browser client is also harmless,
if that turns out simpler to wire.

Open detail: the policy receives a `PeerId`, not a connection, so it needs to
know which peer id is the upstream one. `dial_websocket` returns a handle whose
`established()` yields the remote `peer_id`. Check whether policy decisions for
that connection can happen before the id is recorded. The policy is a stream,
so it can emit an updated decision, and `Request::share_policy_changed` handles
a late grant.

### 2. The remote side needs no change

- Announce-off still serves a request from an allowed peer (`read_write`).
- In the Ready phase, `PeerDocConnection::can_send` is
  `has_requested || their_heads known || announce`. After the local hub asks
  once, the remote keeps sending collaborators' changes down.
- An inbound sync from a writable peer for a document the remote does not have
  is accepted. Local edits and new documents land upstream once the local side
  announces them.

### 3. Startup race: an immediate "unavailable" that recovers

While the local hub is still fetching a document from upstream, a browser
request for it puts that browser connection in the `RequestedFromUs` state, and
`Request::generate_messages` answers `DocUnavailable` immediately. It does not
wait for the upstream answer. Once the document arrives and the actor moves to
Ready, the browser gets a sync anyway, because `has_requested` is set on its
connection.

`quarto-sync-client` already retries cold-start "unavailable"
(`FindDocRetryOptions` and the unavailable-files poll), so the worst case is a
retry delay. To avoid the race entirely, the preview server should fetch the
index document and its file documents before `/health` reports ready. The SPA
boot is already arbitrated by `/health`.

### 4. Index wiring: today it would create an empty project

`load_or_create_index(repo, storage.index_document_id())` (`index.rs`) creates
a new index when the id is not found locally, with only a warning. Peers are
dialed after the index load (`context.rs`, around line 400). Pointed at a
remote project id today, the hub would make a new, empty project.

Remote mode must:

1. Dial the upstream first and await `established()`.
2. Find the remote index id with the request-capable policy.
3. Fail with a clear error when it is unavailable, never create a new index.
4. Fetch the file documents listed in the index, and keep watching the index
   for files added later.

### 5. Authentication

`spawn_peer_connection` calls `repo.dial_websocket` with no credentials. Any hub
with OIDC sign-in, quarto-hub.com included, will refuse the connection. The
spike targets an unauthenticated `q2 hub`. The full feature should reuse the
`provide-hub` token path.

### 6. Editor boot URL

The editor boot URL carries `ephemeral=true`, which makes hub-client set up a
throwaway project-set root and skip the setup gate. In remote mode the local
hub acts as a cache of a real project. Keeping `ephemeral=true` is probably fine
for the spike; decide on it before shipping.

## Spike plan

- \[x\] `HubConfig` gains an upstream URL and a per-peer share policy (announce
  on for the upstream peer only).
- \[x\] Reorder hub startup in remote mode: dial the upstream, await the
  connection, load the index by the remote id with no create fallback, then
  fetch the file documents.
- \[x\] `q2 preview --hub-url ws://... --project <id>` implies standalone mode.
  It conflicts with a positional path, `--no-project`, `--join` and `--share`.
- \[ \] Hide or disable the local-disk routes (re-execute, PDF, render download)
  in remote mode.
- \[x\] Manual test: `q2 hub` on a project directory, plus
  `q2 preview --hub-url ws://127.0.0.1:3000/ws --project <id>` with the default
  viewer, `--allow-edit` and `--ui editor`. Check that edits round-trip to the
  hub's files on disk and that a file created in the editor reaches the hub.
- \[x\] Integration test: two in-process hubs (one standalone with an upstream),
  asserting that a document created on the upstream is readable through the
  local hub, and that a document created locally reaches the upstream.

## Spike results (2026-11-13)

### What was built

- `crates/quarto-hub/src/upstream.rs`: `UpstreamConfig`, the `HubSharePolicy`
  that every hub now uses, `connect_upstream`, `load_upstream_index` and
  `prefetch_files`.
- `HubConfig::upstream` (`context.rs`). With it set, the hub refuses project
  mode, dials the upstream and records its peer id, loads the upstream index
  (an error if it is missing, never a new index), and prefetches the file
  documents in the background.
- `HubSharePolicy`: announce off for every peer, as before, except recorded
  upstream peers (read-write, announce on). The dialer `events()` stream adds
  the peer id of every reconnect, since an upstream restart may present a new
  one. A hub with no upstream records none, so `q2 hub` and the plain preview
  are unchanged.
- `q2 preview --hub-url <ws> --project <id or share URL>`: implies standalone
  mode. It conflicts with a path, `--no-project`, `--join`, `--static`,
  `--share`, `--to`, `-M` and `--profile`. `PreviewConfig::upstream` carries it
  through to the hub.

### What `--allow-edit` means for a mirror

A mirror has no local disk to protect, so the disk write policy says nothing
there. Without `--allow-edit`, the mirror gives its browser clients
`ShareConfig::read_only()`: they receive every change, and the mirror ignores
theirs, so nothing typed in the browser reaches the upstream. With
`--allow-edit`, clients are read-write and edits go to the remote project. This
keeps the flag's meaning ("edits reach the project's files") and keeps the
editor UI's "edits won't be saved" banner (driven by `allowEdit` from
`/api/preview/config`) truthful.

### Verification

- `crates/quarto-hub/tests/integration/upstream_mirror.rs`, five tests: the
  mirror fetches the index and files; an unknown project is an error; edits and
  a new file flow from the mirror to the upstream; a client's edit is forwarded
  with `WriteBack` and dropped with `ReadOnly`.
- The finding in the "Blocker" section above was confirmed by experiment.
  Giving the upstream peer the old announce-off policy makes the fetch test
  fail: the mirror cannot even load the index.
- Manual, against `q2 hub --project <dir>` with the main checkout's SPA builds
  passed through `--preview-dir`:
  - The default viewer rendered the remote project. An edit to the upstream's
    file on disk appeared in the browser.
  - With `--allow-edit`, a block edit in the viewer was written to the
    upstream's `index.qmd`.
  - In `--ui editor`, the file tree and preview loaded. A source edit and a file
    created with "New file" both reached the upstream's disk.

### Open items found during the spike

- The editor banner still reads "Ephemeral session — edits won't be saved to
  disk" when `--allow-edit` is off. With read-only clients that is accurate,
  but its tooltip says edits "sync live to everyone connected", which a
  read-only mirror does not do. Mirror-specific wording is needed.
- The editor boot URL still carries `ephemeral=true` and names the project "q2
  preview". It works, but the project-set entry hub-client creates is
  throwaway.
- The local-disk routes (re-execute, PDF, render download, project artifacts)
  are not hidden in mirror mode yet.
- `/health` on the mirror reports `qmd_file_count: 0`. Nothing in the UI
  depended on it during the manual test.
- Peer ids are self-asserted. A client of the mirror that claims the
  upstream's peer id would be granted announce and read-write. This only
  matters when the preview listens beyond loopback.
- `hub-client/dist-preview-embed` in the main checkout was empty, so the manual
  editor test used `hub-client/dist`. That is unrelated to this feature.

## Out of scope for the spike

- Authentication against OIDC hubs (quarto-hub.com).
- Combining a local project path with `--hub-url`, that is, running the engine
  locally against remote documents.
