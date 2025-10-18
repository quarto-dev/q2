# Making Rendering Dependencies Explicit: Analysis and Recommendations

**Date:** 2025-10-12
**Purpose:** Analyze the benefits and approach for representing rendering dependencies explicitly in Kyoto
**Status:** Complete

## Problem Statement

The current Quarto-CLI rendering system has **implicit data dependencies** that manifest through:

1. **File-based communication**: Steps write to disk, subsequent steps read
2. **Global state**: Shared mutable state (navigation, search index)
3. **Ordering assumptions**: Steps must run in specific order, but this isn't enforced
4. **Hidden dependencies**: No explicit representation of what depends on what

### Examples of Implicit Dependencies

#### Single Document Rendering

```typescript
// Implicit: engine result written to disk
const executeResult = await engine.execute(options);
const mdFile = `/tmp/quarto-${uuid}.md`;
Deno.writeTextFileSync(mdFile, executeResult.markdown);

// Implicit: pandoc reads from disk
await runPandoc({
  input: mdFile,  // Must exist before pandoc runs
  // ...
});
```

**Problem**: Nothing enforces that `engine.execute()` completes before `runPandoc()` starts except sequential execution.

#### Website Project Rendering

```typescript
// Pre-render: Build navigation state (global singleton)
export const navigation: NavigationState = {
  navbar: undefined,
  sidebars: [],
};

await initWebsiteNavigation(project);
// Populates global `navigation` object

// Per-file rendering: Read navigation state
function websiteNavigationExtras(...) {
  const sidebar = sidebarForHref(href, navigation.sidebars);
  // Reads from global state
}
```

**Problem**: Each file render has an implicit dependency on `initWebsiteNavigation()` completing first, but this is enforced only by execution order.

#### Post-Render Dependencies

```typescript
// All files must be rendered before post-render hooks
await renderFiles(files, ...);  // Produces HTML files on disk

// Post-render reads HTML from disk
await updateSearchIndex(project, outputFiles, incremental);
// Reads HTML files written by renderFiles
```

**Problem**: Post-render steps depend on all HTML files existing, but this is implicit.

## Consequences of Implicit Dependencies

### 1. No Safe Parallelization

**Current**: Files must render sequentially
```typescript
for (const file of files) {
  await renderFileInternal(file, ...);
}
```

**Why**: Unclear if files are independent. What if File B references File A? What if they both modify shared state?

### 2. Fixed Pipeline Order

**Current**: Pipeline order is hardcoded
```typescript
await validateYAML(context);
await executeEngine(context);
await handleLanguageCells(context);
await runPandoc(context);
```

**Problem**: Users cannot reorder (e.g., run filters before engine execution)

### 3. Difficult to Debug

**Current**: Error messages lack context
```
Error: Pandoc failed with exit code 1
```

**Wish**: Show dependency chain
```
Error: Pandoc failed
  Dependency chain:
    ParseYAML -> ValidateYAML -> ExecuteEngine -> HandleCells -> [Pandoc]
  Input markdown: /tmp/step-5.md (2847 lines)
  Pandoc command: pandoc --from markdown --to html ...
```

### 4. No Caching/Memoization

**Current**: Freeze system is manual and limited
```yaml
freeze: auto  # User must opt-in
```

**Problem**: Can't automatically cache any step, only engine execution

### 5. Fragile to Changes

**Current**: Adding a new step requires:
1. Finding correct insertion point in code
2. Ensuring all data is available
3. Testing entire pipeline
4. Hoping nothing breaks

**Problem**: No way to validate that new step's dependencies are satisfied

## Proposed Solution: Explicit Workflow Representation

Represent rendering as a **directed acyclic graph (DAG)** where:
- **Nodes** = processing steps
- **Edges** = data dependencies
- **Artifacts** = data flowing between steps

See [`explicit-workflow-design.md`](explicit-workflow-design.md) for full design.

### Key Benefits

#### 1. Safe Parallelization

**With explicit dependencies**, executor can identify independent steps:

```
BuildNavigation
      │
      ├──> [File1 ──> HTML1] ──┐
      │                        │
      ├──> [File2 ──> HTML2] ──┼──> GenerateSitemap
      │                        │
      └──> [File3 ──> HTML3] ──┘
```

Files 1, 2, 3 can render **concurrently** because they don't depend on each other.

