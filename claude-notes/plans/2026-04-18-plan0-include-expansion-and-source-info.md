# Plan 0: Pre-Engine Include Expansion & Engine SourceInfo

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Nothing
**Blocks:** Plans 1a, 1b, 2, 3, 4 (all TS engine extension work)
**Estimated sessions:** 2-3

## Overview

Establish two prerequisites for TS engine extensions (and improve correctness
for all engines):

1. **Include shortcode expansion before engine execution** — Without this,
   `{{< include file.qmd >}}` containing code cells is broken: the included
   code never reaches the engine. Quarto 1 resolves includes pre-engine (in
   TypeScript, at the text level). q2 currently resolves ALL shortcodes
   post-engine (in `ShortcodeResolveTransform`, stage 6). This plan adds a
   new pre-engine stage that resolves include shortcodes at the AST level.

2. **SourceInfo for the text engines receive** — Quarto 1 passes a
   `MappedString` (string + source provenance) to engines. q2's engine
   interface currently passes a bare `&str` with no provenance. This plan
   adds `SourceInfo` to `ExecutionContext`, constructed by having the QMD
   writer track which AST node produced each byte range in its output. This
   gives engines (or q2's error handling) the ability to map error positions
   back to original source files — including through include boundaries.

### Why this must come before the TS engine plans

The TS engine protocol design depends on knowing:
- Whether the engine receives text with includes already expanded (yes)
- Whether source mapping data is available for the engine's input (yes)
- Who is responsible for error position remapping (open — see below)

Without Plan 0, the protocol would be designed around the wrong assumptions.

## Context: How Quarto 1 handles this

In Quarto 1, shortcodes are expanded in **two phases**:

- **Pre-engine (TypeScript layer):** `include` shortcodes are resolved at the
  text level in `projectResolveFullMarkdownForFile()` → `expandIncludes()`.
  The result is a `MappedString` that tracks which ranges came from which
  files. This expanded text is what the engine receives.

- **Post-engine (Lua filter pipeline):** All other shortcodes (`meta`, `var`,
  `env`, custom extension shortcodes) survive engine execution unchanged and
  are expanded during the `quarto-pre` Lua filter phase in Pandoc.

Quarto 1's `MappedString` provenance is available to engines but **barely
used for error remapping**: Jupyter passes errors through raw, knitr remaps
only one specific R error pattern, Julia builds sourceRanges but doesn't
remap errors on the way back. Only OJS actually uses it for error line
translation.

Our goal is **parity with Quarto 1**: SourceInfo exists, is correct, is
available to engines, and is thoroughly tested — but systematic error
remapping is deferred as an open question.

## Pipeline change

```
Current:
  Parse → MetadataMerge → PreEngineSugaring → EngineExec → CompileThemeCss →
    UserFilters(pre) → AstTransforms(ALL shortcodes) → UserFilters(post) →
    RenderHtmlBody → ApplyTemplate

After Plan 0:
  Parse → MetadataMerge → IncludeExpansion(NEW) → PreEngineSugaring → EngineExec →
    CompileThemeCss → UserFilters(pre) → AstTransforms(non-include shortcodes) →
    UserFilters(post) → RenderHtmlBody → ApplyTemplate
```

**Ordering rationale:** `IncludeExpansion` must run before `PreEngineSugaring`
because included files may contain cross-references that `PreEngineSugaring`
needs to index (it seeds `RefTypeRegistry` and builds `CrossrefIndex`). Both
must run before `EngineExec`.

**Note on non-QMD files:** Plan 1c restructures the pipeline entry point so
that `claimsFile` (engine detection Phase 1) runs before `ParseDocument`.
If an engine claims a non-QMD file (e.g., `.jl` percent script), the engine
converts it to QMD text via `markdownForFile`, and that text enters the
pipeline at `ParseDocument`. Plan 0's work (include expansion, QMD writer
SourceInfo) applies equally regardless of whether the input started as QMD
or was converted from another format — by the time include expansion runs,
the AST is the same either way.

## Prerequisites

