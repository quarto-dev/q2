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
