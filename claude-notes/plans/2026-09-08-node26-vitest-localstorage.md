# hub-client vitest: 23 tests fail under Node 26 — `localStorage` global undefined (bd-lh30hlvd)

**Date:** 2026-09-08
**Braid:** bd-lh30hlvd
**Checkout:** invoked in the main checkout on `braid/bd-ve916wr8-brand-file-fonts` @ `89c580f1` (no worktree created — see § Notes on the checkout)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** Root cause is fully understood and reproduced at HEAD; three fix
candidates were tried empirically and one flag-free candidate passes all 62 affected
tests under Node 26.8.1. The remaining decisions are about *which* layers to fix
(test config, dependency version, environment enforcement) — not about what is wrong.

The user also asked *why this started happening recently* and *how to avoid the class*.
Both are answered below (§ Timeline, § Why the May pin did not hold, § Avoiding the class).

## Issue context

Filed 2026-09-08 by Carlos (bug, P2, labels `hub-client`, `testing`), discovered during
the bd-jsvetdea pre-flight verify. Under Node v26.8.1, `npm run test` in `hub-client`
prints `ExperimentalWarning: localStorage is not available because --localstorage-file
was not provided` and every test touching `localStorage` fails with
`TypeError: Cannot read properties of undefined (reading 'clear')`. 23 tests across 6
files; `cargo xtask verify`'s hub-client leg is red on Node 26 machines; CI (Node 24) is
green.

## Dependency graph

- **discovered-from** bd-jsvetdea (closed) — "User theme .scss compile error is
  swallowed". Its pre-flight `cargo xtask verify` tripped over this. No other context
  carried; the parent is unrelated to Node.
- **related** (incoming) bd-202u5bld — "Add a Rust grammar to quarto-highlight".
  Unrelated in substance; the edge looks like a same-session link. Ignore.
- No `blocks` edges either way. No incoming pressure beyond "verify is red locally".

## What the code looks like today — root cause

Reproduced at HEAD (`89c580f1`) with Node 26.8.1, vitest 4.1.8, jsdom 26:

```
$ cd hub-client && npx vitest run src/hooks/usePreference.test.tsx src/services/branchService.test.ts
 FAIL  src/hooks/usePreference.test.tsx > ... 
TypeError: Cannot read properties of undefined (reading 'clear')
 ❯ src/hooks/usePreference.test.tsx:10:16
      10|   localStorage.clear();
 Test Files  2 failed (2)   Tests  12 failed (12)
```

The six failing files all carry `@vitest-environment jsdom` (60 files in hub-client do).
They never polyfill `localStorage` themselves — they rely on vitest's jsdom environment
copying `window.localStorage` onto `globalThis`. That copy is what Node 26 breaks:

1. **Node ≥ 25 defines a `localStorage` accessor on `globalThis`** (Web Storage was
   unflagged in v25.0.0). Without `--localstorage-file` the getter emits the
   ExperimentalWarning and returns `undefined` — but the *property exists*:
   `'localStorage' in globalThis === true`. (Probe output in the investigation README.)
   Node 24 has no such property at all.
2. **vitest's jsdom environment skips window keys that already exist on the global**
   unless they are in its explicit allowlist (`KEYS`). In vitest 4.1.8
   (`node_modules/vitest/dist/chunks/index.DC7d2Pf8.js`, `getWindowKeys`):

   ```js
   if (k in global) return keysArray.includes(k);
   ```

   `KEYS` contains `"Storage"` but **not** `"localStorage"`/`"sessionStorage"`. So on
   Node 26 jsdom's storage is never installed; Node's undefined-returning accessor stays.
3. Every `localStorage.clear()` / `getItem()` in a test then dereferences `undefined`.

**Upstream status.** vitest `main` now lists `localStorage` and `sessionStorage` in the
jsdom keys allowlist, but that change shipped only in **vitest 5.0.0** — I unpacked
4.1.9, 4.1.10, 4.1.11 and 5.0.0 from npm: the key is absent in every 4.1.x and present
in 5.0.0. Nothing in the 4.x line will fix this. (vitest-dev/vitest#8757 is the
upstream issue; workspace pins `^4.0.17`, vite `^7.2.4`, and vitest 5 peers on
`vite ^6.4 || ^7 || ^8`, so an upgrade is at least dependency-compatible.)

### Fix candidates tried (Node 26.8.1, hub-client)

