---
title: 'Git epochs: registry and index contract'
date: 2026-11-18
description: 'Normative contract for the git-epochs sidecar registry, the version 4 index keys, the arrival and startup decision tables, the status route, the event vocabulary, the swap events, and the module map that the Rust crate and the TypeScript package both build.'
---

**Status:** normative. The Rust crate `quarto-git-epochs` (GE-2), the TypeScript package
`@quarto/quarto-git-epochs` (GE-5), the hub (GE-3, GE-4), the swap module (GE-6), and the extension (GE-8 to GE-12)
all follow this file. The epic is [git epochs](../plans/2026-11-18-git-epochs-epic.md); decision ids (G6, G7, ...)
refer to its decision tables. Where this file and a plan disagree, fix the disagreement in the same commit.

The shared fixtures live in `ts-packages/quarto-git-epochs/fixtures/`. Every row schema below is validated by
`fixtures.test.ts` and read by both implementations.

## Sidecar doc

One Automerge doc per project, referenced from the index by the key `epochs` (see [Index version 4](#index-version-4)).

| Field | Type | Meaning |
|---|---|---|
| `version` | integer, `1` | Sidecar schema version. A reader that sees a higher version leaves the doc alone |
| `entries` | flat map, key to integer | One key per registered doc. The value is the doc\'s generation, stored as an Automerge `int` |
| `retired` | flat map, key to integer | Same key shape. Grow-only: no code path ever removes a key. An entry means the doc no longer stands for that path at that blob |

**Key.** `path`, a NUL byte (U+0000), the blob id, a NUL byte, the docId, concatenated as one string. The path is
project-relative with `/` separators; the blob id is git\'s object id of the file at HEAD (the same value
`git ls-tree` prints); the docId is the string form of the Automerge document id. Because every entry is its own key,
concurrent additions are a set union and nothing is overwritten (G6). A nested `path`, `blob`, `doc` map loses one
side of a concurrent creation; Gate 0 item 6 reproduced this.

**Creating the maps.** `entries` and `retired` are created once, in the same change that creates the sidecar doc, by
the one writer that creates it. A second writer must never create either map in a later change: two concurrent
creations of the map object itself lose one side exactly like the nested form (Gate 0 item 6). If two peers create
sidecars at once, the index key `epochs` resolves to one winner; the loser adopts the winner\'s doc and re-registers
its attached docs (G6).

**Integers.** The generation is written as an Automerge `int`. Both languages read it back as a number. A NUL byte in
a key survives save and load in both (Gate 0 item 6).

**Soft limit.** 15,000 tracked paths, about 2 MB and about 100 ms to load in JavaScript (Gate 0 item 9). Above the
limit a peer logs one warning per session and keeps working. The registry holds participating files only.

## Scalar forms

Every string value this contract names (the `epochs` pointer, `owner`, each item of `repo.roots`, `repo.prefix`) is
read in either form: a scalar string, or a Text object. A JavaScript client writes a string as Text, the Rust hub
writes a scalar string, and each reads the other\'s form as a distinct type (`ImmutableString` in JavaScript, `Text`
in Rust; see `index.rs` and `rust-binary-scalars.test.ts`). Writers use their platform\'s default form. Readers must
accept both. Every golden document exists in both forms.

## Index version 4

The index doc moves from version 3 to version 4 with no structural change. Existing keys (`files`, `version`,
`identities`, `folders`) are unchanged. New optional keys:

| Key | Value | Written by |
|---|---|---|
| `epochs` | The sidecar docId (a string) | Whichever git-aware peer needs the sidecar first |
| `owner` | The originating folder\'s random `ownerId`. Absent for hub-owned projects | The extension that originates a project (G16) |
| `repo` | A map with `roots` (a list of root commit hashes) and `prefix` (the project path inside the repo, empty for the repo root) | A git-aware owner (G17, G24) |

`createIndexDocumentContent()` and the Rust creation path write exactly `files`, `version: 4`, `identities`, and
`folders` for a new project, and nothing else. `migrateIndexDocument` raises a version 3 doc to 4 as a pure version
bump and reports a change; it leaves a doc at version 4 or above untouched. **Unknown keys are preserved by every
reader and writer**: no accessor rebuilds the document.

## Operations

All operations are pure functions of the sidecar content plus plain-data inputs; the doc mutation itself is the
caller\'s.

