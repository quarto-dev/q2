# hub-client vitest: 23 tests fail under Node 26 — `localStorage` global undefined (bd-lh30hlvd)

**Date:** 2026-09-08
**Braid:** bd-lh30hlvd
**Worktree:** `.worktrees/bd-lh30hlvd-node-version-guard` (branch `braid/bd-lh30hlvd-node-version-guard`, based on `main` @ `b7e7c96a`)
**Status:** Executing — direction agreed with the user on 2026-09-08 (see § Decision).

## Overview

`cargo xtask verify`'s hub-client leg went red on this machine because a routine
`brew upgrade` on 2026-09-04 relinked `/opt/homebrew/bin/node` from node@24 to node 26.8.1.
Node ≥ 25 defines a `localStorage` accessor on `globalThis` (returning `undefined` without
`--localstorage-file`), and vitest 4.x's jsdom environment skips window keys that already
exist on the global unless allowlisted — `localStorage` is not allowlisted until vitest 5.0.0.
So jsdom's storage is never installed and every `localStorage.clear()` dereferences `undefined`.

This is a **recurrence** of the May 2026 incident (`ca6d47c8` pinned Node 24 via `.nvmrc` +
`engines`). The pin was advisory: no version manager on the machine, `engine-strict` off,
and nothing in `xtask` looks at the Node version.

Full root-cause narrative, timeline, candidate table and probe artifacts are preserved in
§ Investigation record below and in `node26-vitest-localstorage-investigation/`.

## Decision (2026-09-08)

The user's top-level decision: **do not move the repo to Node 26 yet**. Keep `engines.node`
at `^24.0.0`. Fix the local machine so the existing pin is honoured, and make the pin
*enforced* in the repo so the next drift fails fast with the cause named. Defer the vitest
shim / vitest 5 upgrade to the deliberate LTS bump (Node 26 becomes LTS in Oct 2026).

Concretely:

1. **Machine**: fnm, initialised in `~/.zprofile`, selects Node 24 from `.nvmrc` on `cd`.
   Homebrew's `node` stays for `bitwarden-cli`; fnm's default is `system`, so nothing
   changes outside pinned projects.
2. **Repo**: a Node-version check in `cargo xtask verify` (fail, with an explicit escape
   hatch) and `cargo xtask dev-setup` (warn + install hints); `engine-strict=true` in a
   committed root `.npmrc`; a short instructions note.
3. **Deferred** (own strand): vitest 5 / jsdom-Storage shim, when `engines` moves to 26.

## Phase A — Local machine (done 2026-09-08)