### Prerequisite: `Block::source_info()` and `Inline::source_info()` accessors

Every `Block` variant's inner struct has `pub source_info: SourceInfo`, but
there is no enum-level accessor. The codebase has **4 independent copies** of
a `get_block_source_info` / `block_source_info` free function scattered
across pampa (`writers/incremental.rs`, `pandoc/treesitter.rs`,
`pandoc/treesitter_utils/postprocess.rs`, `lua/diagnostics.rs`) plus one
in a test file (`tests/incremental_writer_investigation.rs`). A similar
`get_inline_source_info` exists in `lua/diagnostics.rs`.

**Action (separate commit before Phase 0A/0B):**
1. Add `impl Block { pub fn source_info(&self) -> &SourceInfo }` in
   `quarto-pandoc-types/src/block.rs` — match on all variants. Every
   variant's inner struct has a `source_info` field, so this always
   returns a reference.
2. **Restructure `Inline::Attr`** to include a `source_info: SourceInfo`
   field. Currently `Inline::Attr(Attr, AttrSourceInfo)` is a tuple
   variant where `Attr` is `(String, Vec<String>, LinkedHashMap<...>)` —
   it has no `SourceInfo`. `AttrSourceInfo` tracks per-component source
   locations and has a `combined()` method that merges them into a single
   `SourceInfo`. Add a precomputed `source_info: SourceInfo` field
   (computed from `AttrSourceInfo::combined()` at construction time).
   Update all `Inline::Attr` construction sites.
3. Add `impl Inline { pub fn source_info(&self) -> &SourceInfo }` in
   `quarto-pandoc-types/src/inline.rs` — match on all variants. With the
   `Attr` restructuring, every variant now has a `source_info` field to
   borrow from, giving both enums the uniform `-> &SourceInfo` API.
4. Replace all 4+ duplicate free functions with calls to the new methods.
5. Run `cargo nextest run --workspace` to verify no regressions.

This is a standalone cleanup that Phase 0B (QMD writer tracking) and
Phase 0A (AST walking) both benefit from.

## Commit order

Four commits, in this order:

1. **Prerequisite** — `Inline::Attr` restructure + `source_info()` accessors + dedup
2. **Phase 0B** — QMD writer `write_with_source_info` + its unit tests
3. **Phase 0C wiring** — `ExecutionContext` fields + `serialize_ast_to_qmd` update + single-file SourceInfo tests
4. **Phase 0A** — Include expansion stage + all include tests (unit tests, integration tests, and tests that verify SourceInfo through include boundaries)

Phases 0B and 0C are include-independent SourceInfo infrastructure. They
land first so that when Phase 0A adds include expansion, the full
SourceInfo chain (include → QMD writer → ExecutionContext → map_offset)
can be tested end-to-end in the same commit.

## Work Items

### Phase 0A: Include shortcode expansion stage (commit 4)

New pipeline stage that resolves `include` shortcodes in the AST before
engine execution.

**Note:** q2 does not currently have an `include` shortcode handler. The
existing `ShortcodeResolveTransform` handles `meta` (built-in Rust handler)
and Lua-based shortcodes, but not `include`. This phase implements include
handling from scratch.

- [x] Create `crates/quarto-core/src/stage/stages/include_expansion.rs`:
  ```rust
  pub struct IncludeExpansionStage;
  ```
  Implements `PipelineStage`. Input/output: `DocumentAst`.

- [x] Implement AST walking to find include shortcodes:
  - Walk all blocks looking for `Paragraph` nodes whose sole inline content
    is an `Inline::Shortcode` where `name == "include"`
  - **Block-level only:** Quarto 1 only expands includes that occupy an
    entire line (`isBlockShortcode` in `parse-shortcode.ts` uses regex
    `^\s*{{< ... >}}\s*$`). The AST-level equivalent is: the shortcode is
    the only child of a `Paragraph`. If an include shortcode appears inline
    among other inlines (e.g., `text {{< include f.qmd >}} more`), leave it
    in place — `ShortcodeResolveTransform` will encounter it later and can
    warn or pass it through. This matches Quarto 1 behavior where inline
    includes are silently not expanded.
  - Extract the file path from the shortcode's first positional argument

