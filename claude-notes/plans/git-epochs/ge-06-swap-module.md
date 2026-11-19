---
title: 'GE-6: The swap module, the notice aggregator, the moved message'
date: 2026-11-18
description: 'Builds the pure TypeScript swap coordinator that every follower uses when the project owner flips a path to a different doc, with the shared notice wording and the type of the moved presence message.'
status: done
depends-on: [ge-1]
lane: B
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-1](ge-01-contract-and-gate-0.md) ·
**Unblocks:** [GE-7](ge-07-client-integration.md) (browser clients) and, with GE-5 and GE-8,
[GE-9](ge-09-extension-follower-modes.md) (the extension\'s mirror and editor-only modes) · **Branch:**
`ge/06-swap-module` off `feature/git-epochs`, merged by rebase and fast-forward; nothing is pushed.

## Goal

When the project owner flips `files[path]` to a different doc, every follower has to swap: stop writing to the old doc,
load the new one, rebind, and tell the user. Browsers will do this inside `quarto-sync-client` (GE-7); mirrors and
editor-only folders will do it inside the extension (GE-9). The hard parts are the same everywhere, so they are written
once here as a pure state machine with injected I/O, and tested once with a fake host. This plan also fixes the notice
wording in one place and the TypeScript type of the moved presence message.

## Decisions implemented

See the [epic decisions](../2026-11-18-git-epochs-epic.md#decisions).

- **G26** the shared swap module: phases `pending`, `done`, `unavailable`; per-path serialization with the latest index
  state winning; a 5 s timeout; retired handles kept, at most 8 per path, never deleted; docId-bound writes with a
  stranded-edit counter; one aggregate notice per batch.
- **G25** the moved message: the type is defined once in the shared package (`src/moved.ts`, from the fields GE-1\'s
  contract lists: `{ type: 'moved', peerId }`); this plan exports it and the short text shown for it.
- **G26** the notice constants: every notice text of the epic is an exported constant in `src/notices.ts`, the one
  source; GE-7, GE-9, and GE-10 import them and never restate them.
- **G15** vocabulary: the notices say \"project owner\", not \"host\".
- **G32** the numbers: freeze and unavailable timeout 5 s, retired handles 8 per path.

## Context

- Today a changed docId for an existing path is ignored. `syncWithFiles` compares path sets only
  (`ts-packages/quarto-sync-client/src/client.ts:1393-1440`) and, for new paths, awaits `findDoc` inline in a loop
  (`:1399-1412`), so one stalled fetch delays every later index change.
- The precedent for swapping a handle in place is `recoverFileDocument` (`client.ts:1086`): it detaches the change
  handler, resubscribes, and re-fires `onFileAdded`; the cursor and undo history reset is accepted there.
- The extension\'s `syncIndex` untracks, retracks, and writes the new doc to disk with no freeze and no stranded-edit
  accounting (`vscode-sync-experiment/src/project.ts:322-330`). GE-9 replaces it with this module.
- The package is `ts-packages/quarto-git-epochs` (skeleton, `test` script, and CI step from GE-1). The module lives in
  `src/swap.ts`. It imports nothing from `vscode`, IndexedDB, or Automerge, and uses no timer except through the
  injected clock. The contract spec from GE-1 (under `claude-notes/designs/`) defines the moved message shape; this
  plan implements the TypeScript type from it and invents no field.
- Only TypeScript needs this module. The Rust hub never follows a pointer, so there is no Rust twin.

## The API

A sketch; names may be refined, the shape is the decision.

```ts
export type SwapPhase = 'pending' | 'done' | 'unavailable';

export interface ReplacedEvent {
  path: string;
  phase: SwapPhase;
  oldDocId: string;
  newDocId: string;
  stranded?: number; // on 'done': writes that stayed with the old doc so far
}

// Everything environment-specific is injected. H is the host's handle type.
export interface SwapHost<H> {
  currentDocId(path: string): string | undefined;
  loadDoc(docId: string, signal: AbortSignal): Promise<H>; // rejects if unavailable
  rebind(path: string, newDocId: string, newHandle: H, oldHandle?: H): void | Promise<void>;
  // Optional. Edits this peer made to the old doc that the server has not confirmed yet (for example, typed while
  // offline), counted as stranded when a swap starts (detail chosen by the third reviewer; confirm).
  unconfirmedLocalEdits?(path: string, oldHandle: H): number;
  onReplaced(event: ReplacedEvent): void;
  clock?: { setTimeout: typeof setTimeout; clearTimeout: typeof clearTimeout };
}

export interface SwapOptions {
  timeoutMs?: number; // default 5000
  retiredCap?: number; // default 8
}

export type WriteTarget<H> =
  | { kind: 'live'; docId: string; handle: H }
  | { kind: 'retired'; docId: string; handle: H } // counted as stranded
  | { kind: 'drop'; reason: string };

export interface SwapCoordinator<H> {
  indexChanged(path: string, docId: string): void; // start, queue, or coalesce
  resolveWriteTarget(path: string, baseDocId?: string): WriteTarget<H>;
  retired(path: string): readonly { docId: string; handle: H }[];
  takeStranded(path: string): number; // returns and resets (detail chosen by the plan author; confirm)
  dispose(): void;
}

export function createSwapCoordinator<H>(host: SwapHost<H>, opts?: SwapOptions): SwapCoordinator<H>;

export class NoticeAggregator {
  add(event: ReplacedEvent): void;
  addStranded(n: number): void; // (detail chosen by the plan author; confirm)
  flush(openFile?: string): string | null; // one aggregate per batch; null when there is nothing to say
}
```

Behaviors, each with a test:

- **Serialize per path, latest state wins.** If the index flips twice before the first swap completes, the first
  load is aborted through its signal and the swap goes straight to the final docId; the intermediate doc is never
  bound. One `pending` event fires per batch (detail chosen by the plan author; confirm).
- **Edits during `pending`.** The old handle is still bound, so a write with `baseDocId` equal to the old docId
  resolves to the old handle and counts as stranded, because that handle is about to be retired. After `done` such a
  write resolves as `retired`. A write with no `baseDocId` resolves to the live handle, as today.
- **Edits made before the swap began.** When a swap starts, the coordinator adds `unconfirmedLocalEdits(path, oldHandle)`
  (when the host provides it) to the path\'s stranded count, because an edit typed offline is already in the old doc
  and never passes through `resolveWriteTarget` (GE-13 S3 and GE-14 H2 expect the notice to say it stayed).
- **Timeout.** If the new doc is not loaded after 5 s, fire `unavailable` and leave the old doc bound. The host routes
  the path into its unavailable-file machinery; the path stays read-only (the host\'s job).
- **Retired handles** are kept for the life of the coordinator, at most 8 per path (the oldest is forgotten, never
  deleted from any repo), so a late edit against a retired doc can still reach it.
- **Flip back** to a docId the path has had reuses its retired handle instead of loading it again. **Flip to the
  current docId** does nothing.
- **Independence.** Swaps run on separate promise chains per path, so a load that never completes for one path never
  delays another path.
- **Unknown `baseDocId`** resolves to `drop` with a reason; the host logs a console warning.

Notice wording, from `NoticeAggregator.flush`. One aggregate per batch. A line about edits is appended when the stranded
count is nonzero.

| Replaced in the batch | Text |
|---|---|
| One file, and it is the open file or none is open | This file was replaced because the project owner\'s checked-out version changed. |
| One file, not the open file | The file `path` was replaced because the project owner\'s checked-out version changed. (detail chosen by the plan author; confirm) |
| N files | N files were replaced because the project owner\'s checked-out version changed. |
| Any, with N stranded edits | Append: N of your recent edits stayed with the previous version. |

`unavailable` events are not counted in the notice; the host shows the unavailable marker it already has. The strings
are exported constants in `src/notices.ts`, and so are all the other notice texts of the epic, which this plan owns
so that no client restates one. The other plans refer to them by constant name:

| Constant | Text | Used by |
|---|---|---|
| `PAUSED_BANNER` | Paused for git: edits to files the project owner rewrites may be set aside | GE-7 |
| `BRANCHES_STAY` | N branches of this file stay with the previous version | GE-7 |
| `GIT_OP_EDITS_DROPPED` | Edits made during a git operation were not kept. | GE-9 |
| `EDITOR_ONLY_NO_FLIP` | The project owner has not switched versions yet. Keep waiting? (actions: Keep waiting, Dismiss) | GE-9 |
| `FILES_DETACHED` | N files are detached: your checked-out version differs from the shared one. Your edits to them no longer reach the project. (actions: Adopt shared version, Stay detached) | GE-10 |
| `FILES_JOINED_LIVE` | N files joined live edits from collaborators that are not committed to git. (action: Use my checkout instead) | GE-10 |
| `ADOPT_CONFIRM` | This file has uncommitted changes. Replace it with the shared version? | GE-10 |
| `DOC_SUPERSEDED` | Your doc was superseded | GE-10 |

(texts chosen by the plan authors of GE-7, GE-9, and GE-10; confirm.)

The moved message (G25) is a type and a text helper only: `MovedMessage` defined in `src/moved.ts` from the fields in
the GE-1 contract (`{ type: 'moved', peerId }`, the one message type for every client, with no reason field on
`leave`), and
`movedNotice(name?: string): string` returning `"<name> switched versions"`, or \"This person switched versions\" when
there is no name (detail chosen by the plan author; confirm). Sending and showing it is GE-7 for browsers and
GE-9 and GE-10 for the extension.

## Test specifications

All with a fake host whose `loadDoc` returns promises the test resolves or rejects by hand, and a fake clock. No real
timers; nothing outlives a test.

1. Phases: `pending` then `done`, with `rebind` called once with the new handle and the old one.
2. Two quick flips of one path: one swap to the final docId, the first load aborted, `rebind` called once.
3. A stalled load for doc A does not delay a swap of path B to doc B (independence).
4. `resolveWriteTarget`: live with no `baseDocId`; live and counted during `pending` for the old docId; retired after
   `done`; `drop` for an unknown docId.
5. The stranded counter: counts during `pending` and after `done`; `takeStranded` returns and resets; the `done` event
   carries the count at that moment; a host whose `unconfirmedLocalEdits` returns 2 at swap start produces a `done`
   event with `stranded` of at least 2.
6. The 5 s timeout fires `unavailable` and leaves the old doc bound; a later `indexChanged` to the same docId retries.
7. The retired cap: the ninth retired handle drops the oldest; none is deleted by the module.
8. Flip back reuses the retired handle without calling `loadDoc`; a flip to the current docId calls nothing.
9. `dispose` aborts in-flight loads and clears timers.
10. `NoticeAggregator`: one file; one file that is not the open file; many files; stranded edits appended; `unavailable`
    ignored; two batches produce two notices; `flush` with nothing returns null.
11. `movedNotice` with and without a name.

## Checklist

- [x] Confirm GE-1 is complete and read the moved-message shape in its contract.
- [x] Write the test file `src/swap.test.ts` (items 1 to 9) and `src/notice.test.ts` (items 10 and 11) against the API
  above; run them and see them fail because the module does not exist.
- [x] Implement `createSwapCoordinator`: per-path state, abort signals, the injected clock, the retired list with its
  cap, and the stranded counter. Make tests 1 to 9 pass one at a time.
- [x] Implement `NoticeAggregator` and `movedNotice`; tests 10 and 11 pass.
- [x] Define `MovedMessage` in `src/moved.ts`, the constants in `src/notices.ts` (every notice text of the epic, one
  constant each, with a test that lists them), and re-export the whole surface from `src/index.ts`.
- [x] Add the module to the package README: the API, the host contract, and the rule that wording lives here only.
- [x] Check there is no import of `vscode`, IndexedDB, or Automerge, and no direct use of a global timer
  (`grep` the file; the injected clock is the only way).
- [x] Update this plan\'s checkboxes and fill in the notes below.

## Gates

```bash
npm test -w @quarto/quarto-git-epochs
npm run typecheck -w @quarto/quarto-git-epochs
npm run build -w @quarto/quarto-git-epochs
cargo xtask lint
```

## End-to-end verification

A pure module has no binary of its own. Run the test suite, and record in the notes the test count and the result.
The module is exercised against a real client in [GE-7](ge-07-client-integration.md) (two sync clients through the test
hub) and in the extension in [GE-9](ge-09-extension-follower-modes.md); say here that this plan did not exercise it
against a real handle.

## Exit criteria

- Tests 1 to 11 pass; the module is exported and documented.
- The notice text matches the table, in one place only.
- The module has no environment dependency (no `vscode`, IndexedDB, Automerge, or global timer).

## Out of scope

- Wiring into `quarto-sync-client` and the consumers (GE-7), and into the extension (GE-9).
- Sending or displaying the moved message.
- Freezing an editor (each host does that); the module only reports the phases.

## Notes from the run

**Session 1 (lane B, 2026-11-18).**

- GE-1 is done on `feature/git-epochs`. The moved message in its contract is `{ type: 'moved', peerId }`, with no
  reason field; `src/moved.ts` implements exactly that.
- Test first: `src/swap.test.ts` and `src/notice.test.ts` were written against the API and run red because
  `swap.js`, `notices.js`, and `moved.js` did not exist. The implementation then made them pass. Two tests beyond the
  plan's list were added: flipping back to the still-bound doc while pending, and an index change that arrives during
  an async `rebind`.
- Result: `npm test -w @quarto/quarto-git-epochs` 51 passed (25 from GE-1 plus 26 new); `npm run typecheck` and `npm run
  build` clean; `cargo xtask lint` clean. A grep of `swap.ts`, `moved.ts`, and `notices.ts` finds no `vscode`,
  IndexedDB, or Automerge, and the only `setTimeout`/`clearTimeout` references are through the injected `clock`.
- Not exercised against a real handle: this is a pure module with a fake host. GE-7 (two sync clients through the test
  hub) and GE-9 exercise it for real.
- Choices beyond the plan's sketch (detail chosen by the plan author; confirm, so I took the plan's choice where it
  gave one):
  - `SwapHost` gains a required `currentHandle(path)`: the coordinator only learns handles from `loadDoc`, so it
    cannot retire the initial doc's handle or return a `live` target without it.
  - `clock` is required, not optional, so that no global timer is reachable (an exit criterion). Hosts pass
    `{ setTimeout, clearTimeout }`.
  - Flipping back to the still-bound doc while a swap is pending abandons the swap and fires `done` with equal old and
    new docIds, so the host unfreezes; `NoticeAggregator` ignores such an event.
  - The `done` event's `stranded` is cumulative and is not reset by the event; `takeStranded` resets. A host should feed
    the aggregator from `done` events and use `addStranded` only for edits counted after them.
  - A write during `pending` against the old docId resolves as `retired` (counted as stranded), since that handle is
    about to be retired.
  - Constants: every text in the plan's table is a constant, plus the pieces the aggregator and `movedNotice` need
    (`REPLACED_*`, `STRANDED_EDITS`, `MOVED_*`) and the action labels as `*_ACTIONS` arrays. The six names the GE-1
    contract fixed (`NOTICE_SWITCHED_VERSIONS` and five others) are exported as aliases; `NOTICE_UNAVAILABLE` has no
    text in any plan, so I wrote "The project owner's version of this file is not available yet." Templates use
    `{n}`, `{path}`, and `{name}`, filled by `formatNotice`.
  - `unavailable` and a retried load: a later `indexChanged` to the same docId starts a fresh load (test 6).
