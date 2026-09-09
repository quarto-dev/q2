# cargo xtask verify does not run hub-client lint:css (CI does) (bd-4bu7vwi5)

**Date:** 2026-09-09
**Braid:** bd-4bu7vwi5
**Branch:** `main` @ `3ecabd28f` (investigated in the main checkout, no worktree)
**Pre-flight:** `cargo xtask verify --skip-hub-build` green at HEAD (under Node 24 via fnm; the Homebrew `node` on PATH is v26 and trips the preflight).
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The gap is a single missing step in `crates/xtask/src/verify.rs`, the CI step it should mirror is unambiguous, and the fix is small. The only real decision is scope: close just this hole, or also close the other CI-vs-verify drift the investigation turned up.

## Issue context

Filed 2026-09-09 by Carlos Scheidegger, `chore`, priority 3, labels `hub-client` + `tooling`.

> CI's ts-test-suite workflow runs `npm run lint:css -w hub-client` (scripts/lint-css.mjs, e.g. the no-physical-box-props rule), but `cargo xtask verify` does not, so a CSS change can pass the full local gate and still fail CI. Found on PR #667: a `margin-left: 0` passed verify and failed both "Run test suite" jobs. Add the lint:css step to the verify hub-build leg (it is fast) so the local gate matches CI.

## Dependency graph

- **discovered-from bd-ew0vak6b** (in_progress) — "hub-client q2-preview: editable/read-only toggle in the bottom status bar". That strand's PR is #667 (`feature/bd-ew0vak6b-hub-client-q2-preview`). Its author ran the full local gate, got green, pushed, and CI run 34388639694 failed both `Run test suite` matrix legs at the `Lint hub-client CSS` step. No other edges.
- Context strand (not linked, found via `git log -S`): **bd-owmzraf6** (closed) — "Wire hub-client lint:css into CI". Commit `28afc1e14` added the CI step to `ts-test-suite.yml` and fixed the ConnectionStatusDialog violations. It touched only the workflow file and one CSS file; `verify.rs` was not updated. That is where the drift was introduced: the CI gate got a new step and the local mirror did not.

## What the code looks like today

Confirmed at HEAD (`3ecabd28f`):

- `crates/xtask/src/verify.rs` (14 numbered steps) has no lint:css call. `grep -n 'lint:css' crates/xtask/src/verify.rs` is empty. Its module doc claims it matches "the CI environment as closely as possible".
- `.github/workflows/ts-test-suite.yml`, job `test-suite`, step `Lint hub-client CSS`: `npm run lint:css -w hub-client`, placed right after `npm ci` and before the WASM build, on both `ubuntu-latest` and `macos-latest`.
- `hub-client/scripts/lint-css.mjs` is dependency-free Node; it walks `hub-client/src/**/*.css` and applies four rules (`no-hardcoded-color`, `no-bare-z-index`, `no-outline-none-without-focus-visible`, `no-physical-box-props`) against a grandfather list in `scripts/lint-css-exceptions.json`. Stale exceptions also fail.
- Cost: `npm run lint:css -w hub-client` on main takes about 0.2 s wall clock and is clean.
- `hub-client/design-system.md` names `lint:css` as the enforcement for its rules; `cargo xtask lint`'s `ci-test-suite-unwired` rule only reconciles `test` scripts against CI, so it cannot see this (its own header says so).

### Reproduction

The PR #667 failure is the repro. Saved log excerpt: `claude-notes/plans/verify-lint-css-investigation/pr-667-run-34388639694-lint-css.log`. The offending line:

```
lint:css[no-physical-box-props] src/components/ReplayDrawer.css:377  margin-left: 0;
lint:css: 1 problem(s)
```

A mechanical repro on main: add `margin-left: 0;` to any file under `hub-client/src/`, run `cargo xtask verify` (full leg), observe it passes; run `npm run lint:css -w hub-client`, observe it fails. Not committed as a fixture because the future regression test lives in xtask (see Phase 0), not in a CSS file.

### Wider drift (found while comparing the two gates)

The strand asks about lint:css only, but the same comparison shows other steps CI runs that `verify` does not. Listed so the scope question below is concrete, not to expand the strand unilaterally:

