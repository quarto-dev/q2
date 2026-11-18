---
title: 'GE-6: The swap module, the notice aggregator, the moved message'
date: 2026-11-18
description: 'Builds the pure TypeScript swap coordinator that every follower uses when the project owner flips a path to a different doc, with the shared notice wording and the type of the moved presence message.'
status: approved
depends-on: [ge-1]
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
- **G25** the moved message: the type is defined once in the shared package; this plan exports it and the short text
  shown for it.
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

`unavailable` events are not counted in the notice; the host shows the unavailable marker it already has.

The moved message (G25) is a type and a text helper only: `MovedMessage` re-exported from the contract, and
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
   carries the count at that moment.
6. The 5 s timeout fires `unavailable` and leaves the old doc bound; a later `indexChanged` to the same docId retries.
7. The retired cap: the ninth retired handle drops the oldest; none is deleted by the module.
8. Flip back reuses the retired handle without calling `loadDoc`; a flip to the current docId calls nothing.
9. `dispose` aborts in-flight loads and clears timers.
10. `NoticeAggregator`: one file; one file that is not the open file; many files; stranded edits appended; `unavailable`
    ignored; two batches produce two notices; `flush` with nothing returns null.
11. `movedNotice` with and without a name.

## Checklist

- [ ] Confirm GE-1 is complete and read the moved-message shape in its contract.
- [ ] Write the test file `src/swap.test.ts` (items 1 to 9) and `src/notice.test.ts` (items 10 and 11) against the API
  above; run them and see them fail because the module does not exist.
- [ ] Implement `createSwapCoordinator`: per-path state, abort signals, the injected clock, the retired list with its
  cap, and the stranded counter. Make tests 1 to 9 pass one at a time.
- [ ] Implement `NoticeAggregator` and `movedNotice`; tests 10 and 11 pass.
- [ ] Export `MovedMessage` from the contract type and re-export the whole surface from `src/index.ts`.
- [ ] Add the module to the package README: the API, the host contract, and the rule that wording lives here only.
- [ ] Check there is no import of `vscode`, IndexedDB, or Automerge, and no direct use of a global timer
  (`grep` the file; the injected clock is the only way).
- [ ] Update this plan\'s checkboxes and fill in the notes below.

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

Not started.