**Performance impact**: 100-file website
- Sequential: 100 files × 10s/file = 1000s (16.7 minutes)
- Parallel (16 cores): 100 files / 16 × 10s/file = 62.5s (1 minute)
- **16× speedup**

#### 2. User-Configurable Pipelines

**With explicit steps**, users can specify custom order:

```yaml
# _quarto.yml
pipeline:
  order:
    - parse-yaml
    - extract-markdown
    - run-filters:        # NEW: Filters BEFORE engine
        filters:
          - inject-cells.lua
    - execute-engine
    - run-filters:        # Filters AFTER engine
        filters:
          - process-outputs.lua
    - run-pandoc
```

**Use cases**:
- Inject code cells dynamically before execution
- Pre-process markdown before engine sees it
- Post-process engine outputs before Pandoc
- Insert custom transformation stages

#### 3. Automatic Caching

**With explicit artifacts and cache keys**, any step can be cached:

```rust
// Automatic caching based on input hash
let cache_key = hash_inputs(&[
    markdown_artifact,
    metadata_artifact,
    format_config,
]);

if let Some(cached) = cache.get(&cache_key) {
    return cached;  // Skip execution
}
```

**Performance impact**: Incremental render
- No cache: Re-execute all steps (10s)
- With cache: Skip unchanged steps (0.5s)
- **20× speedup for incremental renders**

#### 4. Better Error Messages

**With execution traces**, show full context:

```
Error: Engine execution failed
  Step: ExecuteEngine { engine: "jupyter" }
  Input: chapter-3.qmd (547 lines)

  Dependency chain:
    ParseYAML (completed in 12ms)
      ↓
    ValidateYAML (completed in 45ms)
      ↓
    ExtractMarkdown (completed in 8ms)
      ↓
    [ExecuteEngine] ← FAILED

  Error: Kernel died
  Last output: TypeError: unsupported operand type(s)

  Debug:
    - Intermediate markdown: /tmp/workflow-abc/step-3.md
    - Kernel log: /tmp/jupyter-kernel-123.log
```

#### 5. Easier Testing

**With isolated steps**, test each independently:

```rust
#[tokio::test]
async fn test_execute_engine_step() {
    let executor = JupyterEngineExecutor::new();

    let inputs = vec![
        Artifact::Markdown(test_markdown()),
        Artifact::Metadata(test_metadata()),
    ];

    let outputs = executor.execute(inputs, &test_context()).await?;

    assert_eq!(outputs.len(), 1);
    match &outputs[0] {
        Artifact::ExecuteResult(result) => {
            assert!(result.markdown.contains("<!-- output -->"));
        }
        _ => panic!("Expected ExecuteResult"),
    }
}
```

## Implementation Recommendations

### Phase 1: Foundation (Weeks 1-4)

**Goals**: Core workflow infrastructure

**Tasks**:
1. Define core types (`Step`, `Artifact`, `Workflow`, `StepExecutor`)
2. Implement `WorkflowBuilder` with cycle detection
3. Implement sequential `WorkflowExecutor`
4. Add basic error handling and tracing

**Deliverable**: Can construct and execute simple workflows

**Test**: Single document rendering pipeline as workflow

### Phase 2: Single Document Rendering (Weeks 5-8)

**Goals**: Port single document pipeline to workflow system

**Tasks**:
1. Implement step executors:
   - `ParseYAMLExecutor`
   - `ValidateYAMLExecutor`
   - `ExtractMarkdownExecutor`
   - `ExecuteEngineExecutor` (adapts existing engines)
   - `HandleLanguageCellsExecutor`
   - `PandocExecutor`
   - `PostprocessHTMLExecutor`
2. Build single document workflow generator
3. Test against current quarto-cli output (bit-for-bit identical)
4. Add compatibility layer for legacy API

**Deliverable**: Can render single documents using workflows

**Test**: `quarto render doc.qmd` produces identical output to current CLI

### Phase 3: Parallelization (Weeks 9-12)

**Goals**: Enable concurrent execution

**Tasks**:
1. Implement parallel execution in `WorkflowExecutor`:
   - Topological sort
   - Group independent steps
   - Spawn concurrent tasks
   - Collect results
2. Add thread safety to artifact storage
3. Test with website projects
4. Benchmark performance improvements
5. Add parallelism configuration

**Deliverable**: Website projects render files concurrently

**Test**: 100-file website renders in ~1/16th time (with 16 cores)

### Phase 4: Caching (Weeks 13-16)

