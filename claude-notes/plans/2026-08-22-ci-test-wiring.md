# Wiring the workspace test suites into CI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every green TypeScript suite in the npm workspace runs on every PR, the two real reds are fixed, and a lint rule stops the wiring from drifting again.

**Architecture:** All new legs go into the existing `ts-test-suite.yml`, after
its WASM + hub-client steps (the WASM build is a prerequisite for
`preview-renderer`'s integration tier). One new step builds the `ts-packages`
`dist/` outputs — three packages resolve their workspace siblings through the
`"import": "./dist/index.js"` export condition and fail cold without it. The
suites then run as explicit per-package steps rather than
`npm test --workspaces`, so ordering is controllable and known-red suites can be
held out; a new `cargo xtask lint` rule makes holding one out a deliberate,
documented act instead of an oversight.

**Tech Stack:** GitHub Actions, npm workspaces, vitest, node:test, Rust (xtask lint).

**Spec:** `claude-notes/research/2026-08-22-ci-test-census.md` — the census this
plan implements. Read it first: it has the measured per-suite numbers, the
reason each suite is currently ungated, and the gap classes this plan does and
does not cover.

**Issue:** [GH #250](https://github.com/quarto-dev/q2/issues/250)

## Global Constraints

- Scope is the census's **gap classes 1–2 only** (wiring + build ordering), plus
  the two reds and the `sync-test-harness` skip. Classes 6–7 (vacuous engine
  passes, Rust doctests, `tree-sitter-doctemplate` corpus, `wasm-qmd-parser`,
  root `typecheck`) are **out of scope** — file strands, do not implement.
- Total added CI time is ~90 s of test wall time plus ~30 s for the
  `ts-packages` build, measured locally. Do not add caching or parallelism to
  optimise this; it is not a problem.
- The workflow matrix is `[ubuntu-latest, macos-latest]`; every new step runs on
  both. Do not add `if: runner.os == ...` guards.
- Never edit a test's expectation to match observed output without proving the
  test still binds — see Task 1 Step 5 for the required revert check.
- `npm ci` / `npm install` is run **from the repo root**, never from a package
  directory.
- Commit at each task boundary. Do not push.

---

### Task 1: Fix the `preview-renderer` Equation `\tag{N}` failure

`ts-packages/preview-renderer` `test:integration` is red on `main`:
`custom-components.integration.test.tsx > Equation > appends \tag{N} to the
LaTeX when plain_data.order is set` fails at `expect(tagEl).not.toBeNull()`.

**The product code is correct.** `Equation.tsx` appends `\tag{N}` and
`Math.tsx` renders it through KaTeX with `displayMode: true`. What changed is
KaTeX: version **0.18** (pinned at `package.json:26`, bumped in `c09586584`)
prefixed its output classes — `tag` → `katex-tag`, `base` → `katex-base`,
`strut` → `katex-strut`. The test still queries `.tag`.

Verify for yourself before changing anything:

```bash
node -e "
const katex=require('katex');
const html=katex.renderToString('a^2 + b^2 = c^2\\\\tag{1}',{displayMode:true,throwOnError:false,output:'html'});
console.log(html.match(/class=\"[^\"]*tag[^\"]*\"/g));
"
# => [ 'class="katex-tag"' ]
```

The `.katex-tag` element's `textContent` is `(1)`, so the second assertion in
the test is already correct.

**There is a second, quieter bug in the same `describe`.** The negative test at
line 676 asserts `expect(span!.querySelector('.tag')).toBeNull()` — that has
been passing **vacuously** since the KaTeX bump, because `.tag` never matches
anything. Both selectors must be fixed, or the negative test keeps proving
nothing.

**Files:**
- Modify: `ts-packages/preview-renderer/src/q2-preview/custom-components.integration.test.tsx:663` and `:676`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: a green `npm run test:integration` in `ts-packages/preview-renderer`,
  which Task 5 depends on.

- [ ] **Step 1: Reproduce the failure**

The WASM package must exist or 26 unrelated files fail on
`Failed to resolve import "wasm-quarto-hub-client"`. If
`crates/wasm-quarto-hub-client/pkg/` is missing, build it first (~10 min):

```bash
cd hub-client && npm run build:wasm && cd ..
```

Then:

```bash
cd ts-packages/preview-renderer
npm run test:integration
```

Expected: `Test Files 1 failed | 49 passed (50)`, `Tests 1 failed | 578 passed | 1 skipped (580)`,
failing on `Equation > appends \tag{N}`.

- [ ] **Step 2: Fix the positive assertion's selector**

At line 663, change the query and the comment that explains it:

```tsx
        // KaTeX renders \tag{N} as a side-floated number. Since KaTeX
        // 0.18 the wrapper class is `katex-tag` (0.17 and earlier used
        // a bare `tag`); the number is split across character spans,
        // so assert on textContent rather than innerHTML.
        const tagEl = span!.querySelector('.katex-tag');
        expect(tagEl).not.toBeNull();
        expect(tagEl!.textContent).toBe('(1)');
```

- [ ] **Step 3: Fix the negative assertion's selector**

At line 676, in `it('does NOT append \\tag when order is missing')`:

```tsx
        expect(span!.querySelector('.katex-tag')).toBeNull();
```

- [ ] **Step 4: Run the suite to verify it passes**

```bash
cd ts-packages/preview-renderer
npm run test:integration
```

Expected: `Test Files 50 passed (50)`, `Tests 579 passed | 1 skipped (580)`.

- [ ] **Step 5: Prove both assertions actually bind (revert check)**

A test fixed by editing its expectation is guilty until proven otherwise. Break
the product code and confirm **both** tests notice.

In `ts-packages/preview-renderer/src/q2-preview/custom/Equation.tsx`, find:

```tsx
        const taggedFirst: InlineNode =
            number !== undefined ? tagInline(first, number) : first;
```

Temporarily replace it with:

```tsx
        const taggedFirst: InlineNode = first;
```

Run `npm run test:integration` again. Expected: the *positive* test now FAILS
(`expected null not to be null`) while the negative test still passes. That
proves the positive assertion binds.

Now restore that line, and instead make the tag unconditional:

```tsx
        const taggedFirst: InlineNode = tagInline(first, number ?? 0);
```

Run again. Expected: the *negative* test now FAILS (`expected <span> to be
null`). That proves the negative assertion binds — the thing it could not do
before this task.

Restore `Equation.tsx` to its original two-line form and re-run to confirm
green:

```bash
git checkout -- src/q2-preview/custom/Equation.tsx
npm run test:integration
```

- [ ] **Step 6: Check for other stale KaTeX class assertions**

```bash
cd /path/to/repo/root
grep -rnE "querySelector\('\.(tag|base|strut|mord)'" \
  --include='*.test.ts*' --include='*.spec.ts' \
  ts-packages/ hub-client/ q2-preview-spa/ trace-viewer/ q2-demos/
```

Expected: no output. (At time of writing the two lines fixed above were the only
hits. If new ones appear, fix them the same way and note them in the commit.)

- [ ] **Step 7: Commit**

```bash
git add ts-packages/preview-renderer/src/q2-preview/custom-components.integration.test.tsx
git commit -m "Fix Equation \\tag test for KaTeX 0.18 class rename (GH #250)

KaTeX 0.18 (pinned in c09586584) prefixed its output classes: tag ->
katex-tag. The positive assertion in custom-components.integration.test.tsx
had been failing since; the negative assertion two lines down had been
passing vacuously, because .tag matches nothing under 0.18.

Both now query .katex-tag. Verified by revert: nulling the \\tag append
reddens the positive test, making it unconditional reddens the negative
one.

This is the failure that made \`cargo xtask verify\` red on main -- it
went unnoticed because ts-packages/preview-renderer runs in no workflow,
which is what GH #250 is about."
```

---

### Task 2: Make `sync-test-harness`'s `ts-sync-server` tier skip when `external-sources/` is absent

`ts-packages/sync-test-harness/src/server-manager.ts:150` builds
`serverDir = path.join(REPO_ROOT, 'external-sources', 'automerge-repo-sync-server')`
and spawns `node src/index.js` there. `external-sources/` is not
version-controlled, so this tier can never run in CI, and depending on it is
prohibited by the External Sources Policy in `CLAUDE.md` ("Test fixtures that
depend on external-sources/"). Locally it currently fails with
`Timeout (30000ms) waiting for ts-sync-server to be ready`.

The sibling `hub` tier spawns our own binary and is fully green (8 tests,
including the three reconnect-delay cases #250 suspected of flake). The decision
(2026-08-22) is to **skip the tier when the directory is absent**, keeping the
local capability for whoever has `external-sources/` checked out.

**Files:**
- Modify: `ts-packages/sync-test-harness/src/server-manager.ts` (export the probe)
- Modify: `ts-packages/sync-test-harness/src/roundtrip.test.ts:112` (`describe` → `describe.skipIf`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `tsSyncServerAvailable(): boolean` exported from
  `./server-manager.js`; a `sync-test-harness` `npm test` that exits 0 without
  `external-sources/`. Task 5 wires that suite into CI.

- [ ] **Step 1: Reproduce the failure**

```bash
cd ts-packages/sync-test-harness
npm test
```

Expected: `Test Files 1 failed | 1 passed (2)`, `Tests 8 passed | 3 skipped (11)`,
with `FAIL src/roundtrip.test.ts > ts-sync-server` /
`Error: Timeout (30000ms) waiting for ts-sync-server to be ready.`

(If you *do* have `external-sources/automerge-repo-sync-server` checked out,
this will pass instead. Rename the directory aside to reproduce.)

- [ ] **Step 2: Export an availability probe from `server-manager.ts`**

Add `existsSync` to the imports at the top of the file (it currently imports
only `mkdtemp, rm` from `node:fs/promises`):

```ts
import { existsSync } from 'node:fs';
```

Then add this export directly above `startTsSyncServer` (i.e. above the
`/** Start the TypeScript automerge-repo-sync-server. */` docblock at ~line
143):

```ts
/**
 * Path to the TypeScript reference sync server. It lives in
 * `external-sources/`, which is NOT version-controlled — see the External
 * Sources Policy in CLAUDE.md. Tests that need it must skip when it is
 * absent rather than fail, so the suite is CI-able (bd-…/GH #250).
 */
const TS_SYNC_SERVER_DIR = path.join(
  REPO_ROOT,
  'external-sources',
  'automerge-repo-sync-server',
);

/**
 * True when the TS reference sync server is checked out locally.
 *
 * Use with `describe.skipIf(!tsSyncServerAvailable())` — never assume it
 * is present. CI never has it.
 */
export function tsSyncServerAvailable(): boolean {
  return existsSync(path.join(TS_SYNC_SERVER_DIR, 'src', 'index.js'));
}
```

Replace the local `serverDir` computation inside `startTsSyncServer` with the
new constant:

```ts
  const proc = spawn('node', ['src/index.js'], {
    cwd: TS_SYNC_SERVER_DIR,
```

(Delete the now-unused `const serverDir = path.join(...)` line.)

- [ ] **Step 3: Skip the tier in `roundtrip.test.ts`**

Extend the existing import block:

```ts
import {
  startHubServer,
  startTsSyncServer,
  tsSyncServerAvailable,
  type ServerHandle,
} from './server-manager.js';
```

Change the `describe` at line 112 and document why:

```ts
// ---------------------------------------------------------------------------
// TS sync server tests (baseline — these should pass)
//
// The reference server lives in external-sources/, which is not
// version-controlled, so this tier can only run on a checkout that has it.
// It is skipped (not failed) elsewhere, including in CI. See GH #250.
// ---------------------------------------------------------------------------

describe.skipIf(!tsSyncServerAvailable())('ts-sync-server', () => {
```

- [ ] **Step 4: Run the suite to verify it passes**

```bash
cd ts-packages/sync-test-harness
npm test
```

Expected: exit 0, `Test Files 1 passed | 1 skipped (2)`, with the `hub` tier's 8
tests passing and the `ts-sync-server` tier reported as skipped.

- [ ] **Step 5: Verify the skip is conditional, not unconditional**

The failure mode to rule out is a probe that always returns `false`, which would
silently disable the tier for developers who *do* have the server. Confirm the
probe reads the filesystem:

```bash
cd ts-packages/sync-test-harness
mkdir -p ../../external-sources/automerge-repo-sync-server/src
echo "console.log('Listening on port ' + process.env.PORT)" \
  > ../../external-sources/automerge-repo-sync-server/src/index.js
npx vitest run src/roundtrip.test.ts 2>&1 | grep -E 'ts-sync-server|Test Files'
```

Expected: the `ts-sync-server` tier is now *attempted* (it will fail against the
stub server — that is fine and expected; we only need to see it stop being
skipped).

Clean up:

```bash
rm -rf ../../external-sources/automerge-repo-sync-server
```

- [ ] **Step 6: Commit**

```bash
git add ts-packages/sync-test-harness/src/server-manager.ts \
        ts-packages/sync-test-harness/src/roundtrip.test.ts
git commit -m "sync-test-harness: skip ts-sync-server tier without external-sources (GH #250)

server-manager.ts spawned the TS reference sync server from
external-sources/automerge-repo-sync-server, which is not
version-controlled -- so the tier could never run in CI, and depending on
it violates the External Sources Policy in CLAUDE.md.

Adds tsSyncServerAvailable() and gates the describe on it, so the suite
exits 0 wherever the server is absent while staying runnable for anyone
who has it checked out. The hub tier (8 tests against our own binary) is
unaffected and green."
```

---

### Task 3: Build the `ts-packages` dists in CI

Three packages fail from a cold `npm ci` because they resolve workspace siblings
through the `"import": "./dist/index.js"` export condition:

| Package | Needs | Cold failure |
| --- | --- | --- |
| `@quarto/quarto-sync-client` | `quarto-automerge-schema` dist | 14 of 21 files: `Failed to resolve entry for package` |
| `@quarto/hub-mcp` | `quarto-sync-client` dist | 13 files + `symlink-invocation.test.ts` (spawns `dist/index.js`) |
| `@quarto/annotated-qmd` | `pandoc-types` dist | `ERR_MODULE_NOT_FOUND` mid-run |

This mirrors `cargo xtask verify` step 6 (`crates/xtask/src/verify.rs:225-274`).
Per `crates/xtask/src/ts_packages.rs`, **build order does not matter** — types
resolve via `src/`, so each package's `tsc` compiles without its dependencies'
`dist/` present. So a plain loop suffices; do not hand-order the list (it would
drift).

**Files:**
- Modify: `.github/workflows/ts-test-suite.yml` (insert after the `Run hub-client tests` step, currently ending at line 164)

**Interfaces:**
- Consumes: the `Install npm dependencies` (`npm ci`) step already in the workflow.
- Produces: `ts-packages/*/dist/` present for all later steps in the job. Tasks 4
  and 5 assume it.

- [ ] **Step 1: Add the build step**

Insert immediately after the `Run hub-client tests` step:

```yaml
      # ── Workspace TS suites (GH #250) ──────────────────────────────────
      #
      # ts-packages dists must exist before the suites below: quarto-sync-client,
      # quarto-hub-mcp and annotated-qmd resolve workspace siblings through the
      # `"import": "./dist/index.js"` export condition, and hub-client bundles
      # these packages from *source*, so nothing above this line builds them.
      # Mirrors `cargo xtask verify` step 6. Build order is irrelevant (types
      # resolve via src/) — see crates/xtask/src/ts_packages.rs.
      - name: Build ts-packages workspaces
        shell: bash
        run: |
          for pkg in ts-packages/*/; do
            npm run build --if-present -w "${pkg%/}"
          done

      # `--help` exits 0 only after the whole ESM graph links, so a missing or
      # stale dependency dist fails here rather than in someone's MCP session.
      - name: Smoke-check quarto-hub-mcp module graph
        shell: bash
        run: node ts-packages/quarto-hub-mcp/dist/index.js --help
```

- [ ] **Step 2: Verify the loop locally**

```bash
rm -rf ts-packages/*/dist
for pkg in ts-packages/*/; do npm run build --if-present -w "${pkg%/}"; done
node ts-packages/quarto-hub-mcp/dist/index.js --help
```

Expected: every build exits 0, and the smoke check prints usage text and exits 0.

- [ ] **Step 3: Verify the step fixes the cold failures**

```bash
cd ts-packages/quarto-sync-client && npm test
```

Expected: `Test Files 21 passed (21)`, `Tests 137 passed (137)` — where the same
command on a dist-less tree failed 14 files.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ts-test-suite.yml
git commit -m "CI: build ts-packages dists + smoke-check the MCP graph (GH #250)

quarto-sync-client, quarto-hub-mcp and annotated-qmd resolve workspace
siblings through the \"import\": \"./dist/index.js\" export condition, and
nothing in CI built those dists -- hub-client bundles these packages from
source. Cold, that is 27 failing test files across two packages, which is
the \"couple of deps not installed\" #250 guessed at: a build-order
requirement, not flake.

Mirrors cargo xtask verify step 6, including the node dist/index.js
--help ESM-link smoke check."
```

---

### Task 4: Wire the eight green suites into CI

These pass as-is at `e3b3d7d4a` — no fixes needed, only steps. Measured counts
and durations from the census:

| Package | Command | Tests | Time |
| --- | --- | --- | --- |
| `ts-packages/preview-renderer` | `npm test` | 549 (+36 skip) | 4.8 s |
| `ts-packages/quarto-api` | `npm test` | 368 (+1 skip) | 1.4 s |
| `ts-packages/preview-runtime` | `npm test` | 77 | 0.7 s |
| `ts-packages/quarto-automerge-schema` | `npm test` | 36 | 0.3 s |
| `ts-packages/wasm-js-bridge` | `npm test` | 19 | 10.8 s |
| `q2-preview-spa` | `npm test` + `npm run test:integration` | 46 + 76 | 9.7 s |
| `q2-demos/kanban` | `npm test` + `npm run test:integration` | 35 + 20 | 5.3 s |
| `trace-viewer` | `npm test` | 10 | 2.3 s |

Two deliberate choices:

1. **Run `npm test` / `npm run test:integration` explicitly, not `test:ci`.**
   `q2-demos/kanban`'s `test:ci` chains a `test:wasm` leg whose glob
   (`src/**/*.wasm.test.ts`) matches **zero files**; `preview-runtime`'s
   `test:wasm` points at a `vitest.wasm.config.ts` that **does not exist**.
   Those dead scripts are out of scope here (census class 5) — avoid them rather
   than fix them.
2. **Use `npm test -w <path>` from the repo root**, matching the
   `engine-host-deno` step already in the workflow.

**Files:**
- Modify: `.github/workflows/ts-test-suite.yml` (after Task 3's steps)

**Interfaces:**
- Consumes: Task 3's `ts-packages` dists.
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Add the steps**

```yaml
      # Suites that are green as-is (GH #250 census, 2026-08-22). Explicit
      # `npm test` rather than each package's `test:ci`: kanban's test:ci chains
      # a test:wasm leg whose glob matches zero files, and preview-runtime's
      # test:wasm names a config file that does not exist.
      - name: Run preview-renderer unit tests
        shell: bash
        run: npm test -w ts-packages/preview-renderer

      - name: Run preview-runtime unit tests
        shell: bash
        run: npm test -w ts-packages/preview-runtime

      - name: Run quarto-api tests
        shell: bash
        run: npm test -w ts-packages/quarto-api

      - name: Run quarto-automerge-schema tests
        shell: bash
        run: npm test -w ts-packages/quarto-automerge-schema

      - name: Run wasm-js-bridge tests
        shell: bash
        run: npm test -w ts-packages/wasm-js-bridge

      - name: Run q2-preview-spa tests
        shell: bash
        run: |
          npm test -w q2-preview-spa
          npm run test:integration -w q2-preview-spa

      - name: Run kanban demo tests
        shell: bash
        run: |
          npm test -w q2-demos/kanban
          npm run test:integration -w q2-demos/kanban

      - name: Run trace-viewer tests
        shell: bash
        run: npm test -w trace-viewer
```

- [ ] **Step 2: Verify every command works from the repo root**

The `-w <path>` form must resolve for each. Run the whole set:

```bash
npm test -w ts-packages/preview-renderer && \
npm test -w ts-packages/preview-runtime && \
npm test -w ts-packages/quarto-api && \
npm test -w ts-packages/quarto-automerge-schema && \
npm test -w ts-packages/wasm-js-bridge && \
npm test -w q2-preview-spa && \
npm run test:integration -w q2-preview-spa && \
npm test -w q2-demos/kanban && \
npm run test:integration -w q2-demos/kanban && \
npm test -w trace-viewer && echo ALL GREEN
```

Expected: `ALL GREEN`, having reported 549, 77, 368, 36, 19, 46, 76, 35, 20 and
10 passing tests respectively. If a `-w` path fails to resolve, check it against
the `workspaces` array in the root `package.json` (`ts-packages/*`, `hub-client`,
`trace-viewer`, `q2-preview-spa`, `q2-demos/*`).

- [ ] **Step 3: Validate the workflow file parses**

```bash
python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ts-test-suite.yml')); print('YAML ok')"
```

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ts-test-suite.yml
git commit -m "CI: run the eight green workspace TS suites (GH #250)

preview-renderer (549), quarto-api (368), preview-runtime (77),
quarto-automerge-schema (36), wasm-js-bridge (19), q2-preview-spa
(46+76), kanban (35+20) and trace-viewer (10) all pass as-is and ran in
no workflow. ~1,200 assertions, ~36s of wall time.

Explicit npm test rather than each package's test:ci: kanban's test:ci
chains a test:wasm leg matching zero files, and preview-runtime's
test:wasm names a nonexistent config. Those dead scripts are census
class 5, out of scope here."
```

---

### Task 5: Wire the suites that need the build/WASM ordering

Three more suites are green **given** earlier steps in the job, so they must come
after them:

| Package | Command | Tests | Depends on |
| --- | --- | --- | --- |
| `ts-packages/quarto-sync-client` | `npm test` | 137 | Task 3's dists |
| `ts-packages/quarto-hub-mcp` | `npm test` | 246 (+3 skip) | Task 3's dists |
| `ts-packages/preview-renderer` | `npm run test:integration` | 578 (+1 skip) | Task 1's fix **and** the workflow's existing `Build WASM module` step |
| `ts-packages/sync-test-harness` | `npm test` | 8 (+3 skip) | Task 2's skip |

**Files:**
- Modify: `.github/workflows/ts-test-suite.yml` (after Task 4's steps)

**Interfaces:**
- Consumes: Task 1 (`\tag` fix), Task 2 (`tsSyncServerAvailable`), Task 3 (dists).
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Add the steps**

```yaml
      # preview-renderer's integration tier imports wasm-quarto-hub-client;
      # without it 26 of 50 files fail on module resolution. The `Build WASM
      # module` step above satisfies that, so this must stay below it.
      - name: Run preview-renderer integration tests
        shell: bash
        run: npm run test:integration -w ts-packages/preview-renderer

      # These two resolve workspace siblings through dist/ — they need the
      # "Build ts-packages workspaces" step above.
      - name: Run quarto-sync-client tests
        shell: bash
        run: npm test -w ts-packages/quarto-sync-client

      - name: Run quarto-hub-mcp tests
        shell: bash
        run: npm test -w ts-packages/quarto-hub-mcp

      # The hub tier spawns our own binary. The ts-sync-server tier skips
      # itself here: it needs external-sources/, which CI never has.
      - name: Run sync-test-harness tests
        shell: bash
        run: npm test -w ts-packages/sync-test-harness
```

- [ ] **Step 2: Verify locally, in order**

```bash
npm run test:integration -w ts-packages/preview-renderer && \
npm test -w ts-packages/quarto-sync-client && \
npm test -w ts-packages/quarto-hub-mcp && \
npm test -w ts-packages/sync-test-harness && echo ALL GREEN
```

Expected: `ALL GREEN`, reporting 578+1skip, 137, 246+3skip, and 8+3skip.

- [ ] **Step 3: Confirm the ordering constraint is real, not cargo-culted**

Prove that `preview-renderer test:integration` genuinely needs the WASM step, so
a future editor doesn't reorder the steps:

```bash
mv crates/wasm-quarto-hub-client/pkg /tmp/pkg-stash
npm run test:integration -w ts-packages/preview-renderer 2>&1 | grep -c 'Failed to resolve import "wasm-quarto-hub-client"'
mv /tmp/pkg-stash crates/wasm-quarto-hub-client/pkg
```

Expected: a non-zero count (26 file-level failures at time of writing).

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ts-test-suite.yml
git commit -m "CI: run the build-ordered workspace TS suites (GH #250)

preview-renderer integration (578), quarto-hub-mcp (246),
quarto-sync-client (137) and sync-test-harness (8). Each is green only
given an earlier step in the job -- the WASM build for
preview-renderer's integration tier, the ts-packages dists for the other
two -- which is why they are ordered after Tasks 3 and 4 rather than
folded in with the standalone suites."
```

---

### Task 6: Lint rule — a workspace package with tests must be wired or explicitly excused

#250's own history is the argument for this task: *"hub-client got wired up and
the rest hasn't been added since."* The wiring drifted for months because
nothing reconciled "packages that have tests" against "packages CI runs". Tasks
3–5 fix today's drift; this task stops tomorrow's.

The rule follows the existing repo-level pattern (`error_docs`,
`error_docs_sidebar` in `crates/xtask/src/lint/`): reconcile two trees, run once
per lint invocation rather than per Rust file, anchor violations at a real line.

**Files:**
- Create: `crates/xtask/src/lint/ci_test_wiring.rs`
- Modify: `crates/xtask/src/lint/mod.rs` (declare the module; call it in `run_check`)
- Modify: `CLAUDE.md` (document the rule under "Current Lint Rules")

**Interfaces:**
- Consumes: `Violation` from `crate::lint` (fields: `file: PathBuf`,
  `line: usize`, `column: usize`, `rule: &'static str`, `message: String`,
  `suggestion: Option<String>`).
- Produces: `pub fn check(workspace_root: &Path) -> Result<Vec<Violation>>`,
  called from `lint::run_check`.

- [ ] **Step 1: Write the failing test**

Create `crates/xtask/src/lint/ci_test_wiring.rs` containing only the test module
for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a fake repo: root package.json with `workspaces`, one package
    /// per (path, name, has_test), and a workflow file with `workflow_body`.
    fn scaffold(
        globs: &[&str],
        packages: &[(&str, &str, bool)],
        workflow_body: &str,
    ) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let globs_json = globs
            .iter()
            .map(|g| format!("\"{}\"", g))
            .collect::<Vec<_>>()
            .join(",");
        fs::write(
            root.join("package.json"),
            format!("{{\"workspaces\":[{}]}}", globs_json),
        )
        .unwrap();

        for (path, name, has_test) in packages {
            let dir = root.join(path);
            fs::create_dir_all(&dir).unwrap();
            let scripts = if *has_test {
                "\"scripts\":{\"test\":\"vitest run\"}"
            } else {
                "\"scripts\":{\"build\":\"tsc\"}"
            };
            fs::write(
                dir.join("package.json"),
                format!("{{\"name\":\"{}\",{}}}", name, scripts),
            )
            .unwrap();
        }

        let wf_dir = root.join(".github/workflows");
        fs::create_dir_all(&wf_dir).unwrap();
        fs::write(wf_dir.join("ts-test-suite.yml"), workflow_body).unwrap();

        tmp
    }

    #[test]
    fn flags_a_tested_package_absent_from_the_workflow() {
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/lonely", "@quarto/lonely", true)],
            "jobs:\n  test-suite:\n    steps: []\n",
        );
        let violations = check(tmp.path()).unwrap();
        assert_eq!(violations.len(), 1, "{:?}", violations);
        assert_eq!(violations[0].rule, "ci-test-suite-unwired");
        assert!(
            violations[0].message.contains("@quarto/lonely"),
            "{}",
            violations[0].message
        );
        // Anchored at the package.json line declaring the test script.
        assert!(violations[0].file.ends_with("ts-packages/lonely/package.json"));
        assert_eq!(violations[0].line, 1);
    }

    #[test]
    fn accepts_a_package_named_in_the_workflow() {
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/wired", "@quarto/wired", true)],
            "jobs:\n  steps:\n    - run: npm test -w ts-packages/wired\n",
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn ignores_packages_without_a_test_script() {
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/typesonly", "@quarto/types", false)],
            "jobs:\n  steps: []\n",
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn ignores_excused_packages() {
        // EXCUSED entries are matched by npm package name.
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/annotated-qmd", EXCUSED[0].0, true)],
            "jobs:\n  steps: []\n",
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn expands_literal_and_glob_workspace_entries() {
        let tmp = scaffold(
            &["ts-packages/*", "trace-viewer"],
            &[
                ("ts-packages/a", "@quarto/a", true),
                ("trace-viewer", "@quarto/trace-viewer", true),
            ],
            "jobs:\n  steps:\n    - run: npm test -w ts-packages/a\n",
        );
        let violations = check(tmp.path()).unwrap();
        assert_eq!(violations.len(), 1, "{:?}", violations);
        assert!(violations[0].message.contains("@quarto/trace-viewer"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo nextest run -p xtask -E 'test(ci_test_wiring)'
```

Expected: FAIL — the module has no `check` function, `EXCUSED`, or imports yet
(compile error `cannot find function 'check' in this scope`).

- [ ] **Step 3: Write the implementation**

Prepend to `crates/xtask/src/lint/ci_test_wiring.rs`, above the test module:

```rust
//! Lint: every npm-workspace package with a `test` script must be run by
//! `.github/workflows/ts-test-suite.yml`, or be listed in `EXCUSED` with a
//! reason.
//!
//! Why this exists (GH #250, 2026-08-22): CI ran `hub-client`'s tests and
//! `engine-host-deno`'s, and nothing else. Roughly 2,000 assertions across a
//! dozen packages sat outside the merge gate for months — long enough for a
//! KaTeX class rename to redden `preview-renderer` on `main` unnoticed. The
//! wiring drifted because nothing reconciled "packages that have tests"
//! against "packages CI runs". This rule is that reconciliation.
//!
//! Like `error_docs` and `error_docs_sidebar`, this is a *repo-level* rule: it
//! compares two trees rather than grepping one Rust file, so it runs once per
//! lint invocation.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::Violation;

const RULE: &str = "ci-test-suite-unwired";

/// The workflow that must reference every tested package.
const WORKFLOW: &str = ".github/workflows/ts-test-suite.yml";

/// Packages deliberately not gated, with the reason. Keep this list short and
/// always name the tracking strand or issue — an entry here is a promise to
/// come back, not a place to park work.
pub(crate) const EXCUSED: &[(&str, &str)] = &[(
    "@quarto/annotated-qmd",
    "2 of 156 tests fail on a source-tracking off-by-one; tracked by bd-1d6io. \
     Wire it in once that strand closes.",
)];

/// Expand the root `package.json` `workspaces` globs into package directories.
///
/// Only the trailing-`*` form npm actually uses here is supported
/// (`ts-packages/*`, `q2-demos/*`); anything else is treated as a literal path.
fn workspace_dirs(root: &Path, globs: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for glob in globs {
        if let Some(prefix) = glob.strip_suffix("/*") {
            let parent = root.join(prefix);
            let Ok(entries) = std::fs::read_dir(&parent) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.join("package.json").is_file() {
                    dirs.push(path);
                }
            }
        } else {
            let path = root.join(glob);
            if path.join("package.json").is_file() {
                dirs.push(path);
            }
        }
    }
    dirs.sort();
    dirs
}

/// 1-indexed line of the `"test":` key in a package.json, or 1 if not found.
fn test_script_line(manifest: &str) -> usize {
    manifest
        .lines()
        .position(|line| line.contains("\"test\":"))
        .map(|i| i + 1)
        .unwrap_or(1)
}

pub fn check(workspace_root: &Path) -> Result<Vec<Violation>> {
    let root_manifest_path = workspace_root.join("package.json");
    let Ok(root_manifest) = std::fs::read_to_string(&root_manifest_path) else {
        // No npm workspace here (e.g. a Rust-only checkout) — nothing to check.
        return Ok(Vec::new());
    };
    let root: serde_json::Value = serde_json::from_str(&root_manifest)
        .with_context(|| format!("Failed to parse {}", root_manifest_path.display()))?;

    let globs: Vec<String> = root
        .get("workspaces")
        .and_then(|w| w.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let workflow_path = workspace_root.join(WORKFLOW);
    let Ok(workflow) = std::fs::read_to_string(&workflow_path) else {
        return Ok(Vec::new());
    };

    let mut violations = Vec::new();

    for dir in workspace_dirs(workspace_root, &globs) {
        let manifest_path = dir.join("package.json");
        let manifest = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let pkg: serde_json::Value = serde_json::from_str(&manifest)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

        let has_test = pkg
            .get("scripts")
            .and_then(|s| s.get("test"))
            .and_then(|t| t.as_str())
            .is_some_and(|t| !t.trim().is_empty());
        if !has_test {
            continue;
        }

        let name = pkg.get("name").and_then(|n| n.as_str()).unwrap_or_default();
        if EXCUSED.iter().any(|(excused, _)| *excused == name) {
            continue;
        }

        // The workspace path as npm's `-w` flag spells it, e.g.
        // `ts-packages/quarto-api`.
        let rel = dir
            .strip_prefix(workspace_root)
            .unwrap_or(&dir)
            .to_string_lossy()
            .replace('\\', "/");

        if workflow.contains(&rel) || (!name.is_empty() && workflow.contains(name)) {
            continue;
        }

        violations.push(Violation {
            file: manifest_path.clone(),
            line: test_script_line(&manifest),
            column: 1,
            rule: RULE,
            message: format!(
                "{} has a `test` script but is never run by {} — its tests are \
                 not gated by CI",
                if name.is_empty() { &rel } else { name },
                WORKFLOW
            ),
            suggestion: Some(format!(
                "Add a step running `npm test -w {}` to {}, or add the package \
                 to EXCUSED in crates/xtask/src/lint/ci_test_wiring.rs with a \
                 reason and a tracking strand.",
                rel, WORKFLOW
            )),
        });
    }

    Ok(violations)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo nextest run -p xtask -E 'test(ci_test_wiring)'
```

Expected: 5 tests pass.

- [ ] **Step 5: Wire the rule into the lint runner**

In `crates/xtask/src/lint/mod.rs`, add the module declaration alphabetically
among the existing ones (`add_file_with_id`, `error_docs`, …):

```rust
mod ci_test_wiring;
```

Then, in `run_check`, after the `error_docs_sidebar` block:

```rust
    if config.verbose {
        eprintln!("Checking npm-workspace test scripts against the TS test workflow");
    }
    all_violations.extend(ci_test_wiring::check(&workspace_root)?);
```

- [ ] **Step 6: Run the real lint and confirm it is clean**

```bash
cargo xtask lint
```

Expected: `All checks passed!`. If it flags a package, that package genuinely
isn't wired — either add its step (it belongs in Task 4 or 5) or add it to
`EXCUSED` with a reason. Do not weaken the rule to make it pass.

- [ ] **Step 7: Confirm the rule bites (revert check)**

```bash
# Temporarily remove one wired suite from the workflow.
python3 - <<'PY'
p='.github/workflows/ts-test-suite.yml'
s=open(p).read()
s=s.replace("        run: npm test -w ts-packages/quarto-api\n","        run: true\n")
open(p,'w').write(s)
PY
cargo xtask lint 2>&1 | grep ci-test-suite-unwired
git checkout -- .github/workflows/ts-test-suite.yml
```

Expected: the grep prints a violation naming `@quarto/api`. If it prints
nothing, the rule is inert — fix it before continuing.

- [ ] **Step 8: Document the rule in `CLAUDE.md`**

Add to the "Current Lint Rules" list, after the `metadata-as-str` entry:

```markdown
- **ci-test-suite-unwired**: Every npm-workspace package with a `test` script must be referenced by `.github/workflows/ts-test-suite.yml`, or be listed in `EXCUSED` in `crates/xtask/src/lint/ci_test_wiring.rs` with a reason and a tracking strand. CI ran only `hub-client` and `engine-host-deno` for months while ~2,000 assertions across a dozen packages sat outside the merge gate — long enough for a KaTeX class rename to redden `preview-renderer` on `main` unnoticed (GH #250). Nothing reconciled "packages that have tests" against "packages CI runs"; this rule is that reconciliation. Like the `error-docs-*` rules it is *repo-level* — it compares two trees rather than grepping one Rust file — and anchors violations at the offending `package.json`'s `"test":` line. **When you add a `test` script to a workspace package, add its CI step in the same commit.**
```

- [ ] **Step 9: Commit**

```bash
git add crates/xtask/src/lint/ci_test_wiring.rs crates/xtask/src/lint/mod.rs CLAUDE.md
git commit -m "xtask lint: flag workspace packages whose tests CI never runs (GH #250)

New repo-level rule ci-test-suite-unwired. Every npm-workspace package
with a test script must appear in ts-test-suite.yml or be listed in
EXCUSED with a reason and a strand.

Tasks 3-5 fixed today's drift; this stops tomorrow's. #250 exists
because \"hub-client got wired up and the rest hasn't been added
since\" -- the drift, not any individual suite, is the bug.

annotated-qmd is the only EXCUSED entry, pending bd-1d6io."
```

---

### Task 7: Wire `annotated-qmd` once bd-1d6io lands (BLOCKED — do not start)

`ts-packages/annotated-qmd` is 154/156. The two failures are
`div-attrs.json - Div with attributes conversion` and
`substring invariant - links.qmd: inline code` (a one-byte-early start offset
that captures the preceding space: got `' \`x = 5\`'`, expected `'\`x = 5\`'`).

**This is bd-1d6io, and it is being worked right now** — `.worktrees/workspace-2`
is on `braid/bd-1d6io-annotated-qmd-source-tracking` with a commit *"Tighten
attribute key source ranges, and guard the annotated-qmd fixtures"* plus
uncommitted changes in `crates/pampa/src/writers/incremental.rs`. **Do not fix it
here** — you would duplicate or conflict with that work.

Do this task only after bd-1d6io closes and its fix is on `main`:

- [ ] **Step 1: Confirm the suite is green**

```bash
npm run build --if-present -w ts-packages/pandoc-types
npm test -w ts-packages/annotated-qmd
```

Expected: `# tests 156`, `# pass 156`, `# fail 0`.

- [ ] **Step 2: Add the CI step**

In `.github/workflows/ts-test-suite.yml`, alongside the Task 5 steps:

```yaml
      # node:test rather than vitest; needs @quarto/pandoc-types' dist, built
      # by the "Build ts-packages workspaces" step above.
      - name: Run annotated-qmd tests
        shell: bash
        run: npm test -w ts-packages/annotated-qmd
```

- [ ] **Step 3: Remove the lint excuse**

In `crates/xtask/src/lint/ci_test_wiring.rs`, delete the `@quarto/annotated-qmd`
entry from `EXCUSED`, leaving an empty slice:

```rust
pub(crate) const EXCUSED: &[(&str, &str)] = &[];
```

The `ignores_excused_packages` test indexes `EXCUSED[0]`, so it must change too.
Replace that test with one that uses a locally-declared excuse list — or, if
`EXCUSED` is empty, assert the empty-list behaviour directly:

```rust
    #[test]
    fn excused_list_is_matched_by_package_name() {
        // With EXCUSED empty, every tested package must be wired.
        assert!(
            EXCUSED.is_empty(),
            "update this test if an excuse is added: {:?}",
            EXCUSED
        );
    }
```

- [ ] **Step 4: Verify**

```bash
cargo nextest run -p xtask -E 'test(ci_test_wiring)'
cargo xtask lint
```

Expected: tests pass, lint clean.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ts-test-suite.yml crates/xtask/src/lint/ci_test_wiring.rs
git commit -m "CI: run annotated-qmd tests, drop its lint excuse (GH #250, bd-1d6io)"
```

---

## Wrap-up

- [ ] **Reconcile this checklist against reality.** Re-read the plan, verify each
  `- [ ]`/`- [x]` against what actually landed, correct any that are wrong, and
  commit the updated plan file.

- [ ] **Run the full gate before proposing a push.**

```bash
cargo xtask verify --skip-hub-build --skip-hub-tests   # Rust + lint legs
cargo nextest run --workspace                          # ~3 min; report the delta
```

`cargo xtask verify`'s preview-renderer leg was **red on `main`** before Task 1.
After Task 1 it should be green — if it isn't, something in Task 1 regressed.

- [ ] **File strands for the out-of-scope census findings** (class 5–7). These
  are genuinely outside this plan, so they belong in braid, one each:
  - Rust doctests run nowhere and are red — 5 failures, ~70 compile errors from
    untagged prose blocks, plus a stale `quarto-sass` doctest calling a two-arg
    `ThemeContext::new` with one argument.
  - ~80 silent-skip sites let engine tests (jupyter, knitr/R, julia, uv,
    dart-sass) pass vacuously in CI; extend the `QUARTO_CI=1` hard-fail pattern
    beyond deno, or install the engines.
  - `tree-sitter-doctemplate`'s 215-line corpus runs in neither CI nor `verify`.
  - `wasm-qmd-parser`'s 4 tests never build (workspace-excluded).
  - `npm run typecheck --workspaces` runs in no workflow.
  - Dead test scripts: `preview-runtime` `test:wasm` (nonexistent config) and
    `test:integration` (zero files), `kanban` `test:wasm` (zero files),
    `editors/vscode-quarto-rust` `test` (no test sources, outside the workspace).
  - `q2-preview-spa`'s 17 Playwright specs run only behind `verify --e2e`.