- **Register.** Put-if-absent on `entries` for the key `(path, blob, doc)` with the doc\'s generation.
- **Canonical selection** for `(path, blob)`: among the keys with that path and blob that are not in `retired`, the
  lowest generation, then the lowest docId compared bytewise on its string form (identical in Rust and TypeScript).
  A creation race resolves by set union, with no timing involved (G7).
- **Creation.** A new doc for a path gets generation `max + 1`, where `max` runs over every entry of the path on any
  blob and the maximum over no entries is -1. So the first doc of a path has generation 0, the same value startup
  registration writes, and an established doc beats a latecomer. Concurrent creators tie and break by docId (G7).
- **Move forward.** In one change, add `(path, newBlob, doc)` carrying the doc\'s existing generation and remove
  `(path, oldBlob, doc)`. An amend does the same. When the doc has no entry under the old blob (a step 5 fallback
  doc), the new entry gets generation `max + 1` (G8).
- **Retire.** The `reset --hard` step (arrival step 2b): add `(path, blob, doc)` to `retired`. The key stays in
  `entries`; canonical selection skips it. Nothing is ever removed from `retired`.
- **Repair.** After each sidecar sync, a peer whose attached doc has no entry on any blob and is not retired
  registers it again under its HEAD blob with its persisted generation. Repair never revives a retired entry and
  never re-registers a doc that moved forward (G6).
- **Fork rule.** A doc is live under at most one path. If an arrival would attach a doc that the index lists under
  another path, fork it with `repo.clone(handle)` (new docId, shared history; Gate 0 item 8: the fork keeps history,
  diverges independently, and syncs). **Fallback**, if a fork ever fails to sync: create a doc from the content with no
  history. Item 8 found no need for the fallback (G12).
- **Case collision.** Two registry keys whose lowercased path part is equal fall back to ordinary sync for that file
  (compare the lowercased path part of the key) (G12).
- **Unavailable canonical doc.** Wait 5 s, treat the doc as absent for this arrival, and create one (G11).

## Arrival

Arrival runs per changed file after a checkout-type decision.

**Inputs** (plain data):

| Field | Meaning |
|---|---|
| `kind` | `text` or `binary` |
| `disk` | Disk content: CRLF-normalized text, or bytes (hashed) |
| `doc` | Canonical doc content, or absent |
| `blob` | Blob content at the new HEAD |
| `attached` | `none`, `canonical`, or `other` (attached to a doc that is not the canonical one) |
| `retiredByOther` | The attached doc was retired by another peer |
| `policy` | `ReadOnly` or `WriteBack` (hub only) |
| `peer` | `hub` or `extension` |
| `jj` | Colocated jj: every changed file takes step 5 |

**Outputs** by step, with S the disk content, T the canonical doc\'s content, B the blob content:

| Step | Condition | Action |
|---|---|---|
| 1 | No registered doc | Create from S, register, flip |
| 2a | Already attached to the canonical doc, and not (S equals B and T differs) | Do not merge disk into it; re-baseline |
| 2b | Already attached to the canonical doc, S equals B, T differs | Retire `(path, B, doc)`, create from S with generation `max + 1`, register, flip; the discarded edits stay in the retired doc and are counted in `discarded` |
| 3 | S equals T | Attach silently |
| 4 | S differs from T, S equals B (clean) | Join the doc (it holds live edits not in git) with a notice and a "use my checkout instead" action. Hub: join, log a warning, record `joined` |
| 5 | S differs from T, S differs from B (dirty) | Do not overwrite. Extension: detach the file (inert) and ask, adopt or stay detached. Hub: create from S, flip, leave unregistered until the next commit-type decision, record `seeded` |

**Binary files** follow the same steps with byte hashes; adopting means taking the other peer\'s bytes when disk is
clean. **ReadOnly variant:** a hub under `ReadOnly` attaches only when S equals T; otherwise it creates a doc from S,
because it must not write its disk. **jj fallback:** under colocated jj every changed file takes the step 5 behavior
(the hub records `seeded`; an extension detaches). A peer attached to a doc that another peer retired stays in it,
live, and runs ordinary arrival at its next change.

## Startup

With a persisted record of `{head, blobs}` and the detached set (G10):