**Goals**: Transparent caching of step outputs

**Tasks**:
1. Design cache key generation:
   - Hash input artifacts
   - Include step configuration
   - Handle non-deterministic steps
2. Implement `Cache` trait:
   - File-based implementation
   - Serialization with `bincode` or similar
   - TTL and eviction policies
3. Implement `CachingWorkflowExecutor`
4. Integrate with existing freeze system
5. Test incremental renders

**Deliverable**: Automatic caching of expensive steps

**Test**: Incremental render skips unchanged steps

### Phase 5: Reconfiguration (Weeks 17-20)

**Goals**: User-specified pipeline order

**Tasks**:
1. Design configuration format (YAML):
   ```yaml
   pipeline:
     order: [...]
     custom-steps: {...}
   ```
2. Implement custom workflow builder from config
3. Add validation:
   - All required steps present
   - Dependencies satisfied
   - No cycles
4. Document advanced use cases
5. Create example configurations

**Deliverable**: Users can customize pipeline order

**Test**: Run filters before engine execution

### Phase 6: Extensions (Weeks 21-24)

**Goals**: Third-party workflow extensions

**Tasks**:
1. Design extension API:
   ```rust
   trait WorkflowExtension {
       fn extend_workflow(&self, builder: &mut WorkflowBuilder) -> Result<()>;
   }
   ```
2. Implement extension loading
3. Add extension points:
   - Before/after existing steps
   - Replace steps
   - Add custom steps
4. Create example extensions:
   - Custom diagram renderer
   - Additional format postprocessor
   - Custom validation step
5. Document extension development

**Deliverable**: Third parties can extend rendering pipeline

**Test**: Example extension successfully inserts custom step

### Phase 7: Production Hardening (Weeks 25-28)

**Goals**: Production-ready system

**Tasks**:
1. Error recovery:
   - Retry policies
   - Partial failure handling
   - Rollback mechanisms
2. Performance optimization:
   - Cache warming
   - Lazy artifact loading
   - Memory efficiency
3. Monitoring:
   - Metrics collection
   - Performance tracing
   - Cache hit rates
4. Documentation:
   - Architecture guide
   - Developer documentation
   - User guide for customization

**Deliverable**: Production-ready workflow system

**Test**: Extended integration tests, stress tests, performance benchmarks

## Comparison: Current vs Explicit Workflows

| Aspect | Current Quarto-CLI | With Explicit Workflows |
|--------|-------------------|------------------------|
| **Parallelization** | Sequential only | Automatic where safe |
| **Reconfiguration** | Hardcoded order | User-configurable |
| **Caching** | Manual (freeze) | Automatic |
| **Error messages** | Basic | Detailed with context |
| **Testing** | Integration only | Unit + integration |
| **Extensions** | Limited hooks | Full workflow extensibility |
| **Debugging** | Print statements | Execution tracing |
| **Performance** | Good (single file) | Excellent (multi-file) |

## Design Trade-offs

### Pros

✅ **Performance**: Parallel execution of independent steps
✅ **Flexibility**: User-configurable pipelines
✅ **Reliability**: Explicit dependencies prevent errors
✅ **Debugging**: Clear execution traces
✅ **Testing**: Isolated unit tests
✅ **Caching**: Transparent memoization
✅ **Extensibility**: Third-party extensions

### Cons

❌ **Complexity**: More abstract than sequential code
❌ **Overhead**: DAG construction and execution management
❌ **Learning curve**: Developers must understand workflow system
❌ **Migration**: Porting existing code to workflows

### Mitigations

**For complexity**: Provide high-level builders and examples
**For overhead**: Optimize hot paths, lazy evaluation
**For learning curve**: Comprehensive documentation and examples
**For migration**: Compatibility layer for gradual transition

## Alternatives Considered

### Alternative 1: Implicit Parallelization (Rayon)

**Approach**: Use Rayon to parallelize file rendering automatically

```rust
files.par_iter().for_each(|file| {
    render_file(file);
});
```

**Pros**: Simple, no explicit dependencies
**Cons**: Unsafe if files have dependencies, no reconfiguration

**Verdict**: ❌ Too risky without dependency analysis

### Alternative 2: Build System Integration (Bazel)

**Approach**: Use existing build system (Bazel, Buck) for rendering

**Pros**: Mature caching, parallelization, incremental builds
**Cons**: External dependency, steep learning curve, less control

**Verdict**: ❌ Too heavyweight for end users

