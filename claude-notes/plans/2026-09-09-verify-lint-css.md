# cargo xtask verify does not run hub-client lint:css (CI does) (bd-4bu7vwi5)

**Date:** 2026-09-09
**Braid:** bd-4bu7vwi5
**Branch:** `main` @ `3ecabd28f` (investigated in the main checkout, no worktree)
**Pre-flight:** `cargo xtask verify --skip-hub-build` green at HEAD (under Node 24 via fnm; the Homebrew `node` on PATH is v26 and trips the preflight).
**Status:** Approved 2026-09-09 — implementing. Decisions: step 1 placement; add `--skip-css-lint`; lint:css only (wider drift filed as bd-l7mcijfe + bd-ya2nacaa); doc rule, no structural lint.

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

## Work items

Phase 0 — tests first (unit tests in `crates/xtask/src/verify.rs`):
- [x] `runs_css_lint`: on by default, off with `skip_css_lint`, still on under `--skip-hub-build` (the exact hole from PR #667).
- [x] `needs_node`: extracted from the inline expression; css lint counts as npm-driven, so "everything else skipped" still preflights Node.
- [x] `css_lint_command_matches_ci_workflow`: the local `npm` args joined equal a `run:` line in `ts-test-suite.yml`, so the two gates cannot drift silently.
- [x] Run, confirm failure (functions/field do not exist yet).

Phase 1 — implementation:
- [x] `VerifyConfig.skip_css_lint` + `--skip-css-lint` clap flag in `main.rs`.
- [x] Step 1 runs `npm run lint:css -w hub-client` from the project root, after `cargo xtask lint` and before clippy (fail-fast, 0.2 s). `TOTAL_STEPS` unchanged (it lives inside step 1).
- [x] Module doc + `Verify` clap doc updated.
- [ ] Tests green; `cargo nextest run --workspace`; `cargo xtask verify --skip-hub-build`.

Phase 2 — end-to-end:
- [x] Inject `margin-left: 0;` into a hub-client CSS file, run `cargo xtask verify --skip-hub-build`, observe step 1 fails on lint:css; revert; observe green. Record the invocation + output here.

End-to-end record (2026-09-09, main + this change, Node 24 via fnm). Appended
`.bd-4bu7vwi5-e2e { margin-left: 0; }` to `hub-client/src/components/ReplayDrawer.css`
and ran, with every other step skipped:

```
cargo xtask verify --skip-rust-build --skip-rust-tests --skip-treesitter-tests \
  --skip-hub-build --skip-hub-tests --skip-ts-packages-build --skip-trace-viewer-build \
  --skip-trace-viewer-tests --skip-shared-package-tests --skip-q2-preview-spa-build \
  --skip-hub-mcp-tests
```

Observed (step 1, before any Rust compile):

```
lint:css[no-physical-box-props] src/components/ReplayDrawer.css:491  margin-left: 0;
lint:css: 1 problem(s)
Error: hub-client lint:css failed (see hub-client/design-system.md)
```

Reverted the file; same invocation printed `✓ hub-client lint:css clean` and
`✓ All verification steps passed!`. With `--skip-css-lint` added the preflight
printed `(skipped — every npm-driven step is disabled)` and step 1 printed
`↳ Skipping hub-client lint:css`. Output inspected by hand.

Phase 3 — docs:
- [x] `CLAUDE.md` "Full Project Verification": add the lint:css step and the rule *when you add a CI step, add its verify counterpart in the same commit*.
- [x] `hub-client/design-system.md`: enforcement sentence names `cargo xtask verify` too.
- [x] Follow-ups filed: bd-l7mcijfe (Node-only suites), bd-ya2nacaa (Deno/wasm32/harness legs).

## Design questions (answered 2026-09-09)

Answers: (1) step 1; (2) add `--skip-css-lint`; (3) lint:css only, wider drift filed separately; (4) docs are enough.

1. **Placement.** Run lint:css in step 1 with the other fail-fast lints (fails in 0.2 s before the multi-minute Rust build, matching CI's "right after npm ci" placement), or inside the hub-client leg (step 7) as the strand text says? Step 1 means `--skip-hub-build` still runs it, which is what a CSS-only change wants; step 7 means `--skip-hub-build` skips it, which recreates a smaller version of this hole.
2. **Skip flag.** Does it need its own `--skip-css-lint`, or is it cheap enough to be unconditional (like `cargo xtask lint` and `cargo fmt --check` today)? Note the Node preflight: if it runs unconditionally, `needs_node` must include it, so the "every npm-driven step is disabled" shortcut goes away unless a flag exists.
3. **Scope.** Fix lint:css only (the strand as filed), or also close the other one-directional drift in the table above? The cheap, Node-only ones (quarto-api, automerge-schema, wasm-js-bridge, q2-preview-spa tests, kanban) are a few seconds each; the Deno and wasm32 ones need toolchains verify does not assume today. My recommendation: lint:css here, file one strand for the Node-only test suites, and one for the Deno/wasm32 legs with a "needs toolchain detection" note.
4. **Structural guard.** The drift happened because adding a CI step has no counterpart obligation. Do you want a lint rule (sibling of `ci-test-suite-unwired`) that parses the workflows and fails when a gating `run:` step has no `verify` counterpart, or is a doc rule ("when you add a CI step, add the verify step in the same commit", like the error-docs rules) enough?

## Risks / tradeoffs (draft)

- Unconditional Node use in step 1 interacts with the `needs_node` preflight shortcut; must be handled explicitly or `--skip-*` combinations that today avoid Node will start requiring it.
- If placed under `!skip_hub_build`, the common Rust-only invocation `cargo xtask verify --skip-hub-build` still misses CSS lint. Low risk in practice (Rust-only changes rarely touch CSS) but it is the same class of gap.
- A structural lint (Q4) is real work and needs an `EXCUSED` list for the Deno/wasm32 steps from day one; without one it would fail immediately on main.
