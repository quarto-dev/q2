# Upgrade `@automerge/automerge` and `@automerge/automerge-repo` (npm)

**Strand:** bd-d08gpqvu
**Branch:** `braid/bd-d08gpqvu-automerge-npm-upgrade` (worktree under `.worktrees/`)
**Date:** 2026-09-17

## Overview

Survey of the automerge npm packages across the npm workspace and the
upgrade that follows from it. Four workspace packages depend on the
automerge stack: `hub-client`, `ts-packages/quarto-sync-client`,
`ts-packages/quarto-hub-mcp`, and `ts-packages/preview-runtime`.
`resources/extension-build/deno.json` also lists them (mirrored in the
root `deno.lock`).

### Survey (2026-09-17)

| package | installed | newest stable | npm `latest` tag | npm `next` tag |
| --- | --- | --- | --- | --- |
| `@automerge/automerge` | 3.4.1 | **3.5.0** (2026-09-16) | 3.5.0 | 3.4.0-rev-frag-hex.3 |
| `@automerge/automerge-repo` (+ `-network-websocket`, `-react-hooks`, `-storage-indexeddb`, `-storage-nodefs`) | 2.5.6 | **2.5.6** (2026-05-18) | 2.6.0-alpha.3 | 2.6.0-alpha.5 (2026-09-16) |

Observations:

- `@automerge/automerge` 3.4.1 → 3.5.0 is a plain stable minor. Release
  notes: opt-in change "author" metadata (`getAuthor`/`getAuthors`),
  `__proto__` assignment now throws, large-list insertion stack-overflow
  fix, `applyPatch(es)` block/mark fixes, a perf regression fix, a list
  insertion positioning fix. `automerge-repo@2.5.6` declares
  `@automerge/automerge: "2.2.8 - 3"`, so 3.5.0 satisfies it with one copy
  in the tree.
- The automerge-repo family has **no stable release newer than 2.5.6**.
  Upstream has shipped only `2.6.0-alpha.{0,1,2,3,5}` since May, and
  moved the npm `latest` dist-tag onto `2.6.0-alpha.3`. The alpha line
  removes xstate (DocHandle state machine rewritten, PR 618), adds
  `DocumentQuery`, rewrites subdoc handles, raises `engines.node` to
  `>=22.13`, bumps `uuid` to 14 / `bs58check` to 4, and carries a long
  list of robustness fixes (throttle, storage, websocket teardown,
  react-hooks stale results, ephemeral-message delivery in alpha.5).
- Version-range drift inside the workspace: `quarto-sync-client` and
  `preview-runtime` still declared `^2.5.1` / `^2.5.4` for the repo
  packages while `hub-client` / `quarto-hub-mcp` declared `^2.5.6`. The
  lockfile already resolved all of them to 2.5.6; only the declared
  ranges differed.
- `hub-client` pins `-react-hooks` and `-storage-indexeddb` **exactly**
  (`2.5.6`, no caret). That pin dates from commit `083814400f` ("Fix deps
  hopefully", 2026-06-09) with no recorded rationale; it predates that
  commit too (`2.5.1` exact). Kept as-is in this pass — it is harmless
  while the whole family sits on one version.
- Rust side, for context only (out of scope here): the hub uses
  `samod` 0.13.0 from the `quarto-dev/samod` fork (`access-policy`
  branch) with Rust `automerge` 0.11.0. Upstream `samod` 0.14.0 landed on
  crates.io on 2026-09-17. The JS 3.5.0 "author" metadata is opt-in, so
  JS clients on 3.5.0 keep producing changes the 0.11 Rust reader
  understands unless we start calling the new API.

## Decision: stable-only (phase 1) vs. 2.6.0 alpha (phase 2)

Phase 1 is unambiguous: take the stable `@automerge/automerge` minor and
align the drifted ranges. Phase 2 (adopting a 2.6.0 alpha because
upstream tags it `latest`) is a judgment call for the maintainers — the
hub is a production sync server and a 2.6 alpha changes the DocHandle
internals our sync client leans on. Phase 2 is **not** done unless
explicitly approved; this plan records the experiment so the question
can be answered with evidence.

## Checklist

### Phase 1 — stable bump + range alignment

- [x] File strand bd-d08gpqvu; create worktree + topic branch
- [x] `@automerge/automerge` `^3.4.1` → `^3.5.0` in `hub-client`,
      `quarto-sync-client`, `preview-runtime`
- [x] Align `@automerge/automerge-repo*` ranges in `quarto-sync-client`
      (`^2.5.1`/`^2.5.4` → `^2.5.6`) and `preview-runtime` (`^2.5.1` →
      `^2.5.6`)
- [x] `npm install` from the repo root under Node 24; lockfile resolves
      `@automerge/automerge` 3.5.0, repo family stays 2.5.6
- [x] `deno.lock` spec lists synced by hand (the lock holds no automerge resolution entries; matches how `fb1ef319b` regenerated it)
- [ ] Fast gate: ts-packages builds, `hub-client` `tsc -b`, vitest suites
- [ ] Full gate: `cargo xtask verify` (hub-client + WASM legs included)
- [ ] `hub-client/changelog.md` entry (two-commit workflow)
- [ ] Commit, push topic branch, open PR (do not merge)

### Phase 2 — 2.6.0-alpha experiment (evidence for the decision)

- [ ] On a throwaway commit, bump the repo family to `2.6.0-alpha.5`
      (`next`) and record: install result, `tsc -b` result, vitest result
- [ ] Report the result and ask whether to adopt the alpha
- [ ] Revert the throwaway commit unless approved

## End-to-end verification

Recorded here once run: the invocation, the observed output, and a note
that the output was inspected.
