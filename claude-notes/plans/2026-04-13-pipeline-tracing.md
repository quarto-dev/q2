---
title: 'Pipeline Execution Tracing'
date: 2026-04-13
---

## Overview

Add the ability to trace and inspect the state of the rendering pipeline at each step during Quarto 2 document rendering. Unlike Quarto 1's tracing (which only covers Lua filter stages and writes a monolithic JSON blob to disk), Quarto 2's tracing should leverage the explicit pipeline stage representation and use a callback-based design that supports multiple use cases without committing to a single output format.

## Background

### Quarto 1 Tracing (for reference)

In Quarto 1, tracing is implemented in `src/resources/filters/ast/runemulation.lua`:

- **Scope**: Only traces the Lua filter chain (not parsing, engine execution, template application, etc.)
- **Mechanism**: After each Lua filter runs, the entire Pandoc AST is serialized to JSON and appended to a trace array
- **Output**: A single `quarto-filter-trace.json` file containing `[{state: "filter-name", doc: <full AST>}, ...]`
- **Activation**: Via `QUARTO_TRACE_FILTERS` env var or `_quarto.trace-filters` YAML metadata
- **Viewer**: A Quarto document (`trace-viewer.qmd`) that embeds the trace data as Base64 for interactive comparison
- **Limitations**:
  - Only sees filter-level AST transforms (no visibility into parsing, engine, template, CSS, etc.)
  - Full AST serialized per step -- O(n \* AST_size) disk/memory cost
  - Lua userdata (Pandoc objects) are silently skipped during serialization
  - No timing information
  - No way to get intermediate formats (e.g., rendered HTML body before template application)

### Quarto 2 Pipeline Architecture

The rendering pipeline is explicitly modeled with typed stages:

```
LoadedSource -> ParseDocumentStage -> DocumentAst -> MetadataMergeStage -> DocumentAst
  -> EngineExecutionStage -> DocumentAst -> CompileThemeCssStage -> DocumentAst
  -> UserFiltersStage::pre() -> DocumentAst -> AstTransformsStage -> DocumentAst
  -> UserFiltersStage::post() -> DocumentAst -> RenderHtmlBodyStage -> RenderedOutput
  -> ApplyTemplateStage -> RenderedOutput
```

Key types:
- **`PipelineStage` trait** (`stage/traits.rs`): `name()`, `input_kind()`, `output_kind()`, `run(PipelineData, &mut StageContext)`
- **`PipelineData` enum** (`stage/data.rs`): `LoadedSource | DocumentSource | DocumentAst | ExecutedDocument | RenderedOutput | FinalOutput`
- **`PipelineObserver` trait** (`stage/observer.rs`): Existing callback interface for stage lifecycle events (start/complete/error), but **does not receive the data** flowing through the pipeline
- **`TransformPipeline`** (`transform.rs`): Inner pipeline of `AstTransform` steps within `AstTransformsStage` (callouts, TOC, sectionize, etc.)

The existing `PipelineObserver` is close to what we need but is missing the critical piece: **access to the `PipelineData` at each stage boundary**.

## Design

### Core Idea: Extend `PipelineObserver` with Data Access