| Candidate | Result | Notes |
| --- | --- | --- |
| `NODE_OPTIONS=--no-webstorage` | unit suite green: 96 files / 1084 tests | **`--no-webstorage` is a bad option on Node 24** (`node: bad option`), so it cannot go unconditionally into shared config or CI. |
| `NODE_OPTIONS=--localstorage-file=<tmp>` | 2 probe files green | Makes Node's *file-backed* Storage the one tests use (jsdom's is still skipped). Persists across runs and workers; wrong semantics for tests. Reject. |
| `PATH` → node@24 | green | Confirms the environment, not a fix. |
| **setupFiles shim using jsdom's own Storage** | **6 affected files green: 62/62** | vitest sets `globalThis.jsdom` to the JSDOM instance; the shim re-points `localStorage`/`sessionStorage` at `jsdom.window.<key>` when the global reads `undefined`. No flags, works on 24 and 26, becomes a no-op after a vitest 5 upgrade. Prototype: `node26-vitest-localstorage-investigation/webstorage-shim.ts`. |

Full `npm run test:ci` under the `--no-webstorage` candidate: unit 1084/1084 and
integration 119/119 pass; one **wasm** smoke-all test fails — identically under Node 24.
That failure is a stale local `wasm_quarto_hub_client_bg.wasm` (built Sep 1; commit
`813850ee` on Sep 3 changed include-error semantics). Not part of this strand; a
`npm run build:wasm` clears it.

## Timeline — why it started "recently" in this environment

All Node on this machine comes from Homebrew; `/opt/homebrew/bin/node` is whatever
`brew` last linked. There is **no version manager** (no nvm/fnm/volta/asdf/mise), so
`.nvmrc` is inert, and `engine-strict` is off, so `engines` only warns.

| Date | Event | Evidence |
| --- | --- | --- |
| 2026-04-28 | `brew` installs unversioned `node` 25.9.0_2 (Node 25 already unflags Web Storage) | `INSTALL_RECEIPT.json` time |
| 2026-05-18 | Same symptom hit; commit `ca6d47c8` "chore(node): pin to Node 24 LTS" adds `.nvmrc` = `24` and `engines.node = ^24.0.0`; `node@24` brew formula installed the same afternoon (15:41) and evidently linked, since local tests were green all summer | commit message; `node@24` receipt |
| 2026-06-01 / 06-10 | CI e2e briefly pinned to 24.15.0 for an unrelated Playwright/yauzl hang, then unpinned back to `'24'` | `55fad91d`, `4a446d85` |
| 2026-09-04 11:10 | `brew upgrade` installs `node` 26.8.1 and **relinks `/opt/homebrew/bin/node` → 26.8.1**, silently displacing `node@24` | `Cellar/node/26.8.1` receipt; `var/homebrew/linked/node` symlink dated Sep 4 |
| 2026-09-08 | First `cargo xtask verify` after the upgrade: hub-client leg red; bd-lh30hlvd filed | strand |

So this is a **recurrence** of the May incident, not a new failure mode. The May fix
aligned the pin with CI but installed nothing that could *enforce* it; the next routine
`brew upgrade` undid the manual link.

## Why the May pin did not hold

- `.nvmrc` only acts through a version manager; none is installed here.
- `engines.node` only acts at `npm install`, and only warns unless `engine-strict` is
  set. (Verified: `npm install --engine-strict --dry-run` under Node 26 fails with
  `EBADENGINE … Required: {"node":"^24.0.0"}`.) `npm test` never consults `engines`.
- `cargo xtask verify` and `dev-setup` never look at the Node version; `dev-setup` checks
  `wasm-opt` but not `node`.
- The test config itself depended on an accident of Node 24 (no `localStorage` global)
  rather than stating the requirement.

## Avoiding the class (draft — to be confirmed in design)

Three independent layers, cheapest first:

1. **Make the test config independent of the Node version.** The shim (or the vitest 5
   upgrade) removes the implicit assumption. This is the only layer that fixes the
   *symptom* for whoever runs the tests on whatever Node.
2. **Make the pin bite.** `.npmrc` with `engine-strict=true` turns the wrong Node into an
   install-time error, and a Node-version check in `cargo xtask verify` (hub leg) and
   `cargo xtask dev-setup` turns it into an actionable message at the point people
   actually hit it (`Node 26.8.1 does not satisfy engines.node ^24.0.0 — brew link
   --overwrite node@24, or install fnm to honor .nvmrc`). Also print the Node version at
   the top of the hub leg so a red log names the culprit immediately.
