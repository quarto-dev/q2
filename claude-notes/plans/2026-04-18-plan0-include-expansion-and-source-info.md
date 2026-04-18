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
  Parse → MetadataMerge → EngineExec → CompileThemeCss →
    UserFilters(pre) → AstTransforms(ALL shortcodes) → UserFilters(post) →
    RenderHtmlBody → ApplyTemplate

After Plan 0:
  Parse → MetadataMerge → IncludeExpansion(NEW) → EngineExec → CompileThemeCss →
    UserFilters(pre) → AstTransforms(non-include shortcodes) → UserFilters(post) →
    RenderHtmlBody → ApplyTemplate
```

**Note on non-QMD files:** Plan 1b restructures the pipeline entry point so
that `claimsFile` (engine detection Phase 1) runs before `ParseDocument`.
If an engine claims a non-QMD file (e.g., `.jl` percent script), the engine
converts it to QMD text via `markdownForFile`, and that text enters the
pipeline at `ParseDocument`. Plan 0's work (include expansion, QMD writer
SourceInfo) applies equally regardless of whether the input started as QMD
or was converted from another format — by the time include expansion runs,
the AST is the same either way.

## Phase order

Phase 0A → Phase 0B → Phase 0C

Phase 0A (include expansion) and Phase 0B (QMD writer SourceInfo) are
independent in implementation but Phase 0C (wiring + integration tests)
depends on both.

## Work Items

### Phase 0A: Include shortcode expansion stage

New pipeline stage that resolves `include` shortcodes in the AST before
engine execution.

**Note:** q2 does not currently have an `include` shortcode handler. The
existing `ShortcodeResolveTransform` handles `meta` (built-in Rust handler)
and Lua-based shortcodes, but not `include`. This phase implements include
handling from scratch.

- [ ] Create `crates/quarto-core/src/stage/stages/include_expansion.rs`:
  ```rust
  pub struct IncludeExpansionStage;
  ```
  Implements `PipelineStage`. Input/output: `DocumentAst`.

- [ ] Implement AST walking to find include shortcodes:
  - Walk all blocks looking for `Inline::Shortcode` nodes where `name == "include"`
  - Include shortcodes can appear as:
    - Inline: `{{< include file.qmd >}}`
    - Block-level: a `Paragraph` containing only the shortcode (common case)
  - Extract the file path from the shortcode's first positional argument

- [ ] Implement include resolution:
  1. Resolve the included file path relative to the including file's directory
  2. Read the included file
  3. Register it in `SourceContext` (so its `FileId` is available for SourceInfo)
  4. Parse it with pampa (`readers::qmd::read`) with source tracking enabled,
     passing the `FileId` so AST nodes get `SourceInfo::Original` pointing
     to the included file
  5. Replace the shortcode node with the parsed AST nodes:
     - If block-level (paragraph containing only the shortcode): replace the
       paragraph with the included blocks
     - If inline: splice the included content inline (or wrap in a Span)
  6. Merge the included file's `AstContext` into the main document's

- [ ] Handle recursive includes:
  - After splicing, re-walk the newly inserted nodes for more include shortcodes
  - Maintain a set of files currently being included (detect circular includes)
  - Error on circular includes with a clear diagnostic

- [ ] Handle edge cases:
  - Missing included file → diagnostic error (not a panic)
  - Include path outside project directory → warning
  - Include of a file with YAML frontmatter: Quarto 1 strips the frontmatter
    of included files. Match this behavior. (The included file's YAML is
    parsed but discarded; only its body content is spliced.)

- [ ] Wire into pipeline in `pipeline.rs`:
  - Insert `IncludeExpansionStage` between `MetadataMergeStage` and
    `EngineExecutionStage`
  - `ShortcodeResolveTransform` in `AstTransforms` continues to handle
    `meta`, `var`, `env`, Lua shortcodes — it simply won't encounter any
    include shortcodes (they're already resolved)

- [ ] Tests:
  - Unit test: simple include — shortcode node replaced by included content
  - Unit test: recursive include — file A includes file B which includes file C
  - Unit test: circular include — A includes B includes A → error diagnostic
  - Unit test: missing file → error diagnostic, not panic
  - Unit test: included file's AST nodes have SourceInfo pointing to the
    included file (correct FileId, byte offsets)
  - Unit test: include inside a code block is NOT expanded (it's literal text
    in CodeBlock.text, not a Shortcode node)
  - Unit test: block-level include (paragraph with only the shortcode) →
    included blocks replace the paragraph
  - Unit test: included file with YAML frontmatter → frontmatter stripped
  - Integration test: document with `{{< include >}}` containing a code cell
    → after include expansion, the code cell's CodeBlock is present in the AST

### Phase 0B: QMD writer produces SourceInfo

Extend the QMD writer to build a `SourceInfo::Concat` that maps byte ranges
in the serialized output to the `source_info` of the AST nodes that produced
them.

**Quarto 1 reference:** Quarto 1 doesn't need this because it does include
expansion at the text level (producing a MappedString directly). In q2,
include expansion happens at the AST level, and the engine receives
serialized QMD — so the serializer must construct the provenance.

- [ ] Change `serialize_ast_to_qmd` return type:
  ```rust
  fn serialize_ast_to_qmd(ast: &Pandoc) -> Result<(String, SourceInfo), PipelineError>
  ```

- [ ] Modify the QMD writer to track provenance as it writes:
  - Maintain a `Vec<SourcePiece>` accumulator
  - Before writing each AST node, record the current buffer offset
  - After writing, record the end offset and associate with the node's
    `source_info`
  - Build `SourceInfo::Concat { pieces }` from the accumulated pieces

  The granularity should be at the block/inline level — each `CodeBlock`,
  `Paragraph`, `Header`, `Str`, etc. contributes a piece. Whitespace
  between blocks (blank lines, indentation) that doesn't come from a
  specific AST node can use `SourceInfo::default()`.

- [ ] Handle the case where AST nodes have `SourceInfo::default()` (no
  provenance): the corresponding Concat piece just has a default SourceInfo.
  This is fine — it means "this part of the serialized text has no known
  origin." Error mapping through such a piece returns `None`, which is the
  correct answer.

- [ ] Tests:
  - Unit test: serialize a simple AST, verify the returned SourceInfo is a
    Concat with pieces covering the full output length
  - Unit test: given a byte offset in the serialized output, `map_offset`
    resolves to the correct original file and position
  - Unit test: AST with nodes from two different files (simulating include
    expansion) → SourceInfo maps to the correct file for each region
  - Unit test: byte offset in whitespace between blocks → `map_offset`
    returns `None` (no provenance for filler whitespace)
  - Unit test: round-trip accuracy — parse a file, serialize, pick a code
    block's offset in serialized text, verify it maps back to approximately
    the right location in the original file

### Phase 0C: SourceInfo in ExecutionContext + integration

Wire the QMD writer's SourceInfo into the engine interface and write
integration tests for the full chain.

- [ ] Add `source_info` field to `ExecutionContext`:
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

- [ ] Also add `source_context` to `ExecutionContext` (or make it available
  through `StageContext`): `map_offset` requires a `&SourceContext` to
  resolve `FileId`s to paths and compute line/column. The engine (or q2's
  error handling) needs access to both.

  Evaluate whether to:
  - Add `source_context: Arc<SourceContext>` to `ExecutionContext`
  - Pass it separately when needed
  - Keep it on `StageContext` and let the error handling code access it there

- [ ] Update `EngineExecutionStage::run()`:
  - `serialize_ast_to_qmd` now returns `(String, SourceInfo)`
  - Pass the `SourceInfo` into `ExecutionContext` when constructing it
  - Pass the `SourceContext` from `DocumentAst` similarly

- [ ] Update `ExecutionContext::new()` to accept SourceInfo (with a default
  of `SourceInfo::default()` for backward compatibility in tests)

- [ ] **Do NOT change the `ExecutionEngine` trait signature.** SourceInfo is
  in `ExecutionContext`, not a separate parameter. Existing engine
  implementations don't need to change.

- [ ] Tests — thorough coverage of SourceInfo even though no engine uses it:
  - Unit test: `ExecutionContext` with SourceInfo — construct, verify field
    accessible
  - Unit test: `EngineExecutionStage` populates SourceInfo from QMD writer
  - Integration test: full pipeline with include → engine receives text →
    SourceInfo maps byte offset in engine input back to included file
  - Integration test: document WITHOUT includes → SourceInfo maps back to
    the original file
  - Integration test: verify `map_offset` works for offsets at:
    - Start of the engine input
    - A code block in the middle
    - A code block from an included file
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
+ `markdownForFile` in the pre-parse detection flow (see Plan 1b Phase 2).

q2 does not need to know about percent or spin script formats. The engine
converts its file format to QMD, q2 parses the QMD, and the pipeline
proceeds. Source mapping for the conversion step is the engine's
responsibility (Quarto 1 doesn't do it either — percent script conversion
loses provenance, producing an identity-mapped MappedString with no
filename).

Built-in engines do not currently implement `claims_file` or
`markdown_for_file`. Adding percent/spin script support to built-in
engines is documented as future work in Plan 1b.

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

- [ ] Include shortcodes resolved before engine execution
- [ ] Recursive includes work; circular includes produce clear error
- [ ] Included code cells are visible to the engine (the whole point)
- [ ] QMD writer produces SourceInfo mapping serialized text to AST nodes
- [ ] SourceInfo in ExecutionContext maps engine input back to original files
- [ ] `map_offset` resolves through include boundaries to correct file + line
- [ ] All existing tests pass (no regressions)
- [ ] Thorough unit tests for SourceInfo chain, even though no engine uses it
- [ ] `cargo nextest run --workspace` passes
- [ ] Error remapping responsibility documented as open question
