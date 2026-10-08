# cargo xtask verify: lint:css fails because `-w hub-client` also selects the nested vscode-sync-experiment workspace (bd-verify-css-lint-nested-ws-r0dd5vdk)

**Date:** 2026-10-08
**Braid:** bd-verify-css-lint-nested-ws-r0dd5vdk
**Branch:** `braid/bd-verify-css-lint-nested-ws-r0dd5vdk-css-lint-nested-ws` (topic branch in the main checkout, not a worktree, at the user's request), based on `main` @ `3d3360ab6`
**Status:** Design agreed 2026-10-08 (see Decisions). Implemented; PR open for CI.

## Triage verdict

**Ready to design.** The bug reproduces at HEAD and its cause is confirmed. CI already worked around the failure. The local gate was missed because the test that should keep CI and `verify` in sync matches by substring. What's left to decide is which fix to use and how wide the regression guard should be.

## Issue context

Filed 2026-10-08 (P1 bug), during pre-flight verify for bd-x30aq7ae. On a clean `main`, step 1 of `cargo xtask verify` fails as follows:

```
lint:css: clean
npm error workspace quarto-hub-vscode@0.0.1
npm error location .../hub-client/vscode-sync-experiment
npm error Missing script: "lint:css"
```

`crates/xtask/src/verify.rs:35` runs `npm run lint:css -w hub-client`. npm resolves `-w <path>` to *every* workspace at or below that path. Commit 909eada30 (2026-10-02, "Add vscode-sync-experiment") registered `hub-client/vscode-sync-experiment` as a root workspace, so it is now nested inside hub-client. Workaround: `--skip-css-lint`.

## Dependency graph

- **discovered-from**: bd-x30aq7ae, a frontmatter YAML parse-error diagnostics bug that has nothing to do with this one. It only matters as the session in which verify failed. It does not inform the fix.
- No `blocks` or `related` edges. Every local `cargo xtask verify` run that doesn't pass `--skip-css-lint` hits this, which is what makes it urgent.

## What the code looks like today

Reproduced at HEAD with Node v24.20.0 / npm 11.19.0. The transcript is in `verify-css-lint-nested-ws-investigation/repro.txt`. Key findings:

1. **npm can't tell a name from a path here.** `npm pkg get name -w hub-client` returns *both* `hub-client` and `quarto-hub-vscode`. The package name happens to equal the directory name, so "select by name" (the issue's first candidate) does **not** help unless the name changes.
2. **CI already patched this, but only in CI.** Commit b8ce24b7c (2026-10-03, Gordon Woodhull) changed `.github/workflows/ts-test-suite.yml:147` to `npm run lint:css -w hub-client --if-present`. `verify.rs` kept the bare form.
3. **The sync test let the two drift apart.** `css_lint_command_matches_ci_workflow` (`verify.rs:~886`) asserts `workflow.contains("run: npm run lint:css -w hub-client")`. That is a substring match, so the CI line with `--if-present` appended still passes. The test was written to stop exactly this kind of drift (bd-4bu7vwi5), and it didn't.
4. **Working forms:** `--if-present` (exit 0, hub-client's lint still runs) and `--prefix hub-client` (exit 0, runs only hub-client).
5. **Bug class, other `-w hub-client` call sites:**
   - `.github/workflows/hub-client-e2e.yml:188-189`: `npm run build:sandboxed --workspace hub-client --if-present` and `npm run build --workspace hub-client --if-present`. Because of `--if-present` these don't fail, but `build` **also runs `quarto-hub-vscode`'s `node esbuild.mjs`**. That is extra CI work, and the e2e job could break if the experiment's build breaks.
   - `AGENTS.md:530` documents `npm run lint:css -w hub-client`. That doc is now wrong: copying the command from it fails.
   - The other xtask npm call sites (`verify.rs` step 6 and `build_all.rs` ts-packages build) pass `-w ts-packages/<pkg>` along with `--if-present`, and nothing is nested under them today. hub-client's build and tests run with `cwd=hub-client`, without `-w`, so they're unaffected.
   - The root `build`/`typecheck` scripts use `--workspaces --if-present` on purpose (all workspaces), so they're fine.

## Proposed phases (draft)

- **Phase 0, tests first.** Tighten `css_lint_command_matches_ci_workflow` so it matches the exact `run:` line, not a prefix. Today it would fail against the current CI (`--if-present` suffix), which demonstrates the drift. If Q2 picks it, add a guard (see Q2).
- **Phase 1, fix the local invocation.** Change `CSS_LINT_ARGS` to the chosen form (see Q1), and update the CI line so the two match exactly.
- **Phase 2, bug-class sweep.** Decide what to do about hub-client-e2e.yml's `build --workspace hub-client` fan-out. Fix `AGENTS.md:530`.
- **Phase 3, verify.** Run `cargo xtask verify --skip-hub-build` (without `--skip-css-lint`) and confirm it is green, and that a seeded CSS violation still fails it (so `--if-present` or `--prefix` didn't silently skip the lint).

## Decisions (2026-10-08, with the user)

1. **Fix shape:** (b), `--prefix hub-client` in both verify and CI. Moving the experiment (c) is a follow-up, filed as bd-prfbxrth, and will be raised with Elliot on the PR.
2. **Regression guard:** (i), a new `nested-npm-workspace` xtask lint (`crates/xtask/src/lint/nested_npm_workspaces.rs`), with `hub-client/vscode-sync-experiment` allowlisted pending bd-prfbxrth. The PR message flags this for the follow-up. The sync test is also tightened to an exact-line match.
3. **e2e build fan-out:** acceptable for now. `hub-client-e2e.yml:189` is unchanged.
4. **Ownership:** Gordon's earlier CI fix is noted on the strand. The PR is where we talk to Elliot.

## Work items

- [x] Phase 0: `css_lint_command_matches_ci_workflow` now matches the whole line. It fails against the old CI line (`--if-present`), as expected.
- [x] Phase 0: unit tests for the nested-workspace lint (sibling, literal-nested, glob-nested, allowlisted, no manifest).
- [x] Phase 1: `CSS_LINT_ARGS` and the CI step both use `npm run lint:css --prefix hub-client`.
- [x] Phase 2: the lint is registered in `lint/mod.rs`, sharing `workspace_dirs`/`workspace_globs` with `ci_test_wiring`. `AGENTS.md` is updated.
- [x] Sanity: with the allowlist entry removed, `cargo xtask lint` flags `package.json:8`. A seeded `margin-left: 0` in `hub-client/src/App.css` makes `npm run lint:css --prefix hub-client` exit 1.
- [x] `cargo xtask verify` is green locally, with one exception: `smoke_all` has 9 environment-only failures (R lacks `gt`/`flextable`, and the venv has `great_tables` without `pandas`/`polars`). Steps 6–14 were rerun with `--skip-rust-tests` after an `npm ci` to refresh stale `node_modules`, and all passed.
- [ ] PR open and CI green.

## Original design questions (answered above)

1. **Fix shape.** Which of these do you want?
   - (a) `--if-present`, mirroring CI. It's cheap and stays in sync with b8ce24b7c, but if hub-client's `lint:css` script were ever renamed or removed, both gates would silently pass.
   - (b) `--prefix hub-client` (or `cwd=hub-client`) in both verify and CI. This targets exactly one package, and a missing script fails loudly.
   - (c) Move `vscode-sync-experiment` out from under `hub-client/` (e.g. `ts-packages/` or a top-level `experiments/`). This removes the nesting for every `-w hub-client` caller at once, but it's a change to Elliot's layout and needs their sign-off.

   My lean is (b) for the lint, plus (c) as a follow-up if Elliot agrees.
2. **Regression guard.** Which guard should we add?
   - (i) An xtask lint (alongside `ci_test_wiring.rs`) that fails when one root workspace path is nested inside another.
   - (ii) Just the exact-match sync test from Phase 0.

   (i) catches the whole class, including e2e's fan-out, but if we keep the experiment where it is it needs an allowlist.
3. **e2e build fan-out.** Should `hub-client-e2e.yml:189` stop building `quarto-hub-vscode`, either with `--prefix` or with the move from (c)? Or is the extra esbuild run acceptable?
4. **Ownership.** Gordon already fixed CI on 2026-10-03. Should this strand note that, or ping Gordon or Elliot before we change their CI line or layout?

## Risks / tradeoffs (draft)

- With `--prefix`, npm runs the script with hub-client as its own root. `scripts/lint-css.mjs` currently passes that way (repro.txt), but if it ever imports hoisted deps, resolution still works, because node walks up to the root `node_modules`.
- Making the sync test an exact match is stricter, so any future cosmetic edit to the CI line will need a matching xtask edit. That's the intent.
- Option (c) touches someone else's in-flight experiment, so coordinate before moving it.

## Pre-flight verify (2026-10-08)

`cargo xtask verify --skip-hub-build --skip-css-lint` on this branch: everything passed (15989 of 15990 tests) except `quarto::integration smoke_all::smoke_all`. All 19 smoke-all failures are in `typst/margin-layout/` and `typst/orange-book*`, and every one is caused by this machine's setup, not by this strand:
- knitr: R 4.6 can't load `cairo.so` or `R_X11.so`, because `/opt/X11/lib/libSM.6.dylib` and `libXrender.1.dylib` are missing (XQuartz isn't installed), so `dev.control()` has no graphics device.
- jupyter: `ModuleNotFoundError: No module named 'great_tables'` in the active venv.