- [x] Implement include resolution using the **parse-then-remap pattern**
  (same approach as `EngineExecutionStage` at engine_execution.rs:267-311):

  1. Resolve the included file path relative to the including file's directory
  2. Read the included file via `ctx.runtime.file_read(&path)` — use the
     `SystemRuntime` trait (not `std::fs::read`) so this works in WASM contexts
  3. Parse the included file with pampa (`readers::qmd::read`). This creates
     a fresh `ASTContext` where the included file is `FileId(0)`.
  4. **Remap FileIds**: The main document already uses `FileId(0)` (and
     possibly higher for earlier includes). Register the included file in
     the main document's `ast_context.source_context` to get a new `FileId`
     (e.g., `FileId(N)`). Then call `remap_file_ids` on the parsed AST to
     shift `FileId(0) → FileId(N)`. Use the existing
     `quarto_ast_reconcile::remap_file_ids` or the `SourceInfo::remap_file_ids`
     method.
  5. **Register in BOTH SourceContexts on DocumentAst** (they serve different
     purposes and both need the included file):
     - `doc_ast.ast_context.source_context` — carry over the `FileInformation`
       from the parsed file's `ASTContext` (needed for `map_offset` line/column
       resolution). Use `add_file_with_info` if `FileInformation` is available,
       otherwise `add_file`.
     - `doc_ast.source_context` — register with `add_file(path, Some(content))`
       so ariadne can render error snippets from included files.
     - Both registrations must use the same `FileId(N)`.
  6. Merge the included file's `ast_context.filenames` into the main document's
     `ast_context.filenames`.
  7. Replace the `Paragraph` containing the shortcode with the included
     file's blocks (after stripping the included file's YAML frontmatter —
     i.e., take `parsed.blocks` and discard `parsed.meta`)

- [x] Handle recursive includes:
  - After splicing, re-walk the newly inserted nodes for more include shortcodes
  - Maintain a set of files currently being included (detect circular includes)
  - Error on circular includes with a clear diagnostic

- [x] Handle edge cases:
  - Missing included file → diagnostic error (not a panic)
  - Include path outside project directory → warning
  - Include of a file with YAML frontmatter: Quarto 1 strips the frontmatter
    of included files. Match this behavior. (The included file's YAML is
    parsed but discarded; only its body content is spliced.)