| CI step (workflow / job) | In `verify`? |
| --- | --- |
| `npm run lint:css -w hub-client` (ts-test-suite / test-suite) | **no** — this strand |
| engine-host-deno vitest suite (ts-test-suite / test-suite) | no |
| engine-host-deno `deno test` x3 (ts-test-suite / test-suite) | no (needs Deno) |
| engine-host-deno bundle freshness gate (ts-test-suite / test-suite) | no |
| quarto-api, quarto-automerge-schema, wasm-js-bridge tests (ts-test-suite / workspace-ts-suites) | no |
| q2-preview-spa `test` + `test:integration` (ts-test-suite / workspace-ts-suites) | no (verify only builds it, step 13) |
| kanban demo `test` + `test:integration` (ts-test-suite / workspace-ts-suites) | no |
| pampa `wasm_lua` tests on wasm32 (test-suite / wasm-tests) | no (needs -Zbuild-std + clang) |
| sync-test-harness tests (hub-client-e2e) | no |

Everything `verify` runs, CI also runs, so the drift is one-directional.

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- Phase 0 — Test plan. A unit test in `crates/xtask` that fails at HEAD: the `verify` module must invoke `lint:css` (e.g. a step-list/registry the test can inspect, or at minimum a source-level assertion the way `ci_test_wiring` parses workflows). If the answer to Q3 is "yes", the test instead becomes a new repo-level lint rule that parses `ts-test-suite.yml` and asserts each gating step has a `verify` counterpart or an `EXCUSED` entry.
- Phase 1 — Add the lint:css step to `verify.rs`. Position: with the other fail-fast checks (step 1, next to `cargo xtask lint` + clippy), or at the top of the hub-client leg (step 7) as the strand suggests. Gate it on the same condition as step 7 (`!skip_hub_build`) or on a new `--skip-css-lint`, per Q2. Bump `TOTAL_STEPS` and the module doc.
- Phase 2 — Optional, per Q3: fold the rest of the drift table in, or file it as separate strands.
- Phase 3 — Docs: `CLAUDE.md` "Full Project Verification" step list, `hub-client/design-system.md` enforcement sentence, and a note on bd-owmzraf6's plan that the local mirror now exists.

## Open design questions for the user

1. **Placement.** Run lint:css in step 1 with the other fail-fast lints (fails in 0.2 s before the multi-minute Rust build, matching CI's "right after npm ci" placement), or inside the hub-client leg (step 7) as the strand text says? Step 1 means `--skip-hub-build` still runs it, which is what a CSS-only change wants; step 7 means `--skip-hub-build` skips it, which recreates a smaller version of this hole.
2. **Skip flag.** Does it need its own `--skip-css-lint`, or is it cheap enough to be unconditional (like `cargo xtask lint` and `cargo fmt --check` today)? Note the Node preflight: if it runs unconditionally, `needs_node` must include it, so the "every npm-driven step is disabled" shortcut goes away unless a flag exists.
3. **Scope.** Fix lint:css only (the strand as filed), or also close the other one-directional drift in the table above? The cheap, Node-only ones (quarto-api, automerge-schema, wasm-js-bridge, q2-preview-spa tests, kanban) are a few seconds each; the Deno and wasm32 ones need toolchains verify does not assume today. My recommendation: lint:css here, file one strand for the Node-only test suites, and one for the Deno/wasm32 legs with a "needs toolchain detection" note.
4. **Structural guard.** The drift happened because adding a CI step has no counterpart obligation. Do you want a lint rule (sibling of `ci-test-suite-unwired`) that parses the workflows and fails when a gating `run:` step has no `verify` counterpart, or is a doc rule ("when you add a CI step, add the verify step in the same commit", like the error-docs rules) enough?

## Risks / tradeoffs (draft)

- Unconditional Node use in step 1 interacts with the `needs_node` preflight shortcut; must be handled explicitly or `--skip-*` combinations that today avoid Node will start requiring it.
- If placed under `!skip_hub_build`, the common Rust-only invocation `cargo xtask verify --skip-hub-build` still misses CSS lint. Low risk in practice (Rust-only changes rarely touch CSS) but it is the same class of gap.
- A structural lint (Q4) is real work and needs an `EXCUSED` list for the Deno/wasm32 steps from day one; without one it would fail immediately on main.
