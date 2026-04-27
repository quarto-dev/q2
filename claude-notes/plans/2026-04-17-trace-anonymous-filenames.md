# Fix `<anonymous>` filenames in pipeline trace `astContext`

Beads: `bd-b0f2`

## Overview

The JSON trace files emitted under `.quarto/trace/<format>/latest.json` contain
a `pipeline[].data.ast.astContext.files` array that always holds a single entry
named `"<anonymous>"`. This defeats the purpose of the attribution metadata:
`SourceInfo` nodes in the trace carry a `file_id` that indexes into `files`, so
readers of the trace can't tell which file a block came from.

The real `ASTContext` produced by the parser already knows the right filename
(the full path to the `.qmd`). The bug is in the **trace serializer**, not the
parser: the observer discards `doc_ast.ast_context` and hands the JSON writer a
fresh `ASTContext::anonymous()`.

A related, deeper issue shows up after engine execution: the knitr engine runs
against an intermediate `<stem>.rmarkdown` file, and the reconciled AST mixes
blocks whose original source is the `.qmd` with blocks whose source is the
engine output. The trace should reflect *both* files in `astContext.files`.

## Reproduction

Input file: `/Users/cscheid/today/knitr.qmd`

```qmd
---
title: hello, knitr
engine: knitr
trace: true
---

## Some text

```{r}
#| label: fig-1
#| fig-cap: this is a caption.
cat(1:100)
```
…
```

Run `cargo run --bin q2 -- render /Users/cscheid/today/knitr.qmd --to=html`,
then:

```bash
jq '.pipeline | map({stage, files: (.data.ast.astContext.files // null)})' \
  /Users/cscheid/today/.quarto/trace/knitr/latest.json
```

Every `DocumentAst` stage reports:

```json
"files": [ { "name": "<anonymous>" } ]
```

Expected (for an unmodified knitr doc):

- Before `engine-execution`: one file entry, `"/Users/cscheid/today/knitr.qmd"`.
- After `engine-execution`: **two** file entries — the `.qmd` for kept blocks
  and the intermediate `.rmarkdown` (or `.knit.md`) for blocks inserted by the
  engine.

## Diagnosis

### Root cause #1 — trace serializer throws away the real context

`crates/quarto-core/src/stage/trace.rs:417-432`:

```rust
fn serialize_pandoc_ast(ast: &quarto_pandoc_types::pandoc::Pandoc) -> serde_json::Value {
    let context = pampa::pandoc::ASTContext::anonymous();   // ← bug
    let mut buf = Vec::new();
    match pampa::writers::json::write(ast, &context, &mut buf) { … }
}
```

The JSON writer builds `astContext.files` from `ast_context.filenames`
(`crates/pampa/src/writers/json.rs:1841`). Because the serializer always passes
an `ASTContext::anonymous()` — whose `filenames` is `["<anonymous>"]` — every
traced document ends up with one bogus entry.

Callers of `serialize_pandoc_ast`:

1. `serialize_pipeline_data` at line 376-383 — this runs for every
   `PipelineData::DocumentAst`. It *has* access to `doc.ast_context`
   (`DocumentAst` struct at `crates/quarto-core/src/stage/data.rs:289`) but
   doesn't pass it in.
2. `on_transform_data` at line 184-205 — called from
   `TransformPipeline::execute` (`crates/quarto-core/src/transform.rs:154`).
   This hand-off currently passes only `&Pandoc`, so the context isn't even
   plumbed through the observer interface.

### Root cause #2 — engine execution rebuilds a fresh context

`crates/quarto-core/src/stage/stages/engine_execution.rs:240-252`:

```rust
let source_name = doc_ast.path.display().to_string();
let (executed_ast, new_ast_context, parse_warnings) = pampa::readers::qmd::read(
    result.markdown.as_bytes(),
    false,
    &source_name,      // ← the original .qmd name, not the intermediate file
    &mut std::io::sink(),
    true,
    None,
);
…
Ok(PipelineData::DocumentAst(DocumentAst {
    path: doc_ast.path,
    ast: reconciled_ast,
    ast_context: new_ast_context,  // ← old context is thrown away
    source_context: doc_ast.source_context,
    warnings,
}))
```

Two problems here:

- The filename passed to `pampa::readers::qmd::read` for the engine-produced
  markdown is the **original `.qmd` path**. That's wrong: the markdown was
  produced by knitr from an intermediate `<stem>.rmarkdown` file (this is the
  file whose line numbers the engine errors refer to, and it's what
  `postprocess_markdown` in `knitr/mod.rs:260-277` rewrites back to `.qmd`
  references in the *content*, not in the source-location metadata).
- `doc_ast.ast_context` is dropped. The reconciled AST still contains
  `SourceInfo` nodes pointing at `FileId(0)` for blocks that were *kept from
  the original parse*, but those FileIds now index into `new_ast_context`'s
  `filenames` — which only knows one file, and it's the one the engine-parse
  passed in. So kept blocks silently get misattributed to the engine output.

This matches the pattern already anticipated by the reconcile crate's test
fixtures: `crates/quarto-ast-reconcile/src/lib.rs:78-84` uses `FileId(0)` for
"original" and `FileId(1)` for "executed". The implementation in the rest of
the pipeline isn't holding up its end of that contract.

### Why the rest of the pipeline looks fine

All later stages preserve `doc.ast_context` as-is (metadata merge, user
filters, ast-transforms, etc. don't touch it). So if we fix the two sites
above, the downstream trace entries will pick up the right filenames for free.

## Plan

### Test strategy (TDD, per CLAUDE.md)

The tests should prove three things without requiring an R installation:

1. **Serializer test** — `serialize_pandoc_ast` (or its successor) must honor
   the filename held by an `ASTContext`. A unit test that builds a `Pandoc` +
   `ASTContext::with_filename("foo.qmd")` and asserts the JSON's
   `astContext.files[0].name == "foo.qmd"` is sufficient.
2. **Pipeline trace test** — a pipeline run on a trivial doc (markdown engine,
   no execution) should produce a trace whose `DocumentAst` entries report the
   real input path. Expand the existing
   `test_json_trace_observer_writes_file` style test in `trace.rs` — feed it a
   `PipelineData::DocumentAst` whose `ast_context` was built with
   `with_filename("test.qmd")` and assert the serialized JSON reflects that.
3. **Engine-execution context merge test** — a unit test in
   `engine_execution.rs` using the `MockIncludesEngine` (or a new mock that
   changes a block) to verify that after `run()`, the resulting
   `DocumentAst.ast_context.filenames` contains both the original `.qmd` path
   and the intermediate `.rmarkdown` path. This test can run on any platform
   because the mock engine fakes knitr output.

Each test is written and run first, observed failing in the expected way,
then the fix goes in, then the test is re-run. Full workspace tests
(`cargo nextest run --workspace`) afterward.

### Work items

Phase 1: trace serializer honors the real context

- [x] Write a unit test in `crates/quarto-core/src/stage/trace.rs` that
      constructs a `DocumentAst` with
      `ast_context = ASTContext::with_filename("test.qmd")`, passes it through
      `serialize_pipeline_data`, and asserts the JSON has
      `astContext.files[0].name == "test.qmd"`. Verify it fails.
- [x] Change `serialize_pandoc_ast` to take `&ASTContext` as a parameter.
      Update the `PipelineData::DocumentAst` arm of `serialize_pipeline_data`
      to pass `&doc.ast_context`. Verify the test passes.
- [x] Extend `PipelineObserver::on_transform_data` to take
      `&ASTContext` in addition to `&Pandoc`. Update the caller in
      `TransformPipeline::execute` — threaded as an explicit `&ASTContext`
      param next to `&mut ast`, since transforms don't need it and
      `RenderContext` doesn't currently carry it. Updated implementers
      (`JsonTraceObserver`, `SummaryTraceObserver`) and test fakes.
- [x] Added a second test (`test_on_transform_data_preserves_filenames`)
      covering the observer entry point directly.
- [x] Manual check: re-rendered knitr.qmd and confirmed all `DocumentAst`
      stages and `transform:*` entries carry the real `.qmd` path in
      `astContext.files`.

Phase 2: engine execution preserves and extends the context

- [x] Write a unit test in
      `crates/quarto-core/src/stage/stages/engine_execution.rs` using a mock
      engine. Assert that after `run()`,
      `DocumentAst.ast_context.filenames` contains both the original `.qmd`
      and the intermediate `.rmarkdown`. Verified failing first.
- [x] In `EngineExecutionStage::run`, pass the intermediate filename
      (`<stem>.rmarkdown`) to `pampa::readers::qmd::read` so that the new
      context's `FileId(0)` refers to the intermediate file.
- [x] Merge the two contexts: `merged_ast_context` is a clone of the
      original that appends the intermediate filename + its
      FileInformation. Filenames end up `[<.qmd>, <.rmarkdown>]` and
      `source_context` holds both with proper line breaks / total lengths.
- [x] Add a `remap_file_ids` helper. Ended up simpler than originally
      planned: rather than post-reconcile plan-walking, the helper is a
      plain recursive visitor that shifts every `FileId` in a `Pandoc` AST
      using a caller-supplied mapping function. The engine stage pre-remaps
      the executed AST's `FileId(0)` → `FileId(1)` *before* calling
      `reconcile()`, which keeps kept-block `FileId(0)` unchanged and
      avoids teaching the reconcile crate about plan-aware remapping.
      Added a companion `SourceInfo::remap_file_ids` method in
      `quarto-source-map` for SourceInfo-level remapping (handles
      Original/Substring/Concat/FilterProvenance).
- [x] Added a second test (`test_engine_execution_remaps_new_blocks_to_intermediate`)
      using a mock "appending" engine that adds a new paragraph. Verifies
      that kept blocks carry `FileId(0)` (.qmd) and the appended block
      carries `FileId(1)` (.rmarkdown) in the reconciled AST.
- [x] Manual check: re-rendered knitr.qmd. Trace shows:
      - pre-engine-sugaring: `["<.../knitr.qmd>"]`
      - engine-execution onwards: `["<.../knitr.qmd>", "<.../knitr.rmarkdown>"]`
      with correct line_breaks / total_length for each.

Phase 3: verification and docs

- [x] Re-render `/Users/cscheid/today/knitr.qmd`. Confirmed trace shows:
      - pre-engine-sugaring: `["<.../knitr.qmd>"]` (total_length 219, 27 line breaks)
      - engine-execution onwards: `["<.../knitr.qmd>", "<.../knitr.rmarkdown>"]`
        (engine output 980 bytes, 58 line breaks)
- [x] `cargo nextest run --workspace` — 7444 tests pass (9 new tests added,
      zero regressions, no snapshot changes).
- [x] `cargo xtask verify --skip-rust-tests --skip-hub-tests` — passes
      (Rust workspace build + hub-client WASM build + trace-viewer build
      + trace-viewer tests). Rust & hub-client tests covered separately.
- [x] `cargo xtask lint` — clean.

## Decisions (2026-04-17)

1. **Intermediate filename for knitr:** `<stem>.rmarkdown`. If
   `.knit.md`-based source locations ever become relevant, revisit.
2. **Merge strategy for `ASTContext.filenames` after reconcile:** option (a)
   — build a fresh two-file context in `engine_execution.rs`, and add a
   FileId-remapping helper in `quarto-ast-reconcile` (no filename knowledge
   in that crate).
3. **Other engines.** Markdown is a passthrough and untouched. Jupyter in
   Quarto 2 is text-in/text-out with no intermediate file (see
   `crates/quarto-core/src/engine/jupyter/text_execute.rs::execute_qmd` —
   it parses code blocks directly from the input string and returns modified
   markdown). For jupyter, Phase 2 is effectively a no-op: the post-engine
   parse uses the same `.qmd` filename as the pre-engine parse, so the
   context stays one-file. We still need the FileId-preservation fix so
   kept-block source locations survive the serialize/parse round-trip, but
   `filenames` will only have one entry.
4. **Non-trace consumers of `ASTContext::anonymous()`.** Left as-is. Pampa's
   lua readwrite and pampa's own tests aren't affected by this bug.

## Files touched (expected)

- `crates/quarto-core/src/stage/trace.rs` — serialize with real context,
  update `on_transform_data` signature
- `crates/quarto-core/src/stage/observer.rs` — extend `on_transform_data`
  trait signature
- `crates/quarto-core/src/transform.rs` — pass ast_context through
  `TransformPipeline::execute` → `on_transform_data`
- `crates/quarto-core/src/stage/stages/engine_execution.rs` — use intermediate
  filename, merge contexts after reconcile
- Possibly `crates/quarto-ast-reconcile/src/apply.rs` — if Option (a)/(b) from
  Phase 2 requires FileId remapping
