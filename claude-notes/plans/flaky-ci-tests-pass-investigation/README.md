---
title: 'Investigation artifacts: flaky CI tests pass (bd-c72wsugj)'
date: 2026-10-10
description: 'Deterministic repro for the doc-inventory binary-entry race, and how to run it.'
---

Artifacts for `claude-notes/plans/2026-10-10-flaky-ci-tests-pass.md`.

## doc-inventory-race.repro.test.ts

A vitest file that reproduces the intermittent CI failure in
`ts-packages/quarto-sync-client/src/doc-inventory.test.ts` on every run.
It mocks `crypto.subtle.digest` with a 150 ms delay, which stretches the
`await computeSHA256(...)` that `createNewProject` performs between
creating the text file and the binary file past the 100 ms sync throttle
window. Case A then fails with the exact diff seen in CI (the
`binary-file` entry missing); case B shows the same schedule passing when
the creator disconnects with `drainMs`.

Run it from the package directory. It has to live next to the package
sources because it imports `./client.js` and `./test-hub.js`:

```bash
cp claude-notes/plans/flaky-ci-tests-pass-investigation/doc-inventory-race.repro.test.ts \
   ts-packages/quarto-sync-client/src/
cd ts-packages/quarto-sync-client
npx vitest run src/doc-inventory-race.repro.test.ts --reporter=verbose
rm src/doc-inventory-race.repro.test.ts
```

Observed on 2026-10-10 (macOS, Node 24.20, automerge-repo 2.6.0-alpha.5):
five consecutive runs, case A failed and case B passed every time.

The repro graduated to a permanent regression test the same day:
`ts-packages/quarto-sync-client/src/project-creation-delivery.test.ts`
runs the same slow-digest schedule against the fixed helper discipline
(`hub.hubHasHeadsOf(creator)` before the creator disconnects). This copy
stays as the record of the failing shape; it still runs as described
above, and case A still fails on purpose.