3. **Document the local convention** (`claude-notes/instructions/` or CLAUDE.md): Node is
   pinned to the current LTS in `.nvmrc`/`engines`; Homebrew users should use `node@24`
   and expect `brew upgrade` to relink the unversioned `node`; a version manager is the
   robust option.

Generalisation: any dependency that arrives via the machine's package manager rather
than the lockfile (Node, wasm-opt, rustup toolchain, tree-sitter CLI) needs an
*enforced* pin or a self-contained config; an advisory pin recurs on the next upgrade.

## Proposed phases (draft)

- Phase 0 — Test plan. A regression test that fails under Node ≥ 25 without the shim:
  a small `*.test.ts` under jsdom asserting `typeof localStorage.getItem === 'function'`
  and `Object.keys(localStorage)` behaviour (branchService relies on key enumeration).
  Verify it fails at HEAD on Node 26 (done informally above; make it a named test).
- Phase 1 — Shim: promote `webstorage-shim.ts` into `hub-client/src/test-utils/`,
  wire it as `setupFiles` in `vitest.config.ts` and `vitest.integration.config.ts`;
  switch the undefined-check to a descriptor check so Node's ExperimentalWarning is not
  triggered by the shim's own read. Run `npm run test:ci` on Node 26 and Node 24.
- Phase 2 — Enforcement: `.npmrc` `engine-strict=true`; Node-version check + version
  banner in `xtask verify` hub leg and `xtask dev-setup`. Test the check's range parsing.
- Phase 3 — Docs: local Node convention note; CLAUDE.md pointer.
- Phase 4 (separate strand, optional) — vitest 5 upgrade across the 11 `^4.0.17`
  packages + `@vitest/coverage-v8`; afterwards the shim is dead code and can be removed.

## Open design questions for the user

1. **Config fix: shim now, vitest 5 later, or vitest 5 only?** Recommendation: land the
   shim (small, no-flag, verified) and file the vitest 5 upgrade as its own strand — a
   major bump across 11 packages is a different risk profile than this bug.
2. **Should a wrong Node make `cargo xtask verify` fail or only warn?** Recommendation:
   fail, with an env-var escape hatch (e.g. `Q2_ALLOW_NODE_MISMATCH=1`) for deliberate
   experiments like the Node 26 runs in this investigation.
3. **`engine-strict=true` in a committed `.npmrc`?** It fails `npm install` on any Node
   outside `^24.0.0` for every contributor, including CI if the runner drifts. That is the
   point, but it is a visible behaviour change — OK?
4. **Local machine remedy**: re-link `node@24` (`brew link --overwrite node@24`, which
   `brew upgrade` will undo again) or install a version manager (fnm/mise) so `.nvmrc`
   is honoured? Not a repo change; asking so the docs recommend what you actually do.
5. **Scope of the shim**: hub-client only, or also the other jsdom-using packages
   (`preview-renderer` 38 files, `q2-preview-spa`, `preview-runtime`, `kanban`)? None of
   their tests reference `localStorage` today, so I'd keep it hub-client-only and note
   the pattern.

## Risks / tradeoffs (draft)

- The shim reaches into `globalThis.jsdom`, an implementation detail of vitest's jsdom
  environment (present in 4.1.8; still present upstream). If it disappears the shim
  no-ops and the Phase 0 regression test fires — acceptable.
- `engine-strict` also blocks Node 25/26 users who have no interest in hub-client tests;
  the `engines` range should track the LTS bump deliberately (next: Node 26 becomes LTS
  in Oct 2026, at which point vitest 5 removes the need for the shim anyway).
- A Node-version check in xtask must not break on non-Homebrew layouts or on Windows;
  read `process.version` via `node -p`, don't inspect paths.

## Notes on the checkout

- Pre-flight `cargo xtask verify --skip-hub-build` was **red**, but not because of HEAD:
  during this session another agent began writing tests in this same checkout
  (`crates/quarto-sass/tests/integration/brand_compile_test.rs`,
  `crates/quarto-brand/tests/integration/font_files_test.rs`,
  `crates/quarto-core/tests/integration/brand_fonts.rs`, mtimes 14:47–14:49) for
  bd-ve916wr8, and those call `brand_to_layers` with a not-yet-implemented signature.
  `main` and the committed HEAD compile. This commit therefore stages **only** the plan
  and investigation files, not `git add -A`.
- The wasm smoke-all failure noted above is a stale local artifact, not a regression.