- [x] Wire into pipeline in `pipeline.rs`:
  - Insert `IncludeExpansionStage` between `MetadataMergeStage` and
    `PreEngineSugaringStage` (before `EngineExecutionStage`). Include
    expansion must precede PreEngineSugaring because included files may
    contain cross-references that need indexing.
  - `ShortcodeResolveTransform` in `AstTransforms` continues to handle
    `meta`, `var`, `env`, Lua shortcodes — it simply won't encounter any
    include shortcodes (they're already resolved)

- [x] Tests:
  - Unit test: simple include — paragraph with shortcode replaced by included
    file's blocks
  - Unit test: recursive include — file A includes file B which includes file C
  - Unit test: circular include — A includes B includes A → error diagnostic
  - Unit test: missing file → error diagnostic, not panic
  - Unit test: included file's AST nodes have SourceInfo pointing to the
    included file (correct FileId, byte offsets)
  - Unit test: include inside a code block is NOT expanded (it's literal text
    in CodeBlock.text, not a Shortcode node)
  - Unit test: block-level include (paragraph with only the shortcode) →
    included blocks replace the paragraph
  - Unit test: inline include (shortcode among other inlines in a paragraph)
    → shortcode is NOT expanded, left in place for ShortcodeResolveTransform
    (matches Quarto 1 behavior where only whole-line includes are expanded)
  - Unit test: included file with YAML frontmatter → frontmatter stripped,
    only body blocks spliced
  - Integration test: document with `{{< include >}}` containing a code cell
    → after include expansion, the code cell's CodeBlock is present in the AST
  - Integration test (end-to-end SourceInfo through includes): full pipeline
    with include → engine receives text → SourceInfo maps byte offset in
    engine input back to included file
  - Integration test: verify `map_offset` works for a code block from an
    included file (offset in serialized QMD → correct file + line in the
    included source)

### Phase 0B: QMD writer produces SourceInfo (commit 2)

Extend the QMD writer to build a `SourceInfo::Concat` that maps byte ranges
in the serialized output to the `source_info` of the AST nodes that produced
them.

**Quarto 1 reference:** Quarto 1 doesn't need this because it does include
expansion at the text level (producing a MappedString directly). In q2,
include expansion happens at the AST level, and the engine receives
serialized QMD — so the serializer must construct the provenance.

- [x] Add `write_with_source_info` to pampa's QMD writer:
  ```rust
  // New public API — owns buffer, returns bytes + SourceInfo
  pub fn write_with_source_info(
      pandoc: &Pandoc,
  ) -> Result<(Vec<u8>, SourceInfo), Vec<DiagnosticMessage>>
  ```
  The existing `write(&Pandoc, &mut impl Write)` is unchanged. All ~19
  other callsites are unaffected.

  The new function owns a `Vec<u8>` internally so it can read `buf.len()`
  at block boundaries. It calls a `write_impl_tracked` variant of the
  15-line top-level loop that records `buf.len()` before/after each
  `write_block` call. The entire `write_block` → `write_inline` → 40
  internal helper tree is shared and untouched.

- [x] Track provenance for the **entire output** with no gaps:

  The Concat must tile the full output buffer so that `SourceInfo::concat()`
  (which computes cumulative `offset_in_concat` values) produces correct
  offsets. Any gap would shift all subsequent pieces, causing lookups by
  engine-reported byte offsets to land in the wrong piece.

  `write_impl_tracked` works as follows:

  ```rust
  let mut pieces = Vec::new();

  // Track YAML frontmatter as a single piece
  let meta_start = buf.len();
  let mut need_newline = write_config_value_meta(&pandoc.meta, buf, ctx)?;
  let meta_len = buf.len() - meta_start;
  if meta_len > 0 {
      pieces.push((pandoc.meta.source_info.clone(), meta_len));
  }

  // Track each block — include preceding blank line in measurement
  for block in &pandoc.blocks {
      let start = buf.len();
      if need_newline { writeln!(buf)?; }
      write_block(block, buf, ctx)?;
      pieces.push((block.source_info().clone(), buf.len() - start));
      need_newline = true;
  }

  Ok(SourceInfo::concat(pieces))
  ```

  By measuring each block from **before** the separating blank line, the
  pieces tile the entire buffer with no gaps. The blank line between blocks
  is attributed to the following block (at worst one line off within a
  block, which is acceptable). YAML frontmatter is tracked via
  `pandoc.meta.source_info`.

  **Known limitation:** After `MetadataMergeStage`, `pandoc.meta.source_info`
  may be `SourceInfo::default()` due to a pre-existing bug in
  `MergedConfig::materialize()` that drops map container source_info
  (tracked as `bd-2mxo`). This means byte offsets landing in the YAML
  frontmatter region of the serialized QMD will resolve to "origin unknown"
  rather than pointing to the actual frontmatter location. Individual
  metadata scalar values retain their source_info, but the container does
  not. Fixing this is orthogonal to Plan 0.

  Per-top-level-block is sufficient for the engine use case: engine errors
  report line numbers, lines fall within blocks, and blocks carry SourceInfo
  pointing to their origin file (including through include boundaries).
  Finer granularity (per-inline) can be added later if needed by
  instrumenting the internal write functions.

  **Accuracy note:** Code block content is written verbatim
  (`write!(buf, "{}", codeblock.text)`), so within-block byte offsets for
  code are exact. Only fencing/attribute formatting may differ from the
  original source, making within-block mapping approximate by at most a
  few bytes of fence overhead. For engine error reporting (which targets
  code lines, not fence lines), this is negligible.

- [x] Handle blocks with `SourceInfo::default()` (no provenance):
  record a Concat piece with default SourceInfo. `map_offset` through
  default SourceInfo resolves to `FileId(0)` offset 0 — callers should
  treat unexpected locations as "origin unknown."

- [x] The wrapper `serialize_ast_to_qmd` in `engine_execution.rs` calls the
  new API and returns `(String, SourceInfo)`:
  ```rust
  fn serialize_ast_to_qmd(ast: &Pandoc) -> Result<(String, SourceInfo), PipelineError>
  ```

- [x] Tests:
  - Unit test: serialize a simple AST, verify the returned SourceInfo is a
    Concat with pieces covering the **entire** output (frontmatter + blocks)
  - Unit test: given a byte offset in a block's region, `map_offset`
    resolves to the correct original file and position
  - Unit test: given a byte offset in the YAML frontmatter region,
    `map_offset` resolves to the frontmatter's source location (note:
    after metadata merge, `meta.source_info` may be default due to
    `bd-2mxo` — test with a pre-merge AST or a manually constructed
    meta with real source_info)
  - Unit test: AST with blocks from two different files (simulating include
    expansion) → SourceInfo maps to the correct file for each block
  - Unit test: Concat piece lengths sum to total buffer length (no gaps)
  - Unit test: round-trip accuracy — parse a file, serialize, pick a code
    block's offset in serialized text, verify it maps back to approximately
    the right location in the original file

