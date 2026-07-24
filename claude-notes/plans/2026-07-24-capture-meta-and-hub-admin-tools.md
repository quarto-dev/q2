# Capture-doc metadata envelope + sync-server maintainer tools

**Braid strand:** bd-eiku4ymo
**Status:** implemented on branch
`braid/bd-eiku4ymo-capture-docs-uncompressed-auditgc`; binary E2E
verified (record below)

## Overview

Two deliverables, one storyline:

1. **Part A — audit/GC metadata envelope** (the strand as filed): stamp
   engine-capture binary docs with a small **uncompressed** `meta` map
   so sync-server audits can read provenance without gunzipping
   payloads.
2. **Part B — minimal sync-server maintainer tools** (added on review,
   2026-07-24): `hub admin` subcommands that (a) inventory a samod
   storage location and identify orphaned capture docs, producing a
   manifest of safely-removable document ids, and (b) act on such a
   manifest — with a design that minimizes the blast radius of any
   accidental deletion.

### Why captures orphan (recap)

Every re-execution creates a **new** capture binary doc and repoints
the index sidecar's `CaptureRef` (`re_execute.rs::perform_re_execute`,
`capture_driver.rs`, hub-provider `execute.rs`); nothing deletes the
old doc. On hub deployments these accumulate forever, and bd-qbhp2cvv
(#410) made each one substantially bigger (embedded figure bytes).

## Architecture facts the design rests on (verified in-tree)

- **Binary-doc schema** (`quarto-hub/src/resource.rs:107`): automerge
  ROOT with `content: Bytes`, `mimeType: String`, `hash: String`.
  Capture docs are identified by
  `mimeType == "application/x-engine-capture+gzip"` — already
  readable without decompression.
- **Doc kinds distinguishable by ROOT shape**:
  - project index: `files` map (+ V2 `captures` sidecar map) —
    `quarto-hub/src/index.rs`
  - project set (collections home, #394): `projects` map keyed by
    indexDocId — `ts-packages/quarto-automerge-schema`
  - text file: `text`
  - binary file: `content` + `mimeType` + `hash`
  - anything else: unknown
- **Reference graph**: index doc → file docs (`files: path → docId`)
  and capture docs (`captures: path → CaptureRef{captureDocId}`);
  project-set doc → index docs. That is the complete in-storage
  reference graph today.
- **Storage**: samod `Storage` trait (`load`, `load_range(prefix)`,
  `put`, `delete`) with `TokioFilesystemStorage` rooted at
  `<data_dir>/automerge/` (`context.rs:283`). Admin tools reuse this
  adapter — no hand-rolled directory walking, and the on-disk chunk
  layout (splayed doc-id dirs with snapshot/incremental chunks) stays
  samod's implementation detail.
- **Concurrency guard**: the server holds `<data_dir>/hub.lock`.
- **CLI**: the `hub` binary is currently a flat clap `Args` (no
  subcommands); `quarto hub` forwards to the same entry, so new
  subcommands surface in both.
- **Capture writers** (all native): three call sites already share
  `quarto_core::engine::capture_files::gzip_captures` (Phase 4 of
  #410), then each does `create_binary_document` + `repo.create`.

---

## Part A — capture-doc metadata envelope

New in `quarto-hub/src/resource.rs`:

```text
ROOT
├── content: Bytes
├── mimeType: String
├── hash: String
└── meta: Map            // NEW — uncompressed, audit-readable
    ├── kind: "engine-capture"
    ├── schemaVersion: 1 (int)
    ├── createdAt: String    // RFC 3339, UTC, from the writer's clock
    ├── sourcePath: String   // project-relative qmd path, fwd slashes
    └── engines: List<String>  // e.g. ["knitr"]
```

- `pub struct CaptureDocMeta { source_path: String, engines: Vec<String> }`
  and `pub fn create_capture_document(gzipped: &[u8], meta: &CaptureDocMeta) -> Result<Automerge>`
  — builds content/mimeType/hash exactly as today (MIME stays the
  discriminator; `meta.kind` is belt-and-suspenders) plus the `meta`
  map. `createdAt` stamped inside the helper.
- The three writers switch from `create_binary_document(...,
  CAPTURE_MIME_TYPE)` to `create_capture_document(...)`, passing the
  rel path + `captures.iter().map(|c| c.engine_name)`.
- Readers are untouched: the SPA/WASM reader only reads `content`;
  docs without `meta` (all existing captures) stay valid forever.
  `meta` is written once at creation and never mutated (no CRDT merge
  concerns).

### Part A tests (TDD)

- [ ] Unit (`resource.rs`): `create_capture_document` produces
      content/mimeType/hash byte-identical semantics to
      `create_binary_document` + a `meta` map with all five fields;
      `createdAt` parses as RFC 3339.
- [ ] Unit: `detect_document_type` still classifies the doc as Binary.
- [ ] Integration (per writer, cheapest at `capture_driver`): a
      recorded capture doc read back from the repo carries `meta`
      with the right `sourcePath`/`engines`.

---

## Part B — sync-server maintainer tools (`hub admin …`)

### Design principles (damage minimization first)

1. **Read and write are separate tools.** `scan` never writes;
   `collect` never decides. `collect` accepts **only a scan manifest**
   — never ad-hoc doc ids from the command line.
2. **Nothing is ever unlinked by `collect`.** Collection = **quarantine**:
   chunks move to a trash area inside the data dir, preserving the
   storage-key layout, restorable byte-for-byte. Actual unlinking is a
   third, separately-gated step (`purge`) with a retention window.
3. **Allowlist, not blocklist.** v1 may only collect docs that are
   (a) binary docs with the capture MIME type and (b) unreferenced.
   Index docs, project sets, file docs, and **unknown-shaped docs are
   never collectible** — a future schema the scanner doesn't know
   reads as "unknown" and is automatically protected.
4. **Age gate.** Only captures whose `meta.createdAt` is older than
   `--older-than <days>` (default 30) are candidates. Pre-envelope
   captures have no timestamp → **excluded by default**; an explicit
   `--include-unstamped` opts them in (still quarantined, never
   purged before retention).
5. **Re-verify at collect time.** The manifest is a photograph;
   between scan and collect, a doc may become referenced (a client
   synced an older index state back, a re-execute repointed, a project
   was re-added). `collect` recomputes liveness for the candidate set
   against *current* storage and skips (and reports) anything that no
   longer qualifies.
6. **Never operate under a live server** (default). `collect`,
   `restore`, and `purge` refuse when `hub.lock` is held; `scan` may
   run live (it is read-only; staleness is handled by principle 5).
   samod holds loaded docs in memory and may rewrite chunks — moving
   files under it risks resurrection or wasted work, so the escape
   hatch (`--allow-live`) exists only for `scan`-equivalent safety
   analysis, not for collect.
7. **Everything is auditable.** The manifest and every quarantine
   batch carry: tool version, scan timestamp, storage path, full
   inventory counts, and per-doc evidence (kind, MIME, size, meta
   fields, and *why* it was classified removable). The trash batch
   directory embeds the manifest that produced it.

### Subcommands

```bash
hub admin scan    --data-dir <dir> [--older-than 30d] [--include-unstamped]
                  [--output manifest.json] [--json]
hub admin collect --data-dir <dir> --manifest manifest.json [--execute]
hub admin restore --data-dir <dir> --batch <trash-batch-dir-or-id> [doc-id ...]
hub admin purge   --data-dir <dir> [--retention 30d] [--execute]
```

**`scan`** (read-only):
1. Enumerate doc ids via the storage adapter (`load_range` from the
   root key; skip `storage-adapter-id`).
2. Load each doc (snapshot + incremental chunks → `Automerge`),
   classify by ROOT shape (see facts above).
3. Root set = every project-index doc + every project-set doc.
   (Project sets are roots because their only inbound pointers live in
   client IndexedDB, invisible to us. Index docs are roots for the
   same reason — share URLs.)
4. Live set = roots ∪ every doc id referenced from any index `files`
   map, any index `captures` sidecar, any project-set `projects` map.
5. Candidates = capture-MIME binary docs ∉ live set, past the age
   gate.
6. Emit the manifest + a human summary (counts and bytes by kind,
   candidate count and reclaimable bytes). The inventory alone makes
   `scan` a useful audit tool with zero risk — it also reports
   *non-capture* unreferenced docs (informational only, explicitly
   marked `not collectible in v1`).

Manifest shape (JSON, versioned):

```json
{
  "manifestVersion": 1,
  "tool": "hub admin scan <crate version>",
  "scannedAt": "2026-07-24T…Z",
  "dataDir": "/srv/hub/data",
  "inventory": { "project-index": 3, "project-set": 1, "text-file": 41,
                  "binary-file": 12, "engine-capture": 9, "unknown": 0,
                  "bytesByKind": { "…": 0 } },
  "candidates": [
    { "docId": "4EBfN…", "kind": "engine-capture",
      "sizeBytes": 812345,
      "meta": { "createdAt": "…", "sourcePath": "posts/a.qmd",
                 "engines": ["knitr"] },
      "reason": "capture MIME; not referenced by any index/captures/project-set; age 41d > 30d" }
  ],
  "notCollectible": [ { "docId": "…", "kind": "unknown", "reason": "unknown shape" } ]
}
```

**`collect`** (quarantine; dry-run by default):
1. Refuse if `hub.lock` held. Refuse if manifest's `dataDir` doesn't
   match (paranoia against pointing the wrong manifest at the wrong
   server).
2. Re-verify each candidate (principle 5): still exists, still
   capture-MIME, still unreferenced, still past age gate.
3. Dry-run prints the verified plan. With `--execute`: for each doc,
   move its chunks to
   `<data_dir>/trash/<UTC-timestamp>-<scan-short-id>/<doc-id>/<original-key-path…>`
   and write `batch.json` (the manifest + per-doc verification results
   + chunk inventory with hashes) into the batch dir.
   Move is per-doc atomic-ish (rename within the same filesystem);
   a failure mid-batch leaves already-moved docs quarantined and
   reports precisely which.
4. `trash/` lives inside the data dir on purpose: same filesystem
   (rename not copy), covered by whatever backup regime the operator
   already has, and impossible to orphan the trash from its server.

**`restore`**: moves a batch's chunks (or a named subset) back into
place, verifying the target keys are absent first (a doc re-created
under the same id since collection → refuse for that doc, report).
Chunk hashes from `batch.json` are verified on the way back.

**`purge`** (the only unlink): deletes trash **batches**, never
individual live-store docs; only batches older than `--retention`
(default 30d); dry-run by default; `--execute` required; prints what
it deletes. Accidental-deletion story end-to-end: a mistake must
survive `scan` (evidence-based), `collect --execute` (re-verified,
quarantine only), a retention window, and `purge --execute` before
bytes are actually gone — with `restore` available at every point
until purge.

### What the tools deliberately do NOT do (v1)

- Collect anything other than capture-MIME docs (index/file/set/
  unknown docs are inventory-only).
- Run against live servers for mutating operations.
- Reason about **peers**: on a replicated/peered deployment, this
  storage location's reference graph is the only truth consulted.
  The runbook must say: run against the authoritative store; a doc
  collected here can resurface via a peer that still holds it (samod
  sync would re-fetch it if referenced — which is exactly the safe
  direction; an *unreferenced* doc re-synced from a peer just becomes
  a future scan candidate again).
- Compact/GC automerge history inside live docs (different problem).

### Placement

- `crates/quarto-hub/src/admin/` — `mod.rs`, `classify.rs`
  (shape classification + reference extraction, pure functions over
  `Automerge`, unit-testable without storage), `scan.rs`,
  `manifest.rs` (serde types, versioned), `collect.rs` (quarantine +
  restore + purge).
- `main.rs` grows `#[command(subcommand)]` with the existing flat
  serve behavior as the default command, so `hub --port …` keeps
  working; `hub admin <sub>` dispatches to the new module. (`quarto
  hub admin …` comes along for free.)
- Operator runbook: `claude-notes/instructions/hub-storage-hygiene.md`
  (promote to `dev-docs/` if/when deployments want it public).

### Part B tests (TDD)

- [ ] Unit (`classify.rs`): each doc kind fixture classifies
      correctly; unknown shapes classify as unknown; reference
      extraction returns exact id sets for index and project-set docs.
- [ ] Integration (build a real repo via `HubContext` in a temp dir:
      project with files + a capture, then re-execute so an orphan
      exists):
      - `scan` inventory counts match; exactly the orphaned capture is
        a candidate; the live capture and all file docs are not.
      - unstamped (legacy) capture excluded without
        `--include-unstamped`, included with it.
      - `collect` dry-run changes nothing on disk (tree hash equal).
      - `collect --execute` quarantines only the candidate; reopening
        the project via `HubContext` still serves every file and the
        live capture.
      - re-verification: re-reference the candidate between scan and
        collect (write it back into a `captures` sidecar) → skipped.
      - `restore` brings chunks back byte-identical (hash check);
        subsequent `repo.find` loads the doc.
      - `purge` refuses young batches; deletes old ones with
        `--execute`.
      - lock guard: collect refuses while a `HubContext` holds the
        data dir.
- [ ] E2E: on a locally-run `hub` with a real project (create,
      execute, re-execute), stop the server, run scan→collect→restore
      →purge through the actual binary; record invocations + output in
      this plan.

## Design decisions (reviewed with Carlos, 2026-07-24)

1. **`hub admin` subcommands** on the existing binary (no separate
   `hub-admin` artifact); `quarto hub admin` comes along for free.
2. **30d defaults** for both `--older-than` and `--retention`, both
   CLI-configurable (they already are in the design; keeping them
   flags, no config-file plumbing in v1).
3. **Live-server `scan` allowed** — read-only, snapshot semantics; no
   consistency is guaranteed anywhere in this system anyway (even an
   offline store can be behind a peer). Mutating subcommands still
   require the lock to be free.
4. **Unreferenced non-capture docs**: report-only in v1; collection of
   e.g. abandoned project indexes is a follow-up strand once real
   inventories are seen.
5. **`--json` output is in** for `scan`; the manifest schema is
   versioned so a future hub-client admin page can consume it.

## Work items

- [x] Phase A: metadata envelope (commit 38af41d3) — includes
      `read_capture_meta` and the `create_capture_document_at` test
      seam for the age gate; `CAPTURE_MIME_TYPE` consolidated into
      quarto-hub.
- [x] Phase B1: `classify.rs` + unit tests
- [x] Phase B2: `scan` + manifest + integration tests (commit
      e1ed2c3b). Implementation note: whole-store enumeration cannot
      use `Storage::load_range([])` — the filesystem adapter splays
      doc ids across two path components and load_range returns raw
      components; enumeration is an explicit backend-specific step
      (`list_doc_ids_filesystem`), per-doc chunk loads stay on the
      adapter. Caught by the real-store integration test.
- [x] Phase B3: `collect` (quarantine) + `restore` + `purge` + lock
      guard + integration tests (commit 4ffc1f8f). Refinement: the
      guard doesn't just *check* hub.lock — mutating subcommands
      ACQUIRE the server's exclusive flock for their duration, so a
      server also can't start mid-operation.
- [x] Phase B4: CLI wiring (`hub admin`), binary E2E, runbook
      (`claude-notes/instructions/hub-storage-hygiene.md`)
- [x] Phase C: `cargo xtask verify --skip-hub-build` passes (Rust-only
      change; hub-client/WASM do not depend on quarto-hub)

## End-to-end verification record (2026-07-24)

Store built entirely by shipped binaries: `q2 preview` on the knitr
repro records a (meta-stamped) capture; a real
`POST /api/preview/re-execute` supersedes it, orphaning the old doc.
Then the `hub` binary drives the whole pipeline
(`scratchpad/e2e-admin/run.sh`; output inspected):

1. **Live scan** (server still running, read-only): inventory
   `engine-capture 2 / project-index 1 / text-file 1`; exactly the
   superseded capture flagged — `22821 bytes  hello.qmd` (sourcePath
   from the Phase A envelope). `--older-than-days=-1` exercised the
   age-gate-disable escape hatch (fresh captures).
2. Store copied out live (preview temp data dirs are deleted on
   shutdown — operating on a snapshot mirrors backup-based
   maintenance), `hub admin scan --output manifest.json` offline.
3. `collect` dry-run: `WOULD COLLECT`, tree untouched.
4. `collect --execute`: `QUARANTINED` into
   `trash/20260724T162747Z-scan081c78a2`, restore command printed.
   Post-collect scan: 0 candidates.
5. `restore`: `RESTORED` (hash-verified); scan shows the candidate
   again.
6. `purge`: young batch `KEPT` at default retention;
   `--retention-days=-1 --execute` → `PURGED`.

**Bonus validation:** the first E2E attempt ran a stale `q2` binary
(pre-Phase A) whose captures were genuinely unstamped — scan reported
the orphan as `unstamped (pre-envelope), excluded by default` and
collected nothing. The legacy-capture protection gate, validated with
authentic legacy data.