| Case | Action |
|---|---|
| Record present, HEAD unchanged | Reconcile as before (offline edits merge) |
| Record present, HEAD changed | Run arrival for each file whose blob differs; reconcile the rest |
| No record | Run arrival for every participating file; never merge disk into a doc |

An offline `git reset --hard` with an unchanged HEAD is the known gap.

## Classifier

Content rule first: disk equals the last synced content means commit-type, otherwise checkout-type. The reflog
subjects since the last decision are the tiebreaker: if every entry is a plain `commit` or `commit (amend)` the
verdict is commit-type even with a differing disk (the author\'s own edit); if any entry is something else the verdict
is checkout-type (G2). Colocated jj and repos without a reflog use the content rule alone. A missing `logs/HEAD` means
"no reflog" and is not an error.

Recorded subjects (Gate 0 item 7, git 2.55.0): `commit: <msg>`, `commit (amend): <msg>`, `cherry-pick: <subject>`,
`revert: Revert "<subject>"`, `checkout: moving from X to Y` (`switch` and `checkout`), `pull ... : Fast-forward`
(arguments as typed), `rebase (start): ...`, and `reset: moving to HEAD` for **both** `git stash` and `git reset
--hard` (not distinguishable by subject; the content rule and the unchanged blob tell them apart from a commit).

**Gate ordering** (Gate 0 item 7): git creates `index.lock` first, writes the working files, releases the lock and
updates `.git/index` in the same millisecond, and writes the `logs/HEAD` entry 0.7 to 1.3 ms after the lock release.
So the reflog may not yet have changed when the lock disappears, and a peer skips any file modified in the last
500 ms in its periodic loop (G1).

## Surfaces

### `GET /api/preview/git-status`

```json
{
  "tracking": true,
  "paused": false,
  "pending": false,
  "markers": ["index.lock"],
  "decisions": [
    { "kind": "switched", "time": "2026-11-18T12:00:00Z", "files": ["a.qmd"], "step": "3", "discarded": 0 }
  ]
}
```

`markers` names the markers holding the gate. `decisions` holds the last 20, newest last: `kind` from the vocabulary
below, an RFC 3339 `time`, `files`, the G9 `step` (a string: `1`, `2a`, `2b`, `3`, `4`, `5`, or `none` for a
commit-type decision), and `discarded`, the number of discarded edits a retire step set aside (zero otherwise).

### Event vocabulary

One list, shared by the hub\'s decision ring and the extension\'s `events.jsonl`, in `fixtures/vocabulary.json`:
`flipped`, `switched`, `detached`, `reattached`, `held`, `reset`, `retired`, `moved-forward` (a commit-type
decision), `joined` (the hub\'s step 4), `seeded` (the hub\'s step 5 and the jj fallback).

### Swap events and notices

Swap phases (G26): `pending`, `done`, `unavailable`. The notice texts live in GE-6\'s `notices.ts` only; every other
plan imports the constants. The constant names are `NOTICE_SWITCHED_VERSIONS`, `NOTICE_STRANDED_EDITS`,
`NOTICE_UNAVAILABLE`, `NOTICE_DETACHED`, `NOTICE_JOINED_LIVE`, and `NOTICE_EDITS_NOT_KEPT` (GE-6 may add more, and
must keep these).

### Moved message

A peer that leaves a doc because of a switch sends one ephemeral message on the old doc: `{ "type": "moved",
"peerId": "<id>" }`. It has the same shape as the existing `leave` message in
`hub-client/src/services/presenceService.ts:49-53` and no reason field (G25).

### Numbers

| Quantity | Value |
|---|---|
| Preview periodic sync | 5 s |
| Quiet window | 1 s |
| Decision cap | 30 s |
| Stale lock | 60 s |
| Client freeze and unavailable timeout | 5 s |
| Unavailable canonical doc | 5 s |
| Editor-only wait for a flip | 10 s |
| Proxy readiness | 15 s |
| Publish cap per file | 10 MB |
| Retired handles kept | 8 per path |
| Controller tick while pending | 250 ms |
| Mtime skip after a lock release | 500 ms |
| Registry soft limit | 15,000 paths |

The hub\'s own default sync interval of 30 s is unchanged.

## Module map

Both implementations build the same six modules with the same names and plain-data inputs and outputs. In Rust they
are modules of `quarto-git-epochs`; in TypeScript they are files of `src/` (`probe` is node-only and exported from
`./probe`).

| Module | Entry points | Role |
|---|---|---|
| `classify` | `classify(input) -> Verdict` | Commit-type or checkout-type from content, hold set, and reflog subjects |
| `arrival` | `arrive(input) -> ArrivalOutput` | The five steps and the retire step, per file |
| `startup` | `startup(input) -> StartupPlan` | The three startup cases |
| `registry` | `register`, `canonical`, `create`, `moveForward`, `retire`, `repair`, `caseCollision` | Pure operations over the flat entries and retired sets |
| `probe` | `probeRepo`, `cheapState`, `headBlobs`, `rootCommits`, `isShallow`, `pathPrefix` | The git subprocess calls (plumbing only, `GIT_OPTIONAL_LOCKS=0`, timeouts) and the cheap synchronous state read; a missing reflog file means "no reflog", not an error |
| `controller` | `tick(state, now) -> (state, actions)`, `check() -> state` | The tick-driven machine (gate, quiet window, 30 s cap) and the synchronous `check()` that every sync entry point calls first |

The pure core takes and returns plain data and does no I/O. Rust uses `snake_case` for these names and TypeScript
`camelCase`; the fixtures use the TypeScript spelling in field names, and the Rust side maps them with serde renames.

## Row schemas

All fixtures are JSON under `fixtures/`. Every row has a unique string `id`.

**`decision-tables/classify.json`:** `{ "id", "peer", "reflog": [string], "changed": [{"path", "disk", "synced", "blob"}], "expected": "commit" | "checkout", "note"? }`. `reflog` may be `null` for a repo with no reflog.

**`decision-tables/arrival.json`:** `{ "id", "peer": "hub" | "extension", "policy"?: "ReadOnly" | "WriteBack", "kind": "text" | "binary", "disk", "doc", "blob", "attached": "none" | "canonical" | "other", "retiredByOther"?: boolean, "jj"?: boolean, "expected": { "step", "action", "discarded"?: number, "event"? } }`. `policy` is required when `peer` is `hub`. Optional input fields for the cases outside the five steps: `canonicalUnavailable` (the canonical doc cannot be fetched), `liveUnderOtherPath` with `forkSyncs` (the fork rule and its no-history fallback), and `caseCollision`. For a row outside the five steps, `expected.step` names the step the row would otherwise take, and `expected.action` carries the difference (`wait-5s-then-create`, `fork-then-attach`, `create-from-content-no-history`, `ordinary-sync`, `stay-live`). Rows may carry a free-text `note`.

**`decision-tables/startup.json`:** `{ "id", "record": null | {"head", "blobs"}, "head", "expected": "reconcile" | "arrive-differing" | "arrive-all" }`.

**`decision-tables/registry.json`:** `{ "id", "op", "entries": [...], "retired": [...], "args": {...}, "expected": {...} }`, where an entry is `{"path","blob","doc","gen"}`.

**Scenarios** (`scenarios/*.json`): `{ "id", "title", "steps": [ step ] }`. A step is `{ "git": [args...] }` (run `git` with those arguments), `{ "write": {"path","content"} }`, `{ "remove": "path" }`, or `{ "writeBinary": {"path","hex"} }`, each with an optional `"expect": {"verdict": "commit" | "checkout" | "none"}`. There are no shell scripts, so Windows runs them unchanged.

**`vocabulary.json`:** `{ "events": [string], "swapEvents": [string] }`, equal to the lists above.

**Golden documents** (`golden/*.bin`, one Automerge doc each, plus `golden/index-v4.json`): `golden/manifest.json` is `{ "<file>": { "entries": [ {"key","gen"} ], "retired": [ {"key","gen"} ] } }`, the decoded content a reader must produce. A manifest entry for an index doc is `{ "index": { "epochs", "owner", "repo": {"roots","prefix"} } }` instead of `entries` and `retired`; `index-v4-text-form.bin` and `index-v4-scalar-form.bin` hold the same values as a Text object and as a scalar string (see [Scalar forms](#scalar-forms)). `index-v4.json` is the content of `createIndexDocumentContent()`. `scripts/make-golden.mjs` regenerates every `.bin` and the manifest. GE-2 and GE-5 each add one document written by their own implementation.