### Phase 0C: SourceInfo in ExecutionContext (commit 3)

Wire the QMD writer's SourceInfo into the engine interface. Include-
dependent integration tests are deferred to Phase 0A's commit.

- [x] Add `source_info` field to `ExecutionContext`:
  ```rust
  pub struct ExecutionContext {
      // ... existing fields ...

      /// Source provenance for the input text.
      ///
      /// Maps byte offsets in the input `&str` back to original source
      /// files (possibly through include expansion boundaries).
      ///
      /// Use `source_info.map_offset(byte_offset, source_context)` to
      /// resolve a position in the engine's input text to the original
      /// file, line, and column.
      ///
      /// Currently not used by any engine for error remapping — see
      /// "Open Questions" in the plan. Available for future use and
      /// for parity with Quarto 1's MappedString.
      pub source_info: SourceInfo,
  }
  ```

- [x] Add `source_context: Arc<SourceContext>` to `ExecutionContext`:
  `map_offset` requires a `&SourceContext` to resolve `FileId`s to paths
  and compute line/column. The engine (or q2's error handling) needs both
  `source_info` and `source_context`.

  **Decision:** Clone into `Arc` at `ExecutionContext` construction.
  `DocumentAst.source_context` remains owned (`SourceContext`, not
  `Arc<SourceContext>`) — the include expansion stage needs to mutate it
  (register included files), and it's simpler to keep it owned during the
  mutable pipeline phases. At `EngineExecutionStage` time, the context is
  finalized (all includes resolved), so we clone into `Arc` once:
  `Arc::new(doc_ast.source_context.clone())`. This is a one-time clone per
  pipeline run, not a hot path.

  No changes to `DocumentAst`'s field types. No migration of downstream
  consumers.

  For TsEngine (subprocess engines), `TsEngine::execute()` extracts the
  serialized source map entries from `source_info` for the protocol —
  the full SourceContext stays Rust-side.

- [x] Update `EngineExecutionStage::run()`:
  - `serialize_ast_to_qmd` now returns `(String, SourceInfo)`
  - Pass the `SourceInfo` into `ExecutionContext` when constructing it
  - Clone `DocumentAst.source_context` into `Arc::new(...)` and pass to
    `ExecutionContext` (one-time clone; context is finalized after include
    expansion)

- [x] Update `ExecutionContext::new()` to accept SourceInfo (with a default
  of `SourceInfo::default()` for backward compatibility in tests)

- [x] **Do NOT change the `ExecutionEngine` trait signature.** SourceInfo is
  in `ExecutionContext`, not a separate parameter. Existing engine
  implementations don't need to change.

- [x] Tests (single-file, no includes — include-dependent tests are in 0A):
  - Unit test: `ExecutionContext` with SourceInfo — construct, verify field
    accessible
  - Unit test: `EngineExecutionStage` populates SourceInfo from QMD writer
  - Integration test: document WITHOUT includes → SourceInfo maps back to
    the original file
  - Integration test: verify `map_offset` works for offsets at:
    - Start of the engine input
    - A code block in the middle
    - End of the engine input
  - Integration test: simulate engine error reporting — given a line number
    in the serialized QMD, convert to byte offset, call `map_offset`, verify
    correct file + line in original source

## Open Question: Error remapping responsibility

When an engine reports an error with a line number, who translates it back
to the original source position?

**Options:**
1. **q2 intercepts engine errors** — The engine returns
   `ExecutionError::ExecutionFailedAtLines` with line numbers in the
   serialized QMD. q2 uses SourceInfo + SourceContext to remap before
   displaying.
2. **Engine does it** — TS engines receive the source map in
   `TsExecuteOptions` (reconstructed as MappedString by the harness).
   Built-in engines have SourceInfo in `ExecutionContext`. Either way
   the engine can remap positions itself before returning errors.
3. **QuartoAPI utility** — For TS engines, the QuartoAPI provides a
   `quarto.sourceMap.resolve(line, col)` method backed by the
   MappedString the harness constructed from the source map.
4. **Nobody does it** — Matching Quarto 1's current (lax) behavior, error
   line numbers are approximate. Engines report positions in the text they
   received; users must mentally map to their source.

Option 1 is the most natural for q2's architecture (q2 holds the SourceInfo,
engines are oblivious). Options 2-3 are needed if the engine wants to
provide real-time error locations during long-running execution (like Julia's
`buildSourceRanges`). These are not mutually exclusive.

