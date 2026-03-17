# Plan: CI Optimization Steps 1 & 2

## Overview

The q2 test-suite CI workflow ran out of disk on ubuntu-latest (14 GB SSD) during run 23156000998. This plan implements the first two optimization steps from `memory://main/tasks/optimize-q2-ci-workflow-disk-usage`.

Key discovery: `ts-test-suite.yml` (a separate workflow on the same triggers) already handles the WASM/hub-client build. The setup steps in `test-suite.yml` (Node.js, npm ci, Clang/LLVM, wasm-pack) were orphaned when those steps were moved to `ts-test-suite.yml` on Jan 29, 2026.

## Work Items

- [x] Add `endersonmenezes/free-disk-space` action (SHA-pinned to v3, Linux-only) after tree-sitter setup
- [x] Remove orphaned WASM/hub-client setup steps (Node.js, npm ci, Clang, LLVM, wasm-pack, commented-out build steps)
- [x] Remove commented-out hub-client test step
- [x] Validate YAML syntax
- [x] Confirm `ts-test-suite.yml` unchanged
- [x] Create follow-up basic-memory task: consolidate workflows (`memory://main/tasks/consolidate-q2-ci-workflows`)
- [x] Commit, push, create PR (https://github.com/quarto-dev/q2/pull/55)

## Details

### Steps removed from test-suite.yml

| Step | Why safe |
|------|----------|
| Set up Node.js | ts-test-suite.yml has this |
| Install npm dependencies | ts-test-suite.yml has this |
| Set up Clang (Linux) | ts-test-suite.yml has this |
| Set up LLVM (macOS) | ts-test-suite.yml has this |
| Install wasm-pack | ts-test-suite.yml has this |
| Build WASM module (commented) | ts-test-suite.yml runs this actively |
| Build TypeScript packages (commented) | Covered by npm run build:all in ts-test-suite.yml |
| Run hub-client tests (commented) | Commented out, no coverage change |

### Follow-up (out of scope)

Consolidate the two workflows per `claude-notes/plans/2026-01-12-wasm-ci-tests.md`: move WASM build into `test-suite.yml` as a separate parallel job, delete `ts-test-suite.yml`.
