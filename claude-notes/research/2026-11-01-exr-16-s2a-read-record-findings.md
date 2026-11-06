---
title: 'EXR-16 S2a: Positron reads a q2 record (service-level vitest)'
date: 2026-11-01
description: 'Running q2''s golden record through the real QuartoOutputCacheService on an in-memory file system. Loading is clean; seeding consumes the file and drops every q2 key; three deletion triggers confirmed.'
---

**Question.** What does Positron\'s real cache service do with a record that has extra keys,
inline-expression units and rendered outputs, and is reading q2 records a viable way to seed its
cache? This is part (a) of S2; part (b), the dev-build run, ran on 2026-11-02 and is in
`2026-11-02-exr-16-s2b-s6-seed-and-demo-findings.md`.

**Branch.** `spike/q2-s2-read-record` in `/Users/gordon/src/positron-spike-s2`, commit
`0bfcb904c1`, cut from `upstream/main` at `d42d28a288`. Pushed to Gordon\'s fork (`origin`) on 2026-11-02. The test is
`src/vs/workbench/contrib/positronQuarto/test/browser/quartoOutputCacheQ2Record.vitest.ts`.

## Method

No `quartoOutputCacheService.vitest.ts` existed, and the four existing tests that mention the
service all stub it, so none supplied a file service. A stub was not needed: the test builds the
real `QuartoOutputCacheService` directly with Positron\'s real `FileService` over
`InMemoryFileSystemProvider`, plus minimal stubs for the lifecycle, profile and working-copy
services. What the service deletes, keeps and rewrites is therefore observed, not read off the
source. The input is `crates/quarto-core/tests/fixtures/records/golden.ipynb` (two inline
units, a labeled cell with a stream, an `execute_result` and a `display_data`, and a knitr cell
with a `text/x-quarto-markdown` output) and in-test variants of it.

```bash
Q2_RECORD_FIXTURES=<q2 checkout>/crates/quarto-core/tests/fixtures/records \
  npx vitest --run src/vs/workbench/contrib/positronQuarto/test/browser/quartoOutputCacheQ2Record.vitest.ts
```

Result: 8 passed. A deliberately wrong assertion in two places failed as expected (4 cells, not
3), so the tests are not vacuous. `npm run test:positron:check-ts` reports 573 errors, all in
upstream e2e files and none in the new file.

## Results

| Case | Observed |
|---|---|
| `loadCache` on the golden record | Loads all 4 cells. Cell ids, content hashes and labels survive; outputs map to the right mimes; display metadata (`{a: 1, b: 2}`) lands in `outputMetadata`; the file is **not** deleted. |
| Unknown mime (`text/x-quarto-markdown`) | Kept as an ordinary output item. What the renderer does with it is untested. |
| `quarto_cache_version` of `2`, `"1"`, or missing | `loadCache` returns nothing and **deletes the file**. |
| One code cell without an `outputs` array | `loadCache` returns nothing and **deletes the whole file**. The same record, found by hash scan, is skipped and **kept**. |
| `text/plain` or `text/html` given as an array of lines, or a JSON object | **Stringified, not joined**: `["a\n","b"]` becomes the text `["a\n","b"]`. q2 writes plain strings, so q2 records are unaffected; Jupyter-written records with line arrays would be. |
| Output of an unknown `output_type` | Loads as an output with an empty item list. |
| Hash-scan seed: one matching cell | The **whole record** loads (all 4 cells, inline units included) into the target document; the file is **deleted**. |
| Rewrite after the seed (`flushCache`) | One file at Positron\'s own path. Notebook metadata is exactly `quarto_source`, `quarto_cache_version`, `last_updated`. Each cell keeps only `quarto_cell_id`, `quarto_content_hash` and `quarto_label`; `id`, `execution_count` (now `null`), and every `quarto_kind`, `quarto_lang`, `quarto_engine`, `quarto_kernel`, `quarto_exec_hash` and notebook-level q2 key are gone. A `display_data` output comes back as `execute_result`. |
| Hash scan with no matching cell | Returns nothing; the record is left alone. |

## Go or no-go

- **Reading q2 records into Positron for display: go.** Structure, ids, hashes, labels, stream,
  rich and error-free outputs and display metadata all load, and unknown keys are ignored rather
  than rejected. Keep `quarto_cache_version` at `1` (q2 does) and every code cell\'s `outputs`
  an array.
- **Using Positron\'s cache directory as a q2 store: no-go**, as the epic already decided. A
  seed consumes the file and the rewrite drops all q2 metadata, so a hand-placed q2 record is
  strictly a one-way copy. The hash scan also seeds any document that shares a single cell hash
  with the record, so a placed copy can land on an unrelated document.

## Not tested

- Part (b) (ran later, see the S2b and S6 note): a record dropped into a dev build\'s cache
  directory, and what the editor shows for plain cells, unknown mimes and inline units.
- Whether the document model or output manager prunes unmatched cells (orphans, inline units)
  before a later write. The service itself keeps all four cells; the pruning step is above it.
- Rendering of an empty-item output, of `image/png`, and of `quarto_webview_metadata`.
- Two matching records in one directory: the scan takes the first in directory order, which
  was not exercised.
- `findAndTransferFromUntitled` (the in-memory path), cache cleanup and the rename handler.

## Consequence for the q2 tests

`positron_loader` in `crates/quarto-core/tests/integration/positron_conformance.rs` mirrored
the first three of the deletion triggers above (version, structure, missing `outputs`). Positron\'s
real service now confirms them, so its comment cites this note. The Rust function stays: it
is the only check that runs in q2\'s CI, and the Positron vitest needs a Positron checkout.
