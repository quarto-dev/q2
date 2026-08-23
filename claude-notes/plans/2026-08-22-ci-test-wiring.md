# Wiring the workspace test suites into CI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every green TypeScript suite in the npm workspace runs on every PR, the two real reds are fixed, and a lint rule stops the wiring from drifting again.

**Architecture:** Most new legs are appended to `ts-test-suite.yml`. One new step
there builds the `ts-packages` `dist/` outputs — three packages resolve their
workspace siblings through the `"import": "./dist/index.js"` export condition and
fail cold without it. `sync-test-harness` is the exception: its `hub` tier shells
out to `cargo run --bin hub`, so it goes into `hub-client-e2e.yml`, which already
pre-builds that binary. Suites run as explicit per-package steps rather than
`npm test --workspaces`, so known-red suites can be held out; a new
`cargo xtask lint` rule makes holding one out a deliberate, documented act
instead of an oversight.

**Tech Stack:** GitHub Actions, npm workspaces, vitest, node:test, Rust (xtask lint).

**Spec:** `claude-notes/research/2026-08-22-ci-test-census.md` — the census this
plan implements. Read it first: it has the measured per-suite numbers, the
reason each suite is currently ungated, and the gap classes this plan does and
does not cover.

**Issue:** [GH #250](https://github.com/quarto-dev/q2/issues/250)

## Global Constraints

- **Working directory: the worktree root**, for every command in this plan
  unless the step itself says otherwise. Paths in `git add` lines are
  root-relative. `npm ci` / `npm install` must *only* ever run from the root
  (repo rule). Running `npm run <script>` from inside a package directory is
  fine and some steps do it — that rule is about installs, not scripts.
- Scope is the census's **gap classes 1–2** (wiring + build ordering), plus the
  two reds and the `sync-test-harness` skip. Classes **5–7** (dead scripts,
  vacuous engine passes, Rust doctests, `tree-sitter-doctemplate` corpus,
  `wasm-qmd-parser`, root `typecheck`, `q2-preview-spa` e2e) are **out of
  scope** — file strands in the Wrap-up, do not implement.
- **Where the new steps go:** append them at the **end** of
  `ts-test-suite.yml`'s `test-suite` job, after the existing
  `engine-host-deno` steps (currently ending at line 206). Functionally any
  position below `Build WASM module` (line 154) works, but appending keeps the
  new block contiguous under one banner comment instead of splitting the
  existing engine-host-deno group.
- **CI time is measured for Task 4 (~35 s total) and unmeasured for Task 5.**
  `quarto-hub-mcp` is the known-heavy one (~32 s locally; its
  `bundle.test.ts` runs the esbuild bundler in `beforeAll`). Measure the real
  job before deciding anything is too slow — and do not add caching or
  parallelism speculatively.
- The `ts-test-suite.yml` matrix is `[ubuntu-latest, macos-latest]`; every step
  added there runs on both. Do not add `if: runner.os == ...` guards.
  `hub-client-e2e.yml` is ubuntu-only; Task 5b inherits that.
- Never edit a test's expectation to match observed output without proving the
  test still binds — see Task 1 Step 5 for the required revert check.
- Commit at each task boundary. Do not push.

---

### Task 1: Fix the `preview-renderer` Equation `\tag{N}` failure

`ts-packages/preview-renderer` `test:integration` is red on `main`:
`custom-components.integration.test.tsx > Equation > appends \tag{N} to the
LaTeX when plain_data.order is set` fails at `expect(tagEl).not.toBeNull()`.

**The product code is correct.** `Equation.tsx` appends `\tag{N}` and
`Math.tsx` renders it through KaTeX with `displayMode: true`. What changed is
KaTeX, which prefixed its output classes at 0.18 — `tag` → `katex-tag`,
`base` → `katex-base`, `strut` → `katex-strut`. The test still queries `.tag`.

The upgrade history, so you can date the breakage:

| Commit | Date | What |
| --- | --- | --- |
| `d93197033` | 2026-08-08 | `upgrade katex from 0.17.0 to 0.18.0` — **this is where the test went red** |
| `c09586584` | 2026-08-20 | `bump remaining katex pins to 0.18.0` |
| `669ad7534` | 2026-08-21 | Snyk, `0.18.0` → `0.18.1` (PR #571); current pin at `package.json:26` |

So the *suite* has been ungated for months, but this particular red is about
two weeks old.

Verify for yourself before changing anything:

```bash
node -e "
const katex=require('katex');
const html=katex.renderToString('a^2 + b^2 = c^2\\\\tag{1}',{displayMode:true,throwOnError:false,output:'html'});
console.log(html.match(/class=\"[^\"]*tag[^\"]*\"/g));
"
# => [ 'class="katex-tag"' ]
```

The `.katex-tag` element's `textContent` is `(1)`, so the *second* assertion in
the test is already correct — only the selector is stale.

**There is a second, quieter bug in the same `describe`.** The negative test at
line 676 asserts `expect(span!.querySelector('.tag')).toBeNull()` — that has
been passing **vacuously** since the bump, because `.tag` never matches
anything. Both selectors must be fixed, or the negative test keeps proving
nothing.

**Files:**
- Modify: `ts-packages/preview-renderer/src/q2-preview/custom-components.integration.test.tsx` — lines 659–665 and 674–676

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: a green `npm run test:integration` in `ts-packages/preview-renderer`,
  which Task 5 depends on.

- [ ] **Step 1: Reproduce the failure**

The WASM package must exist or 26 unrelated files fail on
`Failed to resolve import "wasm-quarto-hub-client"`.
`vitest.integration.config.ts:53-55` aliases that specifier to
`hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client.js`, and
`hub-client/wasm-quarto-hub-client` is a **committed symlink** to
`../crates/wasm-quarto-hub-client/pkg`. So the thing that must exist is
`crates/wasm-quarto-hub-client/pkg/`; do not "fix" either path.

If it is missing, build it first (~10 min):

```bash
cd hub-client && npm run build:wasm && cd ..
```

Then, from the worktree root:

```bash
npm run test:integration -w ts-packages/preview-renderer
```

Expected: `Test Files 1 failed | 49 passed (50)`, `Tests 1 failed | 578 passed | 1 skipped (580)`,
failing on `Equation > appends \tag{N}`.

- [ ] **Step 2: Fix the positive assertion's selector**

Replace lines 659–665 — this exact block:

```tsx
        // KaTeX renders \tag{N} as a side-floated number; the rendered
        // tree contains a `<span class="tag">` whose textContent is the
        // parenthesized number. Splitting by individual character spans
        // means we check textContent rather than innerHTML.
        const tagEl = span!.querySelector('.tag');
        expect(tagEl).not.toBeNull();
        expect(tagEl!.textContent).toBe('(1)');
```

with:

```tsx
        // KaTeX renders \tag{N} as a side-floated number. Since 0.18 the
        // wrapper class is `katex-tag` (0.17 and earlier emitted a bare
        // `tag`); the number is split across character spans, so assert on
        // textContent rather than innerHTML.
        const tagEl = span!.querySelector('.katex-tag');
        expect(tagEl).not.toBeNull();
        expect(tagEl!.textContent).toBe('(1)');
```

- [ ] **Step 3: Fix the negative assertion's selector**

In `it('does NOT append \\tag when order is missing')`, replace lines 674–676 —
this exact block:

```tsx
        // No KaTeX-emitted `.tag` wrapper when no \tag{} command was
        // appended to the latex.
        expect(span!.querySelector('.tag')).toBeNull();
```

with:

```tsx
        // No KaTeX-emitted `.katex-tag` wrapper when no \tag{} command was
        // appended to the latex. (This assertion was vacuous under KaTeX
        // 0.18 while it still queried the pre-0.18 `.tag`.)
        expect(span!.querySelector('.katex-tag')).toBeNull();
```

- [ ] **Step 4: Run the suite to verify it passes**

```bash
npm run test:integration -w ts-packages/preview-renderer
```

Expected: `Test Files 50 passed (50)`, `Tests 579 passed | 1 skipped (580)`.

- [ ] **Step 5: Prove both assertions actually bind (revert check)**

A test fixed by editing its expectation is guilty until proven otherwise. Break
the product code and confirm **both** tests notice.

In `ts-packages/preview-renderer/src/q2-preview/custom/Equation.tsx:83-84`, find:

```tsx
        const taggedFirst: InlineNode =
            number !== undefined ? tagInline(first, number) : first;
```

Temporarily replace it with:

```tsx
        const taggedFirst: InlineNode = first;
```

Re-run the suite. Expected: the *positive* test now FAILS (`expected null not to
be null`) while the negative test still passes. That proves the positive
assertion binds.

Now make the tag unconditional instead:

```tsx
        const taggedFirst: InlineNode = tagInline(first, number ?? 0);
```

Re-run. Expected: the *negative* test now FAILS (`expected <span ...> to be
null`). That proves the negative assertion binds — the thing it could not do
before this task.

Restore and confirm green:

```bash
git checkout -- ts-packages/preview-renderer/src/q2-preview/custom/Equation.tsx
npm run test:integration -w ts-packages/preview-renderer
```

- [ ] **Step 6: Check for other stale KaTeX class assertions**

From the worktree root:

```bash
grep -rnE "querySelector\('\.(tag|base|strut|mord)'" \
  --include='*.test.ts*' --include='*.spec.ts' \
  ts-packages/ hub-client/ q2-preview-spa/ trace-viewer/ q2-demos/
```

Expected: no output. (At time of writing the two lines fixed above were the only
hits in the whole tree. If new ones appear, fix them the same way and note them
in the commit.)

- [ ] **Step 7: Commit**

```bash
git add ts-packages/preview-renderer/src/q2-preview/custom-components.integration.test.tsx
git commit -m "Fix Equation \\tag test for KaTeX 0.18 class rename (GH #250)

KaTeX prefixed its output classes at 0.18 (tag -> katex-tag), which
landed in d93197033 on 2026-08-08. The positive assertion in
custom-components.integration.test.tsx has been failing since; the
negative assertion two lines down has been passing vacuously, because
.tag matches nothing under 0.18.

Both now query .katex-tag. Verified by revert: nulling the \\tag append
reddens the positive test, making it unconditional reddens the negative
one -- so both assertions now bind.

This is the failure that makes \`cargo xtask verify\` red on main. It went
unnoticed for two weeks because ts-packages/preview-renderer runs in no
workflow, which is what GH #250 is about."
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

**This task is a standalone correctness + policy fix.** It does *not* wire the
suite into CI — see Task 5b for that, and note the constraint discovered there:
the sibling `hub` tier shells out to `cargo run --bin hub`, so this package can
only be gated in a workflow that pre-builds that binary.

**Files:**
- Modify: `ts-packages/sync-test-harness/src/server-manager.ts` (export the probe)
- Modify: `ts-packages/sync-test-harness/src/roundtrip.test.ts:112` (`describe` → `describe.skipIf`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `tsSyncServerAvailable(): boolean` exported from
  `./server-manager.js`; a `sync-test-harness` `npm test` that exits 0 without
  `external-sources/`. Task 5b wires that suite into CI.

- [ ] **Step 1: Reproduce the failure**

```bash
npm test -w ts-packages/sync-test-harness
```

Expected: `Test Files 1 failed | 1 passed (2)`, `Tests 8 passed | 3 skipped (11)`,
with `FAIL src/roundtrip.test.ts > ts-sync-server` /
`Error: Timeout (30000ms) waiting for ts-sync-server to be ready.`

(If you *do* have `external-sources/automerge-repo-sync-server` checked out,
this will pass instead. Rename the directory aside to reproduce.)

- [ ] **Step 2: Export an availability probe from `server-manager.ts`**

The file currently imports `mkdtemp, rm` from `node:fs/promises` (line 9) and
has no `node:fs` import. Add one:

```ts
import { existsSync } from 'node:fs';
```

`REPO_ROOT` is already defined at line 29. Add this directly above the
`/** Start the TypeScript automerge-repo-sync-server. */` docblock at lines
143–147:

```ts
/**
 * Path to the TypeScript reference sync server. It lives in
 * `external-sources/`, which is NOT version-controlled — see the External
 * Sources Policy in CLAUDE.md. Tests that need it must skip when it is
 * absent rather than fail, so the suite is CI-able (GH #250).
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

Then, inside `startTsSyncServer`, delete the now-redundant
`const serverDir = path.join(REPO_ROOT, 'external-sources', 'automerge-repo-sync-server');`
line (150) and point the spawn at the new constant:

```ts
  const proc = spawn('node', ['src/index.js'], {
    cwd: TS_SYNC_SERVER_DIR,
```

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

Leave the `describe('hub', ...)` at line 150 alone.

- [ ] **Step 4: Run the suite to verify it passes**

```bash
npm test -w ts-packages/sync-test-harness
```

Expected: exit 0, **`Test Files 2 passed (2)`**, `Tests 8 passed | 3 skipped (11)`.

Note the file count: `roundtrip.test.ts` holds *two* describes — `ts-sync-server`
(line 112) and `hub` (line 150). Skipping only the first leaves the hub tier
running, so the **file** reports passed, not skipped. The 8 passing tests are 3
hub reconnect cases plus 5 from `concurrent-editing.test.ts`; the 3 skipped are
the `ts-sync-server` cases.

- [ ] **Step 5: Verify the skip is conditional, not unconditional**

The failure mode to rule out is a probe that always returns `false`, which would
silently disable the tier for developers who *do* have the server.

**This step writes into `external-sources/`. Do not run it if the real server is
checked out** — it would clobber and then delete someone's checkout. Guard
first:

```bash
if [ -e external-sources/automerge-repo-sync-server ]; then
  echo "REAL CHECKOUT PRESENT — skip this step; the probe is already exercised"
else
  mkdir -p external-sources/automerge-repo-sync-server/src
  echo "console.log('Listening on port ' + process.env.PORT)" \
    > external-sources/automerge-repo-sync-server/src/index.js
  npx vitest run src/roundtrip.test.ts --root ts-packages/sync-test-harness 2>&1 \
    | grep -E 'ts-sync-server|Test Files'
  rm -rf external-sources/automerge-repo-sync-server
fi
```

Expected (in the else branch): the `ts-sync-server` tier is now *attempted*
rather than skipped. It will then fail against the stub server — that is fine
and expected; we only need to see it stop being skipped.

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

Build order does **not** matter — per `crates/xtask/src/ts_packages.rs`, types
resolve via `src/`, so each package's `tsc` compiles without its dependencies'
`dist/` present. So a loop suffices; do not hand-order the list (it would
drift).

Two precedents, and they differ: `cargo xtask verify` step 6
(`verify.rs:230-247`) passes every workspace to a **single**
`npm run build --if-present -w a -w b …` invocation, while
`hub-client-e2e.yml:144-153` uses a **loop**. Use the loop form — it is already
proven in CI and needs no list.

**Files:**
- Modify: `.github/workflows/ts-test-suite.yml` (append at the end of the `test-suite` job, after line 206)

**Interfaces:**
- Consumes: the `Install npm dependencies` (`npm ci`) step already in the workflow.
- Produces: `ts-packages/*/dist/` present for all later steps in the job. Tasks 4
  and 5 assume it.

- [ ] **Step 1: Add the build step**

Append to the end of the `test-suite` job:

```yaml
      # ── Workspace TS suites (GH #250) ──────────────────────────────────
      #
      # ts-packages dists must exist before the suites below: quarto-sync-client,
      # quarto-hub-mcp and annotated-qmd resolve workspace siblings through the
      # `"import": "./dist/index.js"` export condition, and hub-client bundles
      # these packages from *source*, so nothing above this line builds them.
      # Loop form matches hub-client-e2e.yml; build order is irrelevant because
      # types resolve via src/ (see crates/xtask/src/ts_packages.rs).
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

- [ ] **Step 2: Verify the loop locally from a clean state**

`ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js` is **tracked in
git** (the bundle-freshness gate diffs it), so a blanket `rm -rf` deletes a
tracked file. Restore it immediately after:

```bash
rm -rf ts-packages/*/dist
git checkout -- ts-packages/quarto-engine-host-deno/dist
for pkg in ts-packages/*/; do npm run build --if-present -w "${pkg%/}"; done
node ts-packages/quarto-hub-mcp/dist/index.js --help
git status --porcelain ts-packages
```

Expected: every build exits 0; the smoke check prints usage text and exits 0;
`git status --porcelain ts-packages` prints **nothing** (no tracked file left
deleted or modified). `tsc` on `quarto-engine-host-deno` does not regenerate
`dist/engine-host-deno.js` — there is no corresponding
`src/engine-host-deno.ts` — so the tracked bundle is untouched by the loop
itself.

- [ ] **Step 3: Verify the step fixes all three cold failures**

```bash
npm test -w ts-packages/quarto-sync-client && \
npm test -w ts-packages/quarto-hub-mcp && \
npm test -w ts-packages/annotated-qmd
```

Expected: `quarto-sync-client` → `Test Files 21 passed (21)`, `Tests 137 passed (137)`
(it failed 14 files on a dist-less tree). `quarto-hub-mcp` → `Test Files 22 passed (22)`,
`Tests 246 passed | 3 skipped (249)`. `annotated-qmd` → runs to completion
reporting `# tests 156 / # pass 154 / # fail 2` — **still red, and expected to
be** (that is bd-1d6io, Task 7); the point is that it no longer dies with
`ERR_MODULE_NOT_FOUND` partway through.

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

Same loop form as hub-client-e2e.yml, plus the node dist/index.js --help
ESM-link smoke check that cargo xtask verify step 6 runs."
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
   (`src/**/*.wasm.test.ts`) matches **zero files** — harmless only because that
   config sets `passWithNoTests: true`. `preview-runtime`'s `test:wasm` points
   at a `vitest.wasm.config.ts` that **does not exist**, so that script always
   fails. Those dead scripts are census class 5, out of scope — avoid them
   rather than fix them.
2. **Use the root `npm test -w <path>` form.** Note this differs from the
   existing `engine-host-deno` step, which addresses its package by *name*
   (`npm run test -w @quarto/engine-host-deno`). Both forms work; paths are used
   here because they read unambiguously against the `workspaces` globs.

**Files:**
- Modify: `.github/workflows/ts-test-suite.yml` (after Task 3's steps)

**Interfaces:**
- Consumes: `npm ci`. (Not Task 3's dists — none of these eight need them; they
  are sequenced after Task 3 only to keep the new block contiguous.)
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

Expected: `ALL GREEN`, having reported roughly 549, 77, 368, 36, 19, 46, 76, 35,
20 and 10 passing tests respectively.

**On count mismatches:** `ALL GREEN` is the gate; the numbers are the census
baseline and will drift as people add tests. A count that is *higher*, or lower
by a test or two with everything still passing, is drift — note the new number
and move on. Only a non-zero exit, or a count that has dropped sharply, needs
investigating. If a `-w` path fails to resolve, check it against the
`workspaces` array in the root `package.json` (`ts-packages/*`, `hub-client`,
`trace-viewer`, `q2-preview-spa`, `q2-demos/*`).

- [ ] **Step 3: Validate the workflow file parses**

PyYAML is **not** installed in this environment (`python3 -c "import yaml"`
raises `ModuleNotFoundError`); `yq` is, at `/opt/homebrew/bin/yq`. Use it:

```bash
yq '.jobs."test-suite".steps | length' .github/workflows/ts-test-suite.yml
```

Expected: a number — parsing succeeded, and the count should have grown by the
number of steps you added. A parse error exits non-zero with a message naming
the line.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ts-test-suite.yml
git commit -m "CI: run the eight green workspace TS suites (GH #250)

preview-renderer (549), quarto-api (368), preview-runtime (77),
quarto-automerge-schema (36), wasm-js-bridge (19), q2-preview-spa
(46+76), kanban (35+20) and trace-viewer (10) all pass as-is and ran in
no workflow. ~1,200 assertions, ~35s of wall time.

Explicit npm test rather than each package's test:ci: kanban's test:ci
chains a test:wasm leg matching zero files, and preview-runtime's
test:wasm names a nonexistent config. Those dead scripts are census
class 5, out of scope here."
```

---

### Task 5: Wire the dist-dependent suites into `ts-test-suite.yml`

Three more suites, green **given** earlier steps in the job:

| Package | Command | Tests | Depends on |
| --- | --- | --- | --- |
| `ts-packages/preview-renderer` | `npm run test:integration` | 578 (+1 skip) | Task 1's fix **and** the existing `Build WASM module` step (line 154) |
| `ts-packages/quarto-sync-client` | `npm test` | 137 | Task 3's dists |
| `ts-packages/quarto-hub-mcp` | `npm test` | 246 (+3 skip) | Task 3's dists |

**On ordering:** the only dependency this task's *placement* actually enforces
is Task 3's dist build preceding `quarto-sync-client` and `quarto-hub-mcp`. The
WASM dependency for `preview-renderer`'s integration tier is real but is already
satisfied by every position at or below line 154, so splitting it out from
Task 4 documents the constraint rather than creating it. Keep the comment
anyway — it is what stops someone hoisting these steps above the WASM build.

`sync-test-harness` is deliberately **not** here; see Task 5b.

**Files:**
- Modify: `.github/workflows/ts-test-suite.yml` (after Task 4's steps)

**Interfaces:**
- Consumes: Task 1 (`\tag` fix), Task 3 (dists).
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Add the steps**

```yaml
      # preview-renderer's integration tier imports wasm-quarto-hub-client
      # (aliased in vitest.integration.config.ts to the committed
      # hub-client/wasm-quarto-hub-client symlink -> crates/…/pkg). Without it
      # 26 of 50 files fail on module resolution, so this must stay below the
      # `Build WASM module` step.
      - name: Run preview-renderer integration tests
        shell: bash
        run: npm run test:integration -w ts-packages/preview-renderer

      # These two resolve workspace siblings through dist/ — they need the
      # "Build ts-packages workspaces" step above. hub-mcp is the slowest
      # suite here (~32s: bundle.test.ts runs esbuild in beforeAll).
      - name: Run quarto-sync-client tests
        shell: bash
        run: npm test -w ts-packages/quarto-sync-client

      - name: Run quarto-hub-mcp tests
        shell: bash
        run: npm test -w ts-packages/quarto-hub-mcp
```

- [ ] **Step 2: Verify locally, in order**

```bash
npm run test:integration -w ts-packages/preview-renderer && \
npm test -w ts-packages/quarto-sync-client && \
npm test -w ts-packages/quarto-hub-mcp && echo ALL GREEN
```

Expected: `ALL GREEN`, reporting 579+1skip (post-Task-1), 137, and 246+3skip.
The count-mismatch rule from Task 4 Step 2 applies here too.

- [ ] **Step 3: Confirm the WASM ordering constraint is real, not cargo-culted**

```bash
mv crates/wasm-quarto-hub-client/pkg /tmp/pkg-stash
npm run test:integration -w ts-packages/preview-renderer 2>&1 \
  | grep -c 'Failed to resolve import "wasm-quarto-hub-client"'
mv /tmp/pkg-stash crates/wasm-quarto-hub-client/pkg
```

Expected: a non-zero count (26 file-level failures at time of writing). Moving
`pkg` is what breaks it — `hub-client/wasm-quarto-hub-client` is a symlink whose
target this removes.

- [ ] **Step 4: Validate and commit**

```bash
yq '.jobs."test-suite".steps | length' .github/workflows/ts-test-suite.yml
git add .github/workflows/ts-test-suite.yml
git commit -m "CI: run the dist-dependent workspace TS suites (GH #250)

preview-renderer integration (578), quarto-hub-mcp (246) and
quarto-sync-client (137). Each is green only given an earlier step in the
job -- the ts-packages dists for the latter two, the WASM build for
preview-renderer's integration tier -- so they are ordered after Tasks 3
and 4."
```

---

### Task 5b: Wire `sync-test-harness` into `hub-client-e2e.yml`

**Why a different workflow.** `startHubServer` at
`ts-packages/sync-test-harness/src/server-manager.ts:98-116` does not launch a
prebuilt binary — it spawns the compiler:

```ts
  const proc = spawn('cargo', ['run', '--bin', 'hub', '--', …], { cwd: REPO_ROOT });
  await waitForOutput(proc, /Hub server listening/, 120_000, 'hub');
```

Both of the package's test files depend on it: `roundtrip.test.ts:150`
(`describe('hub')`) and `concurrent-editing.test.ts:140` (all 5 tests).
`ts-test-suite.yml` does no native cargo build and its Rust cache is commented
out (lines 66–70), so a cold build of the workspace inside 120 s is not
plausible there. The census's "8 green tests" was measured against a warm local
`target/`.

`hub-client-e2e.yml` already solves this: line 159 is `cargo build --bin hub`,
with a comment saying it exists precisely because the harness shells out to
`cargo run --bin hub` under a 120 s timeout. It runs on push and PR to `main`
with no path filter, so it is a real merge gate, and it has an active Rust
cache. Marginal cost of adding this suite there is ~1 min. The trade-off,
accepted deliberately: ubuntu-only (no macOS leg), and one package's wiring
lives in a second workflow.

**Files:**
- Modify: `.github/workflows/hub-client-e2e.yml` (insert after the `Pre-build hub binary` step at lines 158–159)

**Interfaces:**
- Consumes: Task 2 (`tsSyncServerAvailable`), and that workflow's existing
  `cargo build --bin hub` + `npm ci` steps.
- Produces: nothing later tasks depend on. Task 6's lint rule must scan **this
  workflow too** — that is why its `WORKFLOWS` constant is a list.

- [ ] **Step 1: Confirm the prerequisite step is where the plan says**

```bash
sed -n '155,166p' .github/workflows/hub-client-e2e.yml
```

Expected: a comment about `globalSetup` launching the hub via
`cargo run --bin hub` with a 120 s timeout, then
`- name: Pre-build hub binary` / `run: cargo build --bin hub`, then
`- name: Install Playwright`.

- [ ] **Step 2: Add the step**

Insert directly after the `Pre-build hub binary` step, before
`Install Playwright`:

```yaml
      # sync-test-harness's hub tier spawns `cargo run --bin hub` with a 120s
      # readiness timeout, so it belongs in this workflow rather than
      # ts-test-suite.yml: the step above has already compiled that binary, so
      # `cargo run` is a no-op re-exec. Its ts-sync-server tier skips itself
      # (needs external-sources/, which CI never has). GH #250.
      - name: Run sync-test-harness tests
        run: npm test -w ts-packages/sync-test-harness
```

- [ ] **Step 3: Verify locally with a prebuilt binary**

```bash
cargo build --bin hub
npm test -w ts-packages/sync-test-harness
```

Expected: exit 0, `Test Files 2 passed (2)`, `Tests 8 passed | 3 skipped (11)`.

- [ ] **Step 4: Confirm the `cargo build` prerequisite is real**

This is the claim the whole task rests on, so check it rather than trusting it.
Look at what the harness actually spawns:

```bash
grep -n "spawn(" -A6 ts-packages/sync-test-harness/src/server-manager.ts | head -20
```

Expected: the first `spawn` is `'cargo', ['run', '--bin', 'hub', …]` with
`cwd: REPO_ROOT`, followed by a `waitForOutput(..., 120_000, 'hub')`. If a
future refactor changes this to spawn `target/debug/hub` directly, this task's
placement rationale changes — say so rather than leaving the comment stale.

- [ ] **Step 5: Validate and commit**

```bash
yq '.jobs."e2e-tests".steps | length' .github/workflows/hub-client-e2e.yml
git add .github/workflows/hub-client-e2e.yml
git commit -m "CI: run sync-test-harness in the e2e workflow (GH #250)

Its hub tier spawns \`cargo run --bin hub\` with a 120s readiness timeout,
so it cannot live in ts-test-suite.yml -- that workflow does no native
cargo build and has its Rust cache commented out, so the first run would
be a cold workspace build on both matrix legs. hub-client-e2e.yml
already runs cargo build --bin hub for exactly this reason (and is a
push/PR gate on main), so the marginal cost is ~1 min.

Trade-off accepted: ubuntu-only, and this package's wiring lives in a
second workflow. Task 2 made the ts-sync-server tier skip cleanly, so
what runs here is the 8-test hub tier."
```

---

### Task 6: Lint rule — a workspace package with tests must be wired or explicitly excused

#250's own history is the argument for this task: *"hub-client got wired up and
the rest hasn't been added since."* The wiring drifted for months because
nothing reconciled "packages that have tests" against "packages CI runs". Tasks
3–5b fix today's drift; this task stops tomorrow's.

**Why in `xtask lint` rather than `xtask verify`:** `cargo xtask lint` runs in
`test-suite.yml`, so it is a real merge gate. `verify` is developer-run only —
and the census's headline finding is that a verify-only gate does *not* hold
(`verify` is red on `main` right now). The oddity of a Rust workflow policing a
TS workflow is worth the gate.

The rule follows the existing repo-level pattern (`error_docs`,
`error_docs_sidebar` in `crates/xtask/src/lint/`): reconcile two trees, run once
per lint invocation rather than per Rust file, anchor violations at a real line.

**What "wired" means (the load-bearing design choice).** A naive
"package path appears anywhere in the workflow" predicate is inert for exactly
the packages that matter most: after Task 3, the workflow text contains
`ts-packages/quarto-hub-mcp` in the MCP *smoke-check* line, and `hub-client` in
the WASM *build* step — so deleting either package's test step would still pass.
A bare comment would satisfy it too. So the rule matches **per step**: it splits
the workflow into `- name:` step chunks and requires some chunk to contain both
a test-invocation marker (`npm test`, `npm run test`, `deno test`, `vitest`) and
the package's path or npm name. That handles `hub-client`'s
`cd hub-client` + `npm run test:ci` (two lines, one chunk) with no
special-casing, while the build and smoke-check chunks no longer count.

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

Create `crates/xtask/src/lint/ci_test_wiring.rs` with **only** the test module
below, **and** add the module declaration to `mod.rs` in this same step:

```rust
mod ci_test_wiring;
```

Both halves are needed now: without the `mod` line the crate never compiles the
new file, so Step 2 would report "no tests matched" instead of the compile
failure that makes this a real TDD gate.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a fake repo: root package.json with `workspaces`, one package
    /// per (path, name, has_test), and one workflow file per (name, body).
    fn scaffold(
        globs: &[&str],
        packages: &[(&str, &str, bool)],
        workflows: &[(&str, &str)],
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
            // Multi-line on purpose: the violation's line anchor points at
            // the `"test":` line, which a single-line manifest cannot test.
            let manifest = if *has_test {
                format!(
                    "{{\n  \"name\": \"{}\",\n  \"scripts\": {{\n    \"build\": \"tsc\",\n    \"test\": \"vitest run\"\n  }}\n}}\n",
                    name
                )
            } else {
                format!(
                    "{{\n  \"name\": \"{}\",\n  \"scripts\": {{\n    \"build\": \"tsc\"\n  }}\n}}\n",
                    name
                )
            };
            fs::write(dir.join("package.json"), manifest).unwrap();
        }

        let wf_dir = root.join(".github/workflows");
        fs::create_dir_all(&wf_dir).unwrap();
        for (name, body) in workflows {
            fs::write(wf_dir.join(name), body).unwrap();
        }

        tmp
    }

    /// Both workflow files the rule scans, so a test can put a step in either.
    fn workflows(ts_suite: &str, e2e: &str) -> Vec<(&'static str, String)> {
        vec![
            ("ts-test-suite.yml", ts_suite.to_string()),
            ("hub-client-e2e.yml", e2e.to_string()),
        ]
    }

    fn scaffold2(
        globs: &[&str],
        packages: &[(&str, &str, bool)],
        ts_suite: &str,
        e2e: &str,
    ) -> tempfile::TempDir {
        let wfs = workflows(ts_suite, e2e);
        let refs: Vec<(&str, &str)> =
            wfs.iter().map(|(n, b)| (*n, b.as_str())).collect();
        scaffold(globs, packages, &refs)
    }

    #[test]
    fn flags_a_tested_package_absent_from_the_workflows() {
        let tmp = scaffold2(
            &["ts-packages/*"],
            &[("ts-packages/lonely", "@quarto/lonely", true)],
            "jobs:\n  test-suite:\n    steps: []\n",
            "jobs:\n  e2e-tests:\n    steps: []\n",
        );
        let violations = check(tmp.path()).unwrap();
        assert_eq!(violations.len(), 1, "{:?}", violations);
        assert_eq!(violations[0].rule, "ci-test-suite-unwired");
        assert!(
            violations[0].message.contains("@quarto/lonely"),
            "{}",
            violations[0].message
        );
        assert!(violations[0]
            .file
            .ends_with("ts-packages/lonely/package.json"));
        // The scaffolded manifest puts `"test":` on line 5.
        assert_eq!(violations[0].line, 5);
    }

    #[test]
    fn accepts_a_package_whose_test_step_names_its_path() {
        let tmp = scaffold2(
            &["ts-packages/*"],
            &[("ts-packages/wired", "@quarto/wired", true)],
            "jobs:\n  steps:\n    - name: Run wired\n      run: npm test -w ts-packages/wired\n",
            "jobs:\n  steps: []\n",
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn accepts_a_package_wired_in_the_e2e_workflow() {
        let tmp = scaffold2(
            &["ts-packages/*"],
            &[("ts-packages/harness", "@quarto/harness", true)],
            "jobs:\n  steps: []\n",
            "jobs:\n  steps:\n    - name: Run harness\n      run: npm test -w ts-packages/harness\n",
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn accepts_a_multi_line_step_that_cds_then_tests() {
        // hub-client's real shape: `cd hub-client` and `npm run test:ci` are
        // separate lines of one step, so line-scoped matching would miss it.
        let tmp = scaffold2(
            &["hub-client"],
            &[("hub-client", "hub-client", true)],
            "jobs:\n  steps:\n    - name: Run hub-client tests\n      run: |\n        cd hub-client\n        npm run test:ci\n",
            "jobs:\n  steps: []\n",
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn a_build_step_mention_does_not_count_as_wired() {
        // The regression this rule almost shipped with: Task 3's smoke check
        // names ts-packages/quarto-hub-mcp in a *build* step. That must not
        // satisfy the rule.
        let tmp = scaffold2(
            &["ts-packages/*"],
            &[("ts-packages/quarto-hub-mcp", "@quarto/hub-mcp", true)],
            "jobs:\n  steps:\n    - name: Smoke-check\n      run: node ts-packages/quarto-hub-mcp/dist/index.js --help\n",
            "jobs:\n  steps: []\n",
        );
        let violations = check(tmp.path()).unwrap();
        assert_eq!(violations.len(), 1, "{:?}", violations);
        assert!(violations[0].message.contains("@quarto/hub-mcp"));
    }

    #[test]
    fn a_comment_mention_does_not_count_as_wired() {
        let tmp = scaffold2(
            &["ts-packages/*"],
            &[("ts-packages/commented", "@quarto/commented", true)],
            "jobs:\n  steps:\n    # npm test -w ts-packages/commented would go here\n    - name: Something else\n      run: npm test -w ts-packages/other\n",
            "jobs:\n  steps: []\n",
        );
        assert_eq!(check(tmp.path()).unwrap().len(), 1);
    }

    #[test]
    fn ignores_packages_without_a_test_script() {
        let tmp = scaffold2(
            &["ts-packages/*"],
            &[("ts-packages/typesonly", "@quarto/types", false)],
            "jobs:\n  steps: []\n",
            "jobs:\n  steps: []\n",
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn ignores_excused_packages() {
        // Skips itself if the excuse list has been emptied (see Task 7).
        let Some((excused_name, _)) = EXCUSED.first() else {
            return;
        };
        let tmp = scaffold2(
            &["ts-packages/*"],
            &[("ts-packages/excused-pkg", excused_name, true)],
            "jobs:\n  steps: []\n",
            "jobs:\n  steps: []\n",
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn expands_literal_and_glob_workspace_entries() {
        let tmp = scaffold2(
            &["ts-packages/*", "trace-viewer"],
            &[
                ("ts-packages/a", "@quarto/a", true),
                ("trace-viewer", "@quarto/trace-viewer", true),
            ],
            "jobs:\n  steps:\n    - name: Run a\n      run: npm test -w ts-packages/a\n",
            "jobs:\n  steps: []\n",
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

Expected: a **compile error**, `cannot find function 'check' in this scope`
(and the same for `EXCUSED`). If instead you see "no tests to run", the `mod
ci_test_wiring;` declaration from Step 1 is missing.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/xtask/src/lint/ci_test_wiring.rs`, above the test module:

```rust
//! Lint: every npm-workspace package with a `test` script must be run by a
//! step in one of the CI workflows, or be listed in `EXCUSED` with a reason.
//!
//! Why this exists (GH #250, 2026-08-22): CI ran `hub-client`'s tests and
//! `engine-host-deno`'s, and nothing else. Roughly 2,000 assertions across a
//! dozen packages sat outside the merge gate for months — long enough for a
//! KaTeX class rename to redden `preview-renderer` on `main` unnoticed. The
//! wiring drifted because nothing reconciled "packages that have tests"
//! against "packages CI runs". This rule is that reconciliation.
//!
//! It matches per *step*, not per file: a package path appearing in a build
//! step or a comment does not count as wired. That distinction matters —
//! `ts-packages/quarto-hub-mcp` appears in the MCP smoke-check step and
//! `hub-client` in the WASM build step, so a whole-file substring match would
//! be inert for exactly those two packages.
//!
//! Like `error_docs` and `error_docs_sidebar`, this is a *repo-level* rule: it
//! compares two trees rather than grepping one Rust file, so it runs once per
//! lint invocation.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::Violation;

const RULE: &str = "ci-test-suite-unwired";

/// Workflows that may satisfy the rule. `sync-test-harness` lives in the e2e
/// workflow because its hub tier shells out to `cargo run --bin hub`, which
/// only that workflow pre-builds — hence a list, not a single file.
const WORKFLOWS: &[&str] = &[
    ".github/workflows/ts-test-suite.yml",
    ".github/workflows/hub-client-e2e.yml",
];

/// Substrings that mark a step as actually running tests.
const TEST_MARKERS: &[&str] = &["npm test", "npm run test", "deno test", "vitest"];

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

/// Split a workflow into per-step chunks and keep the ones that run tests.
///
/// A "step" starts at a `- name:` line (any indentation) and runs to the next
/// one. Comment lines are dropped first, so a commented-out invocation cannot
/// satisfy the rule.
fn test_step_chunks(workflow: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in workflow.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        if line.trim_start().starts_with("- name:") {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
        .into_iter()
        .filter(|chunk| TEST_MARKERS.iter().any(|m| chunk.contains(m)))
        .collect()
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

    // Every test-running step across every scanned workflow.
    //
    // Bail only if *no* workflow file could be read (a partial checkout);
    // workflows that exist but run no tests must still produce violations,
    // otherwise the rule would go quiet exactly when everything is unwired.
    let mut test_chunks: Vec<String> = Vec::new();
    let mut found_a_workflow = false;
    for wf in WORKFLOWS {
        if let Ok(body) = std::fs::read_to_string(workspace_root.join(wf)) {
            found_a_workflow = true;
            test_chunks.extend(test_step_chunks(&body));
        }
    }
    if !found_a_workflow {
        return Ok(Vec::new());
    }

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

        let wired = test_chunks.iter().any(|chunk| {
            chunk.contains(&rel) || (!name.is_empty() && chunk.contains(name))
        });
        if wired {
            continue;
        }

        violations.push(Violation {
            file: manifest_path.clone(),
            line: test_script_line(&manifest),
            column: 1,
            rule: RULE,
            message: format!(
                "{} has a `test` script but no CI step runs it — its tests are \
                 not gated (scanned: {})",
                if name.is_empty() { &rel } else { name },
                WORKFLOWS.join(", ")
            ),
            suggestion: Some(format!(
                "Add a step running `npm test -w {}` to one of {}, or add the \
                 package to EXCUSED in crates/xtask/src/lint/ci_test_wiring.rs \
                 with a reason and a tracking strand.",
                rel,
                WORKFLOWS.join(" / ")
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

Expected: 9 tests pass.

- [ ] **Step 5: Wire the rule into the lint runner**

The `mod ci_test_wiring;` declaration went in at Step 1. Now add the call. In
`crates/xtask/src/lint/mod.rs`'s `run_check`, after the `error_docs_sidebar`
block (`workspace_root` is already in scope from line 101):

```rust
    if config.verbose {
        eprintln!("Checking npm-workspace test scripts against the CI workflows");
    }
    all_violations.extend(ci_test_wiring::check(&workspace_root)?);
```

- [ ] **Step 6: Run the real lint and confirm it is clean**

```bash
cargo xtask lint
```

Expected: `All checks passed!`. If it flags a package, that package genuinely
isn't wired — either add its step (it belongs in Task 4, 5, or 5b) or add it to
`EXCUSED` with a reason. Do not weaken the rule to make it pass.

- [ ] **Step 7: Confirm the rule bites (revert check)**

Use `quarto-hub-mcp` deliberately: it is the package a whole-file substring
match would have been inert for, because Task 3's smoke-check step names its
path. If the rule fires here, the per-step matching works.

```bash
python3 - <<'PY'
p='.github/workflows/ts-test-suite.yml'
s=open(p).read()
s=s.replace("        run: npm test -w ts-packages/quarto-hub-mcp\n","        run: true\n")
open(p,'w').write(s)
PY
cargo xtask lint 2>&1 | grep ci-test-suite-unwired
git checkout -- .github/workflows/ts-test-suite.yml
```

Expected: the grep prints a violation naming `@quarto/hub-mcp`, even though
`ts-packages/quarto-hub-mcp` still appears in the smoke-check step. If it prints
nothing, the per-step scoping is not working — fix it before continuing.

- [ ] **Step 8: Document the rule in `CLAUDE.md`**

Add to the "Current Lint Rules" list, after the `metadata-as-str` entry:

```markdown
- **ci-test-suite-unwired**: Every npm-workspace package with a `test` script must be run by a step in `.github/workflows/ts-test-suite.yml` or `.github/workflows/hub-client-e2e.yml`, or be listed in `EXCUSED` in `crates/xtask/src/lint/ci_test_wiring.rs` with a reason and a tracking strand. CI ran only `hub-client` and `engine-host-deno` for months while ~2,000 assertions across a dozen packages sat outside the merge gate — long enough for a KaTeX class rename to redden `preview-renderer` on `main` unnoticed (GH #250). Nothing reconciled "packages that have tests" against "packages CI runs"; this rule is that reconciliation. Matching is **per step**, not per file: a package path appearing in a build step or a comment does not count as wired (`ts-packages/quarto-hub-mcp` appears in the MCP smoke-check step, `hub-client` in the WASM build step, so a whole-file match would be inert for exactly those two). Two workflows are scanned because `sync-test-harness`'s hub tier spawns `cargo run --bin hub`, which only the e2e workflow pre-builds. Like the `error-docs-*` rules it is *repo-level* and anchors violations at the offending `package.json`'s `"test":` line. **When you add a `test` script to a workspace package, add its CI step in the same commit.**
```

- [ ] **Step 9: Commit**

```bash
git add crates/xtask/src/lint/ci_test_wiring.rs crates/xtask/src/lint/mod.rs CLAUDE.md
git commit -m "xtask lint: flag workspace packages whose tests CI never runs (GH #250)

New repo-level rule ci-test-suite-unwired. Every npm-workspace package
with a test script must be run by a step in ts-test-suite.yml or
hub-client-e2e.yml, or be listed in EXCUSED with a reason and a strand.

Matching is per step rather than per file, which matters: hub-mcp's path
appears in the MCP smoke-check step and hub-client's in the WASM build
step, so a whole-file substring match would have been inert for exactly
those two packages. Chunks are split on `- name:` and comment lines are
dropped, so a commented-out invocation does not count either.

Tasks 3-5b fixed today's drift; this stops tomorrow's. #250 exists
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
entry, leaving an empty slice:

```rust
pub(crate) const EXCUSED: &[(&str, &str)] = &[];
```

**No test changes are needed.** `ignores_excused_packages` opens with
`let Some((excused_name, _)) = EXCUSED.first() else { return; };`, so it skips
itself once the list is empty.

Accept, and note in the commit, that this leaves the `EXCUSED.iter().any(...)`
branch uncovered — with an empty slice it is unreachable, so there is nothing
meaningful to cover. The alternative (threading an excuse list through `check`'s
signature purely to keep a test alive) is not worth the API change.

- [ ] **Step 4: Verify**

```bash
cargo nextest run -p xtask -E 'test(ci_test_wiring)'
cargo xtask lint
```

Expected: tests pass (`ignores_excused_packages` now skips), lint clean.

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
cargo xtask verify --skip-hub-build --skip-hub-tests   # Rust + lint + TS package legs
cargo nextest run --workspace                          # ~3 min; report the delta
```

`verify` step 11 (the preview-renderer + preview-runtime leg) is gated by
`--skip-shared-package-tests`, *not* by either flag above, so it does run — and
it is the leg that was **red on `main`** before Task 1. After Task 1 it should
be green; if it isn't, something in Task 1 regressed.

**Precondition:** `--skip-hub-build` does not rebuild the WASM, so
`crates/wasm-quarto-hub-client/pkg/` must already exist or that leg fails for
reasons unrelated to this plan. Build it first (`cd hub-client && npm run
build:wasm`) if it's missing.

- [ ] **File strands for the out-of-scope census findings** (classes 5–7). These
  are genuinely outside this plan, so they belong in braid, one each:
  - Rust doctests run nowhere and are red — 5 failures, ~70 compile errors from
    untagged prose blocks, plus a stale `quarto-sass` doctest calling a two-arg
    `ThemeContext::new` with one argument.
  - ~80 silent-skip sites let engine tests (jupyter, knitr/R, julia, uv,
    dart-sass) pass vacuously in CI; extend the `QUARTO_CI=1` hard-fail pattern
    beyond deno, or install the engines.
  - `tree-sitter-doctemplate`'s 215-line corpus runs in neither CI nor `verify`.
  - `wasm-qmd-parser`'s 4 tests never build (workspace-excluded).
  - `npm run typecheck --workspaces` runs in no workflow, and `verify`'s
    `typecheck:tests` legs for `preview-renderer`/`preview-runtime` are not in
    CI either.
  - Dead test scripts: `preview-runtime` `test:wasm` (nonexistent config) and
    `test:integration` (zero files), `kanban` `test:wasm` (zero files, masked by
    `passWithNoTests`), `editors/vscode-quarto-rust` `test` (no test sources,
    outside the workspace).
  - `q2-preview-spa`'s 17 Playwright specs run only behind `verify --e2e`.
  - `sync-test-harness` is ubuntu-only (Task 5b) — it has no macOS coverage.
  - `npm install` on macOS prunes non-macOS optional `@esbuild/*` entries from
    `package-lock.json` (468 lines). Anyone bootstrapping a worktree with
    `npm install` and then `git add -A` would break the Linux CI leg. Worth a
    guard or a documented `npm ci`-only rule.
