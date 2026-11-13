---
title: "Replay: attribution at the replay step's heads"
---

Strand: bd-bay844no

## Overview

With the authors overlay on, replay showed the text of the replay step
coloured by attribution runs built for the live document's heads. The run
offsets belong to the final text, so on earlier text they land on the wrong
words, and they shift from step to step.

The fix builds the runs for the same point in history that the text comes
from. A replay step is one change hash; its document state is that change
plus all of its ancestors.

## Checklist

- [x] Repro: head runs over historical text misattribute words (throwaway test)
- [x] `attribution-runs.ts`: replay up to given heads (`ReplayTarget`)
  - [x] tests first: runs at every step equal a build of the doc forked there
  - [x] forward step applies only the new changes
  - [x] a target that is not ahead of the work doc asks for a rebuild
- [x] `ReplaySession.getHeadsAt(index)` and `ReplayState.currentHeads`
- [x] `useAttribution`: `atHeads` option, payload tied to the heads it was built for
- [x] `ReactPreview`: hold the render while the step's attribution is pending
- [x] Thread heads `Editor` → `PreviewRouter` → `ReactPreview`
- [x] `npm run build:all`, hub-client tests
- [ ] Browser check of a multi-author replay (not done; see Verification)

## Details

**Which changes to apply.** Walk the dependency graph down from the target
hash and stop at changes the work doc already has. The changes visited are
the ones to apply, in topological order. If a head of the work doc is not
reached by the walk, the work doc holds a change that the target does not
(the user moved backwards, or across a concurrent branch); the forward-only
work doc cannot be used, and the caller rebuilds from an empty doc.

**Costs.** A forward step (playback, step forward) applies only the new
changes. A backward seek to a step not seen before is a full, time-sliced
rebuild up to that step. The hook keeps the runs of recently shown steps, so
stepping back over them is immediate.

**No stale frame.** In replay the hook returns a payload only when it was
built for the current heads, and reports `pending` otherwise. `ReactPreview`
skips its render while `pending`, so a step is drawn once, with its own
attribution, instead of first with the previous step's runs.

**Change index.** Change metadata and bytes are read from the live doc once
per replay (`ReplayTarget.index`) so a step does not rescan history. The hook
drops the index when replay ends.

## Follow-up: the same mismatch outside replay

Two more places paired a text with runs built for another text. Both are
fixed on this branch.

- [x] bd-1ga6itse: live edits. The payload was refreshed only after 500 ms
  without a text change, while the preview re-rendered at once; with
  collaborators typing it never refreshed. The hook now folds each edit into
  the runs in the effects of the render that brought the text, with no
  debounce, and the `pending` hold covers that render.
- [x] bd-1ewams7j: local branches. The preview showed the branch text with
  runs from the synced main doc. The hook now takes `branchId` and reads the
  branch doc (`branchService.getBranchDoc`).
- [x] Oracle test: under concurrent edits to one sentence, the runs name for
  each character the actor of the op that inserted it.

The hook's result now records the text it was settled for, and counts only
for that text. If the history's text and the text shown differ in length
(React state a change behind the doc), the text is drawn without the overlay
instead of with shifted runs.

## Verification

Unit and hook tests cover the run lists and the hook's replay path.

Browser, local hub (2026-11-13): live typing with the authors overlay on was
checked by hand. A multi-author replay and a local branch were not.

## Follow-up: cost of the synchronous live update

With the debounce gone, every text change applies its change to the work doc.
One `applyChanges` call costs time proportional to the document (measured:
2.2 ms at 10k characters, 8.8 ms at 40k, 35 ms at 160k), and a typing burst
no longer shares one call.

- [x] Live update reads the doc's own patches (`A.diff` between the heads the
  runs describe and the live heads) when all new changes share one
  attribution. Automerge keeps the patches of a doc's latest change, so an
  edit costs 0.04 to 0.06 ms at the same sizes. The work doc stays behind
  (`_heads` records where the runs are) and is caught up, in one call, before
  anything replays on it: changes of several authors in one update, or a
  replay step.
- [x] `ReactPreview` passes one shared object when it has no identities.
- [x] The hook reads the runs during render instead of setting state from an
  effect. The run-list state moved to `AttributionStore`
  (`services/attribution-store.ts`), read with `useSyncExternalStore`. A live
  edit, a forward replay step and a cached replay step are each one render
  with their own payload; `pending` now covers only the cold build of a
  replay seek. A read never starts a build: it records the target, and
  `reconcile()` starts, replaces or drops the build after the commit.
- [ ] Not done: the cold build has the same per-call cost, so a history whose
  changes cannot be batched is quadratic (146 s for 8000 changes at 160k
  characters in the benchmark's worst case). Same on `main`.
