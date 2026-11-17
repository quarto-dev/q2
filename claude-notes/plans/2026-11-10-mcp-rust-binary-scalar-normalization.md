---
title: 'Normalize Rust binary scalar strings (bd-k8hykrcy)'
date: 2026-11-10
description: 'Normalize Rust-authored Automerge binary metadata at the sync-client boundary so MCP output validation and binary deduplication work.'
status: done
braid:
  strand: bd-k8hykrcy
  priority: P1
  labels: [hub-mcp, sync-client]
---

## Overview

[Issue #1104](https://github.com/quarto-dev/q2/issues/1104) is reproducible when a real Rust hub imports a binary file. Its `mimeType` reaches JavaScript as an Automerge `ImmutableString`, which MCP output schemas reject. The persisted `hash` has the same representation and can defeat same-content deduplication.

Normalize both declared scalar fields with `String(value)` as raw binary documents cross the `quarto-sync-client` public DTO boundary. Preserve the stored document and MIME value unchanged; do not patch MCP schemas or recursively normalize Automerge documents.

## Phase 1: regression tests

- [x] Add deterministic `ImmutableString` coverage for initial file callbacks, subsequent binary changes, both binary getters, and same-content deduplication.
- [x] Add MCP output-schema coverage for project connect, file list, and binary reads using scalar-backed binary metadata.
- [x] Run the new tests before implementation and confirm the expected failures.

## Phase 2: implementation

- [x] Add one binary-document normalization helper in `quarto-sync-client` for `content`, primitive `mimeType`, and primitive `hash`.
- [x] Use it at initial ingestion, both change handlers, both getters, and the deduplication comparison.
- [x] Run the regression tests and relevant package suites.

## Phase 3: verification

- [x] Exercise `q2 mcp` against a real Rust hub with an imported PNG; inspect successful connect, list, and binary-read responses.
- [x] Run the required TypeScript builds, lint checks, and pre-commit review.
- [x] Update this plan and record the verification evidence in the strand.

## Verification evidence

The five new sync-client tests failed before implementation: callbacks and getters exposed `ImmutableString`, and same-content deduplication produced a renamed file. The MCP regression failed connect, list, full binary read, and metadata-only read. After implementation:

- `npm test -w ts-packages/quarto-sync-client`: 34 files, 200 tests passed.
- `npm test -w ts-packages/quarto-hub-mcp`: 71 files, 655 tests passed and 4 skipped.
- Both package typechecks and production TypeScript builds passed.
- The hub-client production build passed, and preview-runtime passed 92 tests.
- `cargo xtask lint`, `cargo fmt --check`, and the strict render of this plan passed.
- `cargo nextest run --workspace` ran 8,808 of 19,197 tests before stopping: 8,807 passed and the already-tracked knitr test `capture_classifies_pieces` failed because the local R stack returned `rlang_error` instead of `simpleError` (bd-54odm3rz and bd-qwscag8b). No Rust source changed in this work.

The real-binary check rebuilt the MCP bundle and `q2`, then used these invocations (the Node harness spawned the shown `q2 mcp` command):

```sh
target/debug/q2 hub --project /tmp/pa-502/35937b25-e0ef-44ab-ad2c-0fb238dcdf92/issue-1104.dyiz13/project --port 3917
target/debug/q2 mcp --server ws://127.0.0.1:3917/ws
node /tmp/pa-502/35937b25-e0ef-44ab-ad2c-0fb238dcdf92/issue-1104.dyiz13/repro.mjs /Users/shikokuchuo/r/q2/target/debug/q2
```

The inspected connect and list responses included `pixel.png` with primitive `image/png` metadata. Both binary-read forms succeeded; the full read returned the original PNG image block and this structured metadata:

```json
{
  "path": "pixel.png",
  "type": "binary",
  "mimeType": "image/png",
  "size": 68
}
```
