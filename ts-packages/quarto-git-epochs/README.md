# @quarto/quarto-git-epochs

Git-aware logic shared by the preview clients and the VS Code extension: the classifier, arrival, startup rule,
registry operations, the git probe, and the gate and quiescence controller. The Rust crate `quarto-git-epochs`
builds the same modules and reads the same fixtures.

**Names come from the contract.** Module, function, field, and action names are those of
`claude-notes/designs/git-epoch-registry-contract.md` and the rows in `fixtures/`. Renaming one is a change to
the contract and to the Rust twin, not to this package.

## Module map

| Module | Entry points | Notes |
|---|---|---|
| `classify` | `classify(input)` | Commit or checkout, from content first and reflog subjects as the tiebreaker (G2) |
| `arrival` | `arrive(input)`, `normalizeText` | The five steps and the retire step, per file; text and binary; both write policies (G9, G14) |
| `startup` | `startupDecision(input)` | The three startup cases and the per-file plan (G10) |
| `registry` | `registryKey`, `canonical`, `nextGeneration`, `register`, `moveForward`, `retire`, `repair`, `entriesFor`, `caseCollisions`, `liveElsewhere`, `createSidecar`, `readIndexFields` | Flat sidecar over `@automerge/automerge` docs (G6, G7, G8, G11, G12) |
| `controller` | `createController(probe, options, clock)` | `check()` is synchronous; `tick()` is async; the clock is injected (G1, G32) |
| `paths` | `toForwardSlashes`, `relativeProjectPath` | Windows-shaped paths on every platform |
| `swap` | `createSwapCoordinator`, `NoticeAggregator` | Follower swap state machine and the batch notice (G26); see "Swap module" below |
| `notices` | `formatNotice` and every notice text constant | The only place notice wording lives (G26) |
| `moved` | `MovedMessage`, `movedNotice` | The moved presence message type and its text (G25) |
| `vocabulary` | `EVENTS`, `SWAP_EVENTS`, `NUMBERS` | Equal to `fixtures/vocabulary.json` |
| `probe-types` | `GitProbe`, `GitUnavailableError`, plain data types | Browser-safe; types only |
| `probe` (node only) | `createGitProbe`, `contentKey` | Import from `@quarto/quarto-git-epochs/probe` |

The package root is browser-safe: no `node:` import, no `vscode` import, no timer outside the injected clock.
Call `probe.ready()` once before `cheapState()`: it resolves the marker and reflog paths with `--git-path`.

## Running the tests

```bash
npm test -w @quarto/quarto-git-epochs                       # everything
npm test -w @quarto/quarto-git-epochs -- scenarios          # real-repo scenario runs only
SCENARIO_TRACE=1 npm test -w @quarto/quarto-git-epochs -- scenarios   # print each step's verdict
npm run typecheck -w @quarto/quarto-git-epochs
npm run build -w @quarto/quarto-git-epochs
```

Each module has a runner that loads its shared decision table from `fixtures/decision-tables/`. The scenario
runner executes `fixtures/scenarios/*.json` with the real `git` in a temp directory and compares the classifier
verdict after every step; the real colocated-jj test is skipped when `jj` is absent.

The golden document this package writes is `fixtures/golden/ge05-registry-ops.bin`. To regenerate it, run the
registry tests with `UPDATE_GOLDEN=1`; the Rust crate decodes it in the cross-implementation check.

## Swap module (`src/swap.ts`)

When the project owner flips `files[path]` to a different doc, every follower swaps: stop writing to the old doc, load
the new one, rebind, and tell the user. `createSwapCoordinator(host, opts)` is a pure state machine with injected I/O.

**Host contract (`SwapHost<H>`)**

- `currentDocId(path)` and `currentHandle(path)`: what is bound now. The handle becomes the retired handle on a swap.
- `loadDoc(docId, signal)`: resolves a handle, rejects if the doc is unavailable, stops when `signal` aborts.
- `rebind(path, newDocId, newHandle, oldHandle?)`: switch the editor or file over (may be async).
- `unconfirmedLocalEdits(path, oldHandle)` (optional): edits made offline, counted as stranded when a swap starts.
- `onReplaced(event)`: phases `pending`, `done`, `unavailable`. The host freezes the editor on `pending`, shows its
  unavailable marker on `unavailable`, and unfreezes on `done`.
- `clock`: the only source of timers. The module uses no global timer and imports nothing from `vscode`, IndexedDB,
  or Automerge.

**Behavior.** One swap at a time per path, and the latest index state wins: a second flip aborts the load in progress
and goes straight to the final docId, with one `pending` event per batch. A load that takes longer than 5 s (or
rejects) reports `unavailable` and leaves the old doc bound. Retired handles are kept, at most 8 per path, and are never
deleted from any repo. A flip back to a retired doc reuses its handle. Writes carry the docId they were made against
(`resolveWriteTarget(path, baseDocId)`): a write that lands on a doc about to be retired, or already retired, resolves
as `retired` and counts as stranded (`takeStranded`).

**Notices.** `NoticeAggregator` folds the `done` events of a batch into one notice. **All notice wording lives in
`src/notices.ts` and nowhere else**: import the constants and fill placeholders with `formatNotice`; never restate a
text. `src/moved.ts` holds the `MovedMessage` type (`{ type: 'moved', peerId }`) and `movedNotice(name?)`.