- [x] `brew install fnm` (1.39.0)
- [x] `fnm install 24` → v24.20.0 (newer than Homebrew's node@24 24.15.0)
- [x] `fnm default system` — outside pinned projects `node` stays Homebrew's 26.8.1
- [x] `~/.zprofile` (new file): `eval "$(fnm env --use-on-cd --version-file-strategy=recursive)"`
      with a comment explaining why `.zprofile` and not `.zshenv` (`/etc/zprofile`'s
      `path_helper` would reorder `/opt/homebrew/bin` ahead of fnm — `/etc/paths.d/homebrew`
      exists on this machine)
- [x] `~/.zshrc`: pointer comment next to the PATH section
- [x] Verified from fresh shells: `zsh -l` in repo → v24.20.0; in `hub-client/` → v24.20.0
      (recursive strategy); outside repo → "Bypassing fnm: using system node" v26.8.1;
      `zsh -l -i` (Terminal.app shape) → v24.20.0, npm 11.19.0
- [ ] Note for the user: shells started *before* `~/.zprofile` existed (including the Claude
      Code session that did this work) keep Node 26 until restarted

## Phase B — Node toolchain check in xtask (TDD)

Module `crates/xtask/src/node_version.rs`. Pure functions unit-tested; process/IO at the edge.

- [x] B0 tests (written first; all 17 failed on `todo!()` stubs, then passed):
  - [x] `parse_node_version("v24.20.0")` → `24.20.0`; rejects garbage; tolerates trailing newline
  - [x] `engines_node_requirement(package_json_text)` → `VersionReq` for `^24.0.0`; error when
        `engines`/`engines.node` missing; error (not silent pass) on an unparsable range
  - [x] `evaluate(requirement, Some(version))` → `Satisfied` / `Mismatch`; `None` → `NotFound`
  - [x] `Mismatch`/`NotFound` render an actionable message naming the found version, the
        requirement, its source file, and the fnm/`.nvmrc` remedy
  - [x] escape hatch: `enforcement(outcome, allow_mismatch: bool)` → `Proceed` / `Fail`
- [x] B1 implement — **decision changed during B1:** Cargo's `semver` crate rejects npm's
      space-separated `>=24 <25`; switched to `nodejs-semver` 5.0.0 (npm's own grammar; adds
      `miette`/`winnow`/`bytecount` to xtask only). Range via `Range::parse`/`satisfies`;
      `node --version` via `crate::util::nested_command` (Windows `.cmd` shims); read
      `engines.node` from `<root>/package.json`
- [x] B2 wire into `verify::run`: before Step 1, when any npm-driven step will run, fail on
      mismatch unless `Q2_ALLOW_NODE_MISMATCH=1` (then print a loud warning); print
      `Node vX.Y.Z satisfies engines.node ^24.0.0` on success so a log names the toolchain
- [x] B3 wire into `dev_setup::run`: warn-only `check_node()` mirroring `check_wasm_opt`,
      with per-platform install hints (fnm / mise / nvm; `.nvmrc` selects the version)
- [x] B4 `cargo nextest run -p xtask`; `cargo xtask lint`; clippy clean under `-D warnings`

## Phase C — `engine-strict` at install time

- [x] C1 root `.npmrc`: `engine-strict=true` with a comment pointing at `engines.node` and
      the instructions note
- [x] C2 verify: under Node 26 `npm install --dry-run` fails with `EBADENGINE`; under Node 24
      `npm ci` in the worktree succeeds (also proves no *dependency* declares an
      incompatible `engines` range — `engine-strict` applies to the whole tree)
- [x] C3 confirm CI is unaffected: every workflow's `setup-node` uses `node-version: '24'`
      (checked 2026-09-08: hub-client-e2e, ts-test-suite ×2, release ×3, deploy-sandboxed-preview)

## Phase D — Docs

- [x] D1 `claude-notes/instructions/node-version.md`: the pin (`.nvmrc` + `engines`), what
      enforces it (xtask check, `engine-strict`), the Homebrew relink trap, fnm setup
      (`.zprofile` vs `.zshenv` on macOS), the escape hatch, and how the pin gets bumped
- [x] D2 `CLAUDE.md`: two-line pointer under hub-client Development
- [x] D3 `.claude/rules/worktrees.md` fresh-worktree bootstrap: mention `npm ci` runs under the
      pinned Node (one line)

## Phase E — End-to-end verification and hand-off

- [x] E1 `cargo xtask verify` in the worktree under Node 26: stops at the preflight (exit 1)
      before any Rust build; `Q2_ALLOW_NODE_MISMATCH=1` warns and continues; with every
      npm-driven step skipped the preflight reports itself skipped (see § End-to-end record)
- [x] E2 `cargo xtask verify` (full: WASM + hub build + every test leg) in the worktree under
      fnm's Node 24.20.0: `✓ All verification steps passed!` — 13,757 Rust tests, hub-client
      1084/119/133, trace-viewer, shared preview-* and hub MCP suites all green (2026-09-08)
- [x] E3 `cargo xtask dev-setup` output under both Nodes inspected
- [x] E4 pre-commit checklist: no `HashMap`/`FxHashMap` in changed Rust; no TODOs; `cargo fmt`
      clean; clippy `-D warnings` clean; `cargo xtask lint` clean; new module's decision logic
      fully unit-tested, IO edges exercised end-to-end (E1/E3)
- [x] E5 deferred strand filed: **bd-s84z961e** (vitest 5 / shim at the Node 26 LTS bump,
      `related:bd-lh30hlvd`); progress comment left on bd-lh30hlvd; bd-lh30hlvd stays open
      until the PR merges
- [ ] E6 report: exact invocations + observed output for E1/E2, the machine changes made,
      and the branch/PR handoff (push only with the user's permission)

## Investigation record (2026-09-08)

### Root cause

Reproduced at `89c580f1` with Node 26.8.1, vitest 4.1.8, jsdom 26:

```
$ cd hub-client && npx vitest run src/hooks/usePreference.test.tsx src/services/branchService.test.ts
TypeError: Cannot read properties of undefined (reading 'clear')
 ❯ src/hooks/usePreference.test.tsx:10:16
 Test Files  2 failed (2)   Tests  12 failed (12)
```

1. Node ≥ 25 defines a `localStorage` accessor on `globalThis`; without `--localstorage-file`
   it warns and returns `undefined`, but `'localStorage' in globalThis === true`. Node 24 has
   no such property (probe output in the investigation README).
2. vitest 4.1.8 jsdom environment (`getWindowKeys` in
   `node_modules/vitest/dist/chunks/index.DC7d2Pf8.js`): `if (k in global) return
   keysArray.includes(k);` — `KEYS` has `"Storage"` but not `"localStorage"`, so jsdom's
   storage is never copied onto the global.
3. Upstream added `localStorage`/`sessionStorage` to the allowlist in **vitest 5.0.0** only
   (verified by unpacking 4.1.9, 4.1.10, 4.1.11, 5.0.0 from npm). vitest-dev/vitest#8757.

### Fix candidates tried (Node 26.8.1)

| Candidate | Result | Notes |
| --- | --- | --- |
| `NODE_OPTIONS=--no-webstorage` | unit suite green (96 files / 1084 tests) | **Node 24 rejects the flag** (`bad option`) — cannot be shared config. |
| `NODE_OPTIONS=--localstorage-file=<tmp>` | green | Substitutes Node's file-backed Storage for jsdom's; persists across runs/workers. Rejected. |
| `PATH` → node@24 | green | Confirms the environment. |
| setupFiles shim via `globalThis.jsdom.window.localStorage` | 6 files / 62 tests green | Flag-free, Node-agnostic; **deferred** by decision (would enable Node 26). Prototype kept in the investigation dir. |

`npm run test:ci` under the `--no-webstorage` candidate: unit 1084/1084, integration 119/119;
one wasm smoke-all failure identical under Node 24 — a stale local WASM (built Sep 1; `813850ee`
on Sep 3 changed include-error semantics). Unrelated.

### Timeline — why "recently"

| Date | Event | Evidence |
| --- | --- | --- |
| 2026-04-28 | Homebrew installs unversioned `node` 25.9.0_2 | `INSTALL_RECEIPT.json` |
| 2026-05-18 | Same symptom; `ca6d47c8` pins Node 24 (`.nvmrc`, `engines`); `node@24` installed 15:41 and linked | commit; receipt |
| 2026-06-01/10 | CI e2e pinned to 24.15.0 for an unrelated Playwright hang, then unpinned to `'24'` | `55fad91d`, `4a446d85` |
| 2026-09-04 11:10 | `brew upgrade` (manual — no autoupdate agent) installs `node` 26.8.1 and relinks `bin/node` over node@24 | receipt; `var/homebrew/linked/node` |
| 2026-09-08 | First `cargo xtask verify` after the upgrade is red; bd-lh30hlvd filed | strand |

`brew uninstall node` is not an option: `bitwarden-cli` depends on the unversioned formula.

### Why the May pin did not hold

- `.nvmrc` acts only through a version manager; none was installed.
- `engines.node` acts only at `npm install`, and only warns unless `engine-strict`
  (`npm install --engine-strict --dry-run` under Node 26 → `EBADENGINE`, verified).
- `xtask verify` / `dev-setup` never checked Node (`dev-setup` checks `wasm-opt`, not `node`).

Generalisation: a dependency that arrives via the machine's package manager rather than the
lockfile (Node, wasm-opt, the rustup toolchain, tree-sitter CLI) needs an *enforced* pin;
an advisory pin recurs on the next upgrade.

### Dependency graph

- **discovered-from** bd-jsvetdea (closed): its pre-flight verify surfaced this; unrelated in substance.
- **related** (incoming) bd-202u5bld (Rust highlight grammar): same-session link, unrelated.
- No `blocks` edges.

### Notes on the original checkout

The investigation was done in the main checkout on `braid/bd-ve916wr8-brand-file-fonts`
(commit `9dd12112`, cherry-picked here as `57360606`); that branch's pre-flight verify was
red only because another session was writing bd-ve916wr8's tests there. When that branch
lands first, `git rebase main` on this branch drops the patch-identical plan commit.

## End-to-end record (2026-09-08, worktree, xtask at this branch)

`cargo xtask verify --skip-rust-build --skip-rust-tests --skip-treesitter-tests` under
Homebrew's Node 26.8.1 (output inspected):

```
━━━ Preflight: Node toolchain ━━━

Error: Node v26.8.1 does not satisfy engines.node ^24.0.0 (package.json).
  This repository pins Node to ^24.0.0 (package.json `engines.node`, mirrored in `.nvmrc`).
  With fnm, mise, or nvm installed, `.nvmrc` selects the pinned version automatically;
  see claude-notes/instructions/node-version.md for setup (and for the Homebrew relink trap).
  To run against this Node anyway, as a deliberate experiment: Q2_ALLOW_NODE_MISMATCH=1
exit=1
```

Same under fnm's Node 24.20.0: `✓ Node v24.20.0 satisfies engines.node ^24.0.0 (package.json)`,
then Step 1 runs. `cargo xtask dev-setup` prints the same message as a `Warning:` plus the
three fnm install lines under Node 26, and the one-line confirmation under Node 24.

`npm install --dry-run` with the new `.npmrc`: Node 26 → `npm error code EBADENGINE`,
exit 1; Node 24 → exit 0. `npm ci` in the fresh worktree under Node 24 → exit 0 (so no
dependency declares an incompatible `engines` range).