Rather than creating an entirely new trait, extend the existing `PipelineObserver` to optionally receive references to the pipeline data at stage boundaries. This preserves backwards compatibility (existing observers don't need to change) while enabling rich tracing.

### Approach A: Add data-bearing methods to `PipelineObserver` (Recommended)

Add new default methods that receive `&PipelineData`:

```rust
pub trait PipelineObserver: Send + Sync {
    // Existing methods (unchanged)
    fn on_stage_start(&self, name: &str, index: usize, total: usize) {}
    fn on_stage_complete(&self, name: &str, index: usize, total: usize) {}
    fn on_stage_error(&self, name: &str, index: usize, error: &PipelineError) {}
    fn on_event(&self, message: &str, level: EventLevel) {}
    fn on_pipeline_start(&self, total_stages: usize) {}
    fn on_pipeline_complete(&self) {}
    fn on_pipeline_error(&self, error: &PipelineError) {}

    // NEW: Called before first stage with the pipeline input
    fn on_pipeline_input(&self, _data: &PipelineData) {}

    // NEW: Called after stage completes with reference to output data
    fn on_stage_data(&self, _name: &str, _index: usize, _data: &PipelineData) {}
}
```

In `Pipeline::run()`, the change is minimal:

```rust
// Before the stage loop:
ctx.observer.on_pipeline_input(&data);

// After successful stage execution:
Ok(output) => {
    ctx.observer.on_stage_complete(stage.name(), idx, total);
    ctx.observer.on_stage_data(stage.name(), idx, &output);
    data = output;
}
```

Called unconditionally -- the default empty method body makes this a no-op vtable dispatch for observers that don't override it.

**Pros:**
- Minimal API surface change
- Backwards compatible (all new methods have defaults)
- Single trait to implement for all observability needs

**Cons:**
- `PipelineObserver` grows in scope (observability + data inspection)
- `&PipelineData` may not be sufficient for all use cases (some may want owned data or specific sub-fields)

### Approach B: Separate `PipelineTracer` trait

Create a dedicated trait for data inspection, separate from lifecycle observation:

```rust
pub trait PipelineTracer: Send + Sync {
    fn trace_input(&self, data: &PipelineData) {}
    fn trace_stage_output(&self, stage_name: &str, index: usize, data: &PipelineData) {}
    fn trace_error(&self, stage_name: &str, index: usize, data: &PipelineData, error: &PipelineError) {}
}
```

And add `Option<Arc<dyn PipelineTracer>>` to `StageContext`.

**Pros:**
- Clean separation of concerns
- Can evolve independently from observer

**Cons:**
- Two trait objects to manage
- More fields in StageContext
- The concepts overlap (both want to know when stages start/complete)

### Approach C: Middleware / Wrapper Stage Pattern

Instead of callbacks, implement tracing as a wrapper `PipelineStage` that intercepts data:

```rust
pub struct TracingStage<S: PipelineStage> {
    inner: S,
    tracer: Arc<dyn Fn(&str, &PipelineData)>,
}
```

**Pros:**
- Composes naturally with the existing pipeline model
- No changes to Pipeline::run()

**Cons:**
- Cannot inspect the initial input (before first stage)
- Adds complexity to pipeline construction
- Each stage needs wrapping -- verbose

### Recommendation

**Approach A** is the most pragmatic. The `PipelineObserver` already exists, is wired through the entire system, and the extension is minimal. The `wants_data()` gate ensures zero overhead when tracing is disabled.

For the inner `TransformPipeline`, a similar pattern applies: extend the tracing so that individual AST transforms can be traced. This could be done by passing the observer through `RenderContext` (which `TransformPipeline::execute` already receives) and calling `on_event` or a new method with the AST state.

### Concrete Observer Implementations

Once the trait is extended, we can implement specific tracers:

1. **`JsonTraceObserver`**: Writes a Quarto 1-compatible trace file (for the trace viewer). Serializes `PipelineData` to JSON at each stage. This is the "write everything to disk" approach, useful for debugging.

2. **`SummaryTraceObserver`**: Prints a human-readable summary to stderr (stage name, data kind, timing, AST block count, etc.). Useful for `--trace` CLI flag.

3. **`FilteredTraceObserver`**: Only captures specific stages or data kinds. Useful for targeted debugging.

4. **`TimingObserver`**: Extends `TracingObserver` with wall-clock timing per stage. Useful for performance profiling.

### Activation

Tracing is controlled primarily via **document metadata**, following the standard metadata merge hierarchy:

- **Project-level** (`_quarto.yml`): `trace: true` -- traces all documents in the project
- **Directory-level** (`_metadata.yml`): `trace: true` -- traces documents in that directory
- **Document-level** (YAML front matter): `trace: true` -- traces a single document
- **CLI** (future): `--metadata trace:true` -- once per-render metadata injection is implemented
- **Programmatic**: Pass a custom observer when constructing `StageContext`

Because `MetadataMergeStage` now runs at index 1 (after `ParseDocumentStage`), only the parse stage itself is untraced. The observer starts with a no-op, and after `MetadataMergeStage` completes, the pipeline checks the merged metadata for `trace: true` and activates the real observer for all subsequent stages.

Simple form:
```yaml
trace: true
```

Structured form (future):
```yaml
trace:
  stages: [ast-transforms, render-html-body]
  transforms: true
  format: json
```

### Trace Output Location

Trace data is written to `.quarto/trace/`, which is already gitignored. Directory structure:

```
.quarto/trace/<input-filename>/latest.json
.quarto/trace/<input-filename>/<timestamp>.json  (future: history)
```

For multi-document project renders, each document gets its own subdirectory. Trace viewers inspect `.quarto/trace/` for available traces.

### Inner Transform Tracing

The `TransformPipeline` (callouts, TOC, etc.) runs inside `AstTransformsStage`. To trace individual transforms:

Option 1: Thread the observer through `RenderContext` and call `on_stage_data` with a synthetic stage name like `"transform:callout"`.

Option 2: Give `TransformPipeline` its own tracing mechanism that collects snapshots, then `AstTransformsStage` reports them via the outer observer.

Option 1 is simpler and keeps everything unified.

## Decisions (resolved)

1. **Data serialization format**: Full dump for `JsonTraceObserver`. This won't be practical for very large documents, but guarantees completeness. We can add summary/filtered modes later.

2. **Transform-level granularity**: Trace all inner transforms by default. Configurability for filtering will be added later.

3. **Data access in `Pipeline::run()`**: Not an issue. We borrow `&input` before the loop starts (for `on_pipeline_input`) and borrow `&output` in the `Ok(output)` arm before reassigning `data = output` (for `on_stage_data`). Both work naturally with the current code structure.

4. **WASM compatibility**: The observer trait is `Send + Sync`. For WASM builds (hub-client), tracing might want to emit to JavaScript callbacks. The existing `NoopObserver`/`TracingObserver` pattern should work, but we need to verify.

5. **Trace viewer**: Deferred to Phase 4. The Q1 viewer was never quite good; we'll design a better one for Q2's richer pipeline model.

6. **Trait design**: Extend `PipelineObserver` (Approach A) rather than creating a separate trait. It's already wired through the system.

## Prerequisites

- [x] `bd-ft03`: Reorder MetadataMergeStage before EngineExecutionStage (commit `58f69dca`)

## Work Items

### Phase 1: Core Infrastructure
- [x] Extend `PipelineObserver` trait with `on_stage_data`, `on_pipeline_input`, and `on_transform_data`
- [x] Update `Pipeline::run()` to call data-bearing methods
- [x] `NoopObserver` and `TracingObserver` use default empty implementations (no changes needed)
- [x] Add tests for the new observer methods (CountingObserver extended with data tracking)
- [x] Thread observer through `TransformPipeline::execute` for inner-transform tracing (via `RenderContext.observer`)

### Phase 2: Concrete Implementations
- [x] Implement `SummaryTraceObserver` (human-readable stderr output)
- [x] Implement `JsonTraceObserver` (full trace to JSON file)
- [x] Add `PipelineData` serialization support (full JSON via pampa + summary strings)
- [x] Add timing information to trace output (per-stage and total pipeline duration)

### Phase 3: Metadata-Driven Activation
- [x] Read `trace: true` from merged metadata after `MetadataMergeStage`
- [x] Swap observer from no-op to active tracer when trace metadata is detected
- [x] Create `.quarto/trace/<filename>/` output directory (handled by `JsonTraceObserver::write_trace`)
- [x] Write trace JSON to `.quarto/trace/<filename>/latest.json`
- [x] Support `trace: "summary"` for stderr output via `SummaryTraceObserver`
- [x] Fix: Handle PandocInlines metadata values (YAML strings parsed as Pandoc inlines, not raw scalars)
- [x] Fix: Stage ordering in `render_qmd_to_html` (had EngineExecution before MetadataMerge, same bug as bd-ft03)

### Phase 4: Trace Viewer (future)
- [ ] Design trace viewer for Q2's richer pipeline model
- [ ] Support diff between trace snapshots
- [ ] Support viewing different data kinds (not just AST)

## References

- Quarto 1 tracing: `external-sources/quarto-cli/src/resources/filters/ast/runemulation.lua` (run_emulated_filter_chain)
- Quarto 1 trace data: `external-sources/quarto-cli/src/resources/filters/ast/traceexecution.lua` (add_trace, init_trace, end_trace)
- Quarto 1 trace viewer: `external-sources/quarto-cli/src/resources/tools/ast-tracing/trace-viewer.qmd`
- Pipeline stages: `crates/quarto-core/src/stage/`
- Observer trait: `crates/quarto-core/src/stage/observer.rs`
- Transform pipeline: `crates/quarto-core/src/transform.rs`