**Decision deferred.** Plan 0 ensures the SourceInfo exists and is correct.
Error remapping can be implemented incrementally as engines need it.

## Design Notes

### SourceInfo and MappedString are the same concept

q2's `SourceInfo` and Quarto 1's `MappedString` are different
implementations of the same idea: source provenance tracking. Both answer
"for any byte offset in this derived text, where did it come from in the
original source?"

- **SourceInfo**: a serializable tree (Concat of pieces → Original file
  ranges). Designed for cross-process communication. `map_offset()` traces
  the tree.
- **MappedString**: closures and object references. `.map(index)` returns
  `{ index, originalString }`. Designed for in-process use, never
  serialized.

The protocol naturally uses the SourceInfo representation (byte-range
pieces), and the engine-host harness reconstructs a MappedString from it.
See Plan 1a Phase 1 (`TsSourceMapEntry`) and Phase 5 (MappedString
reconstruction) for the crossing.

### Percent scripts: engine-side, not q2-side

Percent-script conversion (`.jl` files with `# %%` markers → QMD) is
engine-specific: different engines check different file extensions and use
different comment syntaxes. In q2, the engine handles this via `claimsFile`
+ `markdownForFile` in the pre-parse detection flow (see Plan 1c Phase 2).

q2 does not need to know about percent or spin script formats. The engine
converts its file format to QMD, q2 parses the QMD, and the pipeline
proceeds. Source mapping for the conversion step is the engine's
responsibility (Quarto 1 doesn't do it either — percent script conversion
loses provenance, producing an identity-mapped MappedString with no
filename).

Built-in engines do not currently implement `claims_file` or
`markdown_for_file`. Adding percent/spin script support to built-in
engines is documented as future work in Plan 1c.

### Why AST-level, not text-level

Quarto 1 does include expansion at the text level (regex/pattern matching
on raw markdown). q2 does it at the AST level because:
- q2 parses first, then works on the AST — there is no "expanded text"
- The parser already identifies shortcode nodes, so no regex needed
- Source tracking composes naturally: included file → pampa parse with
  FileId → AST nodes with SourceInfo::Original → QMD writer Concat
- Avoids the pitfalls of text-level expansion (matching shortcodes inside
  code blocks, handling nested delimiters, etc.)

### SourceInfo chain