### Alternative 3: Dataflow Framework (Dask)

**Approach**: Use existing dataflow framework for rendering

**Pros**: Proven parallelization, good caching
**Cons**: Python dependency (for Dask), runtime overhead

**Verdict**: ❌ Adds external dependencies

### Alternative 4: Manual Dependency Tracking

**Approach**: Manually track dependencies with simple annotations

```rust
#[depends_on(ParseYAML, ExtractMarkdown)]
async fn execute_engine(...) { ... }
```

**Pros**: Simple, explicit in code
**Cons**: No dynamic workflows, harder to reconfigure

**Verdict**: 🤔 Possible simpler alternative

### Recommendation

**Explicit workflows** (proposed design) provides best balance of:
- Performance (parallelization)
- Flexibility (reconfiguration)
- Control (no external dependencies)
- Future-proofing (extensibility)

Start with simpler manual dependency tracking if needed, but architect for full workflow system.

## Questions to Resolve

### 1. Artifact Granularity

**Question**: How fine-grained should artifacts be?

**Options**:
- A: Very fine (every intermediate value)
- B: Coarse (only major outputs: markdown, HTML, etc.)
- C: Hybrid (fine for hot paths, coarse elsewhere)

**Recommendation**: Start with **Option B** (coarse), refine hot paths later

### 2. Dynamic vs Static Workflows

**Question**: Should workflow structure be mutable during execution?

**Example**: Based on metadata, skip certain steps

**Options**:
- A: Static (DAG fixed before execution)
- B: Dynamic (can modify during execution)

**Recommendation**: Start with **Option A** (static), simpler to implement and reason about

### 3. Cache Invalidation

**Question**: How to determine when cache is stale?

**Options**:
- A: Content hashing (Bazel-style)
- B: Modification timestamps
- C: User-specified TTL
- D: Explicit invalidation commands

**Recommendation**: **Option A** (content hashing) for correctness, **Option C** (TTL) for performance

### 4. Error Recovery

**Question**: How to handle step failures?

**Options**:
- A: Fail fast (stop entire workflow)
- B: Continue on error (mark failed, continue others)
- C: Retry policies (configurable retries)

**Recommendation**: **Option A** (fail fast) by default, **Option C** (retry) for network operations

### 5. Distributed Execution

**Question**: Should workflows support distributed execution?

**Use case**: Render 1000-file website across multiple machines

**Options**:
- A: Local only
- B: Support remote execution (gRPC, message queue)

**Recommendation**: Start with **Option A** (local), design for **Option B** (remote) as future enhancement

## Success Metrics

### Performance

- [ ] **Parallelization**: 10× speedup for 100-file website (on 16-core machine)
- [ ] **Caching**: 20× speedup for incremental renders
- [ ] **Overhead**: <5% overhead vs sequential execution (single file)

### Flexibility

- [ ] **Reconfiguration**: Users can run filters before/after engine
- [ ] **Extensions**: Third-party extensions can insert custom steps
- [ ] **Custom pipelines**: At least 3 documented advanced use cases

### Reliability

- [ ] **Error messages**: 100% of errors include dependency chain
- [ ] **Testing**: 100% of step executors have unit tests
- [ ] **Validation**: All workflows validated before execution (no runtime cycles)

### Adoption

- [ ] **Migration**: 100% of current CLI features ported to workflows
- [ ] **Compatibility**: Existing documents render identically
- [ ] **Documentation**: Complete architecture guide and examples

## Related Documents

- [`single-document-render-pipeline.md`](single-document-render-pipeline.md): Current pipeline analysis
- [`website-project-rendering.md`](website-project-rendering.md): Website project rendering
- [`book-project-rendering.md`](book-project-rendering.md): Book project rendering
- [`explicit-workflow-design.md`](explicit-workflow-design.md): Detailed workflow design

## Conclusion

Making rendering dependencies explicit through workflow DAGs provides:

1. **Performance**: Safe parallelization and automatic caching
2. **Flexibility**: User-configurable pipelines and third-party extensions
3. **Reliability**: Better error messages and testing
4. **Future-proofing**: Foundation for advanced features

The proposed design is achievable in 6-7 months with phased implementation, starting with core infrastructure and gradually adding parallelization, caching, and reconfiguration.

**Recommendation**: Proceed with explicit workflow design for Kyoto, starting with Phase 1 (foundation) and Phase 2 (single document rendering).
