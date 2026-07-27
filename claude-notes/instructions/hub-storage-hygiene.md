# Hub storage hygiene (`hub admin …`)

Maintainer tools for quarto-hub sync-server deployments
(bd-eiku4ymo; design + rationale in
`claude-notes/plans/2026-07-24-capture-meta-and-hub-admin-tools.md`).

## Why

Every engine re-execution writes a **new** capture binary doc and
repoints the index sidecar; the old doc is orphaned in samod storage
forever. Since #410 captures also embed figure bytes, so orphans are
big. These tools inventory a storage location, identify orphaned
captures, and reclaim them — through a pipeline designed so a mistake
must get past four independent gates before any byte is gone.

## The pipeline

```bash
# 1. SCAN — read-only. Safe on a live server (snapshot semantics; no
#    consistency is guaranteed anywhere in this system anyway).
hub admin scan --data-dir /srv/hub/data --output manifest.json
#    → prints an inventory by doc kind + the removable candidates,
#      each with evidence (createdAt, sourcePath, engines, WHY).
#    --json prints the manifest to stdout (versioned, camelCase —
#    stable for tooling). --older-than-days N gates candidates
#    (default 30; negative disables the gate). --include-unstamped
#    opts in captures recorded before the audit envelope existed.

# 2. COLLECT — quarantine, never delete. Dry-run by default. Refuses:
#    while a server holds hub.lock; if the manifest's dataDir isn't
#    this data dir; per-doc, if re-verification fails (doc became
#    referenced since the scan, kind changed, age gate no longer met).
hub admin collect --data-dir /srv/hub/data --manifest manifest.json            # plan
hub admin collect --data-dir /srv/hub/data --manifest manifest.json --execute  # quarantine
#    → moves each doc's chunks to data/trash/<batch>/docs/<doc-id>,
#      writes batch.json (embedded manifest + per-chunk SHA-256s).

# 3. RESTORE — undo a batch (or named docs), hash-verified.
hub admin restore --data-dir /srv/hub/data --batch /srv/hub/data/trash/<batch> [doc-id ...]

# 4. PURGE — the ONLY operation that deletes bytes. Whole batches,
#    past the retention window (default 30d), dry-run by default.
hub admin purge --data-dir /srv/hub/data                    # list
hub admin purge --data-dir /srv/hub/data --execute          # delete eligible
```

`quarto hub admin …` is the same tool (the `quarto` binary embeds hub).

## Safety properties to rely on

- **Allowlist**: only binary docs with the capture MIME
  (`application/x-engine-capture+gzip`) are ever collectible. Project
  indexes, project sets, file docs, and unknown-shaped docs (i.e.
  anything a newer schema added) are structurally protected —
  `scan` reports them, nothing can touch them.
- **Locking**: `collect`/`restore`/`purge` *acquire* the server's own
  `hub.lock` (exclusive flock) for their duration. A running server
  makes them refuse; symmetrically, a server cannot start
  mid-operation.
- **Quarantine lives inside the data dir** (`trash/`): renames on one
  filesystem, covered by your existing backup regime, can't be
  orphaned from its server.
- **Unstamped captures** (recorded before the meta envelope) have no
  age evidence and are excluded by default; batches without a
  readable `batch.json` similarly can never be purged.

## Caveats

- **Peers**: liveness is computed from THIS storage location only.
  Run against the authoritative store. A doc collected here can
  re-sync from a peer that still holds it — which is the safe
  direction (if it's referenced, that's exactly recovery; if not,
  it becomes a future scan candidate again).
- **`q2 preview` data dirs are ephemeral** (temp dirs deleted on
  shutdown) — these tools are for persistent hub deployments
  (`hub --project` / standalone `--data-dir`), or for snapshots of a
  store copied out for maintenance.
- Unreferenced **non-capture** docs (e.g. indexes of abandoned
  projects) are reported but not collectible in v1; that's a
  follow-up strand once real inventories are seen.
