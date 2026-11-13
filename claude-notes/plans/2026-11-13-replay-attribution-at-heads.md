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

## Verification

Unit and hook tests cover the run lists and the hook's replay path. The
change was not exercised in a browser against a running hub.