The QMD writer's Concat tiles the **entire** serialized output with no
gaps — frontmatter is tracked via `pandoc.meta.source_info`, each block
is tracked via `block.source_info()`, and inter-block whitespace is
included in each block's measured range.

```
byte offset in serialized QMD (what the engine receives)
  → QMD writer's Concat piece → AST node's source_info
    → Original(FileId for included_file.qmd, byte range)
      → SourceContext.map_offset() → file path, line, column
```

For nodes from the main document, the chain is shorter:
```
byte offset → Concat piece → Original(FileId for main.qmd, byte range)
  → file path, line, column
```

For offsets in YAML frontmatter:
```
byte offset → Concat piece (frontmatter) → meta.source_info
  → Original(FileId for main.qmd, frontmatter byte range)
    → file path, line, column
```
**Note:** Due to `bd-2mxo`, `meta.source_info` is currently
`SourceInfo::default()` after metadata merge, so this chain resolves
to "origin unknown" until that bug is fixed.

### Dual SourceContext in DocumentAst

`DocumentAst` has two separate `SourceContext` fields:

1. **`ast_context.source_context`** — created by pampa's reader. Contains
   `FileInformation` (line break indices) that `map_offset()` needs for
   byte-offset → line/column conversion. This is what AST nodes' `FileId`s
   resolve against.

2. **`source_context` (top-level field)** — created by `ParseDocumentStage`.
   Contains file content strings (via `add_file(path, Some(content))`).
   Used by ariadne for rendering error snippets with source context.

This separation is semi-intentional: different layers own different contexts.
`SourceContext.add_file(path, Some(content))` stores **both** content and
`FileInformation`, so a single entry can serve both purposes — but the two
contexts are separate objects created at different times.

**For include expansion:** each included file must be registered in **both**
contexts with the same `FileId`, so that:
- `map_offset` can resolve AST nodes from included files (needs #1)
- Error messages can show source snippets from included files (needs #2)

Unifying the two `SourceContext`s is desirable long-term but out of scope
for Plan 0.

### Parse-then-remap pattern for multi-file merging

When parsing an included file, `pampa::readers::qmd::read` always creates
a fresh `ASTContext` where the file is `FileId(0)`. To merge into the main
document (which already uses `FileId(0)` for the main file), we use the
**parse-then-remap** pattern established by `EngineExecutionStage`
(engine_execution.rs:267-311):

1. Parse the included file → gets its own `FileId(0)`
2. Register in the main `SourceContext` → gets new `FileId(N)`
3. Call `remap_file_ids` on the parsed AST → `FileId(0)` becomes `FileId(N)`
4. Merge filenames lists
5. Splice remapped blocks into main AST

This pattern requires no changes to pampa's reader API. The reader always
starts fresh; the caller remaps and merges. This is the standard approach
throughout the codebase — `quarto_ast_reconcile::remap_file_ids` provides
the shared utility for walking and remapping.

### No changes to ShortcodeResolveTransform

After include expansion, include shortcode nodes are gone from the AST —
replaced by the included content. `ShortcodeResolveTransform` in stage 6
continues to handle `meta`, Lua shortcodes, and extension shortcodes. It
simply won't encounter include shortcodes. No changes needed.

### No changes to ExecutionEngine trait

SourceInfo goes in `ExecutionContext`, not the trait signature. Built-in
engines (`MarkdownEngine`, `JupyterEngine`, `KnitrEngine`) don't need any
implementation changes. They can optionally use `ctx.source_info` for error
remapping in the future.

## Success Criteria

- [x] Include shortcodes resolved before engine execution
- [x] Recursive includes work; circular includes produce clear error
- [x] Included code cells are visible to the engine (the whole point)
- [x] QMD writer produces SourceInfo mapping serialized text to AST nodes
- [x] SourceInfo in ExecutionContext maps engine input back to original files
- [x] `map_offset` resolves through include boundaries to correct file + line
- [x] All existing tests pass (no regressions)
- [x] Thorough unit tests for SourceInfo chain, even though no engine uses it
- [x] `cargo nextest run --workspace` passes
- [x] Error remapping responsibility documented as open question
