# QMD-Syntax-Helper: Multi-threaded Architecture with Task Reuse

**Date:** 2025-10-22
**Status:** Design Proposal
**Goal:** Restructure qmd-syntax-helper for parallel execution with shared task memoization

---

## Problem Analysis

### Current Architecture Issues

1. **Critical Duplication - Parsing**
   - `ParseChecker` parses file in `check()` → discards AST
   - `DivWhitespaceConverter` parses same file in `check()` → extracts diagnostics
   - `DivWhitespaceConverter` parses AGAIN in `convert()` → extracts diagnostics
   - **Result:** Up to 3 independent parses per file

2. **File I/O Duplication**
   - Each rule independently calls `fs::read_to_string()`
   - No caching of file contents
   - **Result:** 4+ file reads per file (once per rule)

3. **Sequential Execution**
   - Outer loop: sequential file iteration
   - Inner loop: sequential rule application
   - **Result:** No parallelism despite embarrassingly parallel workload

4. **Conversion Mode Inefficiency**
   - Applies fixes sequentially with reparsing between each rule
   - Comment in code: "Apply fixes sequentially, reparsing between each rule"
   - **Result:** O(n) parses for n rules

### Workload Characteristics

| Operation | Type | Dependencies | Parallelizable? |
|-----------|------|--------------|-----------------|
| File reading | I/O | None | YES (per file) |
| Parsing | CPU | File content | YES (per file) |
| ParseChecker | Validation | Parse result | NO (shares parse) |
| DivWhitespace detection | Analysis | Parse diagnostics | NO (shares parse) |
| GridTable detection | Regex | File content | YES |
| DefinitionList detection | Regex | File content | YES |
| GridTable conversion | Subprocess | Detection result | YES (per table) |
| DefinitionList conversion | Subprocess | Detection result | YES (per list) |
| Fix application | I/O | All conversions | NO (sequential writes) |

**Key Insight:** There are two levels of parallelism:
1. **File-level:** Process multiple files concurrently
2. **Task-level:** Within a file, execute independent tasks concurrently + share expensive tasks

---

## Design Goals

1. **Task Sharing:** Parse each file at most once, share result across rules
2. **Parallelism:** Exploit file-level and task-level parallelism
3. **Type Safety:** Leverage Rust's type system for dependency tracking
4. **Simplicity:** Don't over-engineer - avoid complex frameworks
5. **Incremental:** Keep existing rule interface stable during migration
6. **Debuggability:** Maintain verbose mode and error reporting

---

## Design Options Evaluated

### Option 1: Rayon Only (Simple Data Parallelism)

**Approach:**
```rust
file_paths.par_iter().for_each(|path| {
    let content = fs::read_to_string(path)?;
    let parse_result = parse_once(content);

    rules.iter().for_each(|rule| {
        rule.check(path, &parse_result); // Modified signature
    });
});
```

**Pros:**
- Simple to implement
- Well-tested library
- Automatic work stealing

**Cons:**
- Limited task-level granularity
- No automatic memoization
- Hard to express complex dependencies
- Requires changing Rule trait (breaks existing interface)

**Verdict:** Good for file-level parallelism, insufficient for task reuse

---

### Option 2: Async/Tokio (Task-based Concurrency)

**Approach:**
```rust
let tasks: Vec<_> = file_paths.iter().map(|path| {
    tokio::spawn(async move {
        let parse_result = parse_file(path).await;
        let checks = futures::join_all(rules.iter().map(|r|
            r.check_async(path, &parse_result)
        )).await;
    })
}).collect();
```

**Pros:**
- Good for I/O-bound work (file reads, subprocess calls)
- Natural task composition
- Mature ecosystem

**Cons:**
- Parsing is CPU-bound, not I/O-bound
- Async overhead for CPU work
- Infects all code with `async`
- No automatic memoization

**Verdict:** Good for Pandoc subprocess calls, overkill for CPU-bound parsing

---

### Option 3: Custom Task Graph with Memoization

**Approach:**
```rust
struct TaskGraph {
    cache: DashMap<TaskId, Arc<dyn Any + Send + Sync>>,
}

trait Task: Send + Sync {
    type Output: Send + Sync + 'static;
    fn execute(&self, graph: &TaskGraph) -> Self::Output;
    fn dependencies(&self) -> Vec<TaskId>;
}

// Example tasks
struct ParseTask(PathBuf);
struct DivWhitespaceDetectionTask { parse_task: ParseTask }
struct WriteTask { conversion_tasks: Vec<ConversionTask> }
```

**Pros:**
- Explicit dependency tracking
- Automatic memoization via cache
- Type-safe task composition
- Fine-grained parallelism control

**Cons:**
- Custom code to maintain
- More complex than rayon
- Need to handle task scheduling
- Dynamic dispatch overhead

**Verdict:** Most flexible, but significant implementation effort

---

### Option 4: Hybrid - Rayon + Shared Context

**Approach:**
```rust
// Per-file context with lazy evaluation
struct FileContext {
    path: PathBuf,
    content: OnceCell<String>,
    parse_result: OnceCell<ParseResult>,
}

impl FileContext {
    fn parse(&self) -> &ParseResult {
        self.parse_result.get_or_init(|| {
            let content = self.content();
            parse_qmd(content)
        })
    }
}

// Parallel file processing
file_paths.par_iter().map(|path| {
    let ctx = FileContext::new(path);

    // Rules share context automatically
    let results: Vec<_> = rules.par_iter()
        .map(|rule| rule.check_with_context(&ctx))
        .collect();

    results
}).flatten().collect()
```

**Pros:**
- Simple implementation (OnceCell for memoization)
- Rayon for parallelism
- Backward compatible (add context variant of Rule methods)
- Incremental migration path

**Cons:**
- Less flexible than full task graph
- No cross-rule task sharing beyond parse
- File scope only (no finer-grained parallelism)

**Verdict:** Sweet spot - 80% benefit for 20% effort

---

## Recommended Architecture: Hybrid Rayon + Task Context

### High-Level Design

```
┌─────────────────────────────────────────────────────────────┐
│                    Main Orchestrator                        │
│  (Rayon ParallelIterator over files)                       │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │   Per-File Context     │
              │  (Lazy-initialized)    │
              ├────────────────────────┤
              │ - content: OnceCell    │
              │ - parse: OnceCell      │
              │ - diagnostics: OnceCell│
              └────────────────────────┘
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
        ▼                  ▼                  ▼
  ┌──────────┐      ┌──────────┐      ┌──────────┐
  │ ParseChk │      │ DivSpace │      │ GridTbl  │
  │  (uses   │      │  (uses   │      │ (regex)  │
  │  parse)  │      │  parse)  │      │          │
  └──────────┘      └──────────┘      └──────────┘
        │                  │                  │
        └──────────────────┴──────────────────┘
                           │
                           ▼
                    ┌─────────────┐
                    │  Aggregate  │
                    │   Results   │
                    └─────────────┘
```

### Core Components

#### 1. FileTaskContext (New)

```rust
/// Shared context for a single file's processing
/// Provides lazy-initialized, cached access to expensive operations
pub struct FileTaskContext {
    path: PathBuf,
    content: OnceCell<String>,
    parse_result: OnceCell<Arc<ParseResult>>,
    diagnostics: OnceCell<Arc<Vec<DiagnosticMessage>>>,
}

pub struct ParseResult {
    pub ast: Option<PandocAst>, // None if parse failed
    pub success: bool,
}

impl FileTaskContext {
    pub fn new(path: PathBuf) -> Self { ... }

    /// Lazy read file (cached after first call)
    pub fn content(&self) -> Result<&String> {
        self.content.get_or_try_init(|| fs::read_to_string(&self.path))
    }

    /// Lazy parse (cached after first call)
    /// Multiple rules can call this - only parsed once
    pub fn parse(&self) -> Result<&Arc<ParseResult>> {
        self.parse_result.get_or_try_init(|| {
            let content = self.content()?;
            // Call quarto_markdown_pandoc::readers::qmd::read()
            Ok(Arc::new(parse_qmd(content)?))
        })
    }

    /// Lazy diagnostics extraction (cached)
    pub fn diagnostics(&self) -> Result<&Arc<Vec<DiagnosticMessage>>> {
        self.diagnostics.get_or_try_init(|| {
            let content = self.content()?;
            // Parse with error capture
            Ok(Arc::new(extract_diagnostics(content)?))
        })
    }
}
```

**Why OnceCell:**
- Thread-safe lazy initialization
- First caller initializes, subsequent callers get cached value
- No explicit locking needed (OnceCell handles it)
- Zero overhead if not used

---

#### 2. Extended Rule Trait (Backward Compatible)

```rust
/// Original trait (unchanged)
pub trait Rule {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn check(&self, file_path: &Path, verbose: bool) -> Result<Vec<CheckResult>>;
    fn convert(&self, file_path: &Path, ...) -> Result<ConvertResult>;
}

/// New trait for context-aware rules
pub trait ContextAwareRule: Rule {
    /// Check with shared context (can reuse parse results)
    fn check_with_context(
        &self,
        ctx: &FileTaskContext,
        verbose: bool
    ) -> Result<Vec<CheckResult>>;

    /// Convert with shared context
    fn convert_with_context(
        &self,
        ctx: &FileTaskContext,
        in_place: bool,
        check_mode: bool,
        verbose: bool,
    ) -> Result<ConvertResult>;
}

/// Automatic impl: context-aware rules can fall back to old interface
impl<T: ContextAwareRule> Rule for T {
    fn check(&self, file_path: &Path, verbose: bool) -> Result<Vec<CheckResult>> {
        let ctx = FileTaskContext::new(file_path.to_path_buf());
        self.check_with_context(&ctx, verbose)
    }

    // ... similar for convert
}
```

**Migration Strategy:**
1. Add `ContextAwareRule` trait
2. Implement for ParseChecker (uses `ctx.parse()`)
3. Implement for DivWhitespaceConverter (uses `ctx.diagnostics()`)
4. GridTable and DefinitionList can use `ctx.content()` instead of own reads
5. Old `Rule` trait continues to work during transition

---

#### 3. Parallel Execution with Rayon

```rust
pub fn process_files_parallel(
    file_paths: Vec<PathBuf>,
    rules: Vec<Arc<dyn ContextAwareRule>>,
    verbose: bool,
) -> Result<Vec<CheckResult>> {

    use rayon::prelude::*;

    // File-level parallelism
    let all_results: Vec<Vec<CheckResult>> = file_paths
        .par_iter()  // Parallel iterator
        .map(|path| {
            let ctx = FileTaskContext::new(path.clone());

            // Rule-level parallelism (within each file)
            let file_results: Vec<CheckResult> = rules
                .par_iter()  // Also parallel!
                .map(|rule| {
                    rule.check_with_context(&ctx, verbose)
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();

            Ok(file_results)
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(all_results.into_iter().flatten().collect())
}
```

**Parallelism Levels:**
1. **Outer par_iter:** Process multiple files concurrently
2. **Inner par_iter:** Run multiple rules concurrently per file
3. **Automatic sharing:** First rule to call `ctx.parse()` parses; others wait and reuse

**Thread Safety:**
- `FileTaskContext` is `Sync` (OnceCell is thread-safe)
- Rules are `Arc<dyn ... + Send + Sync>`
- Rayon handles work stealing and thread pool

---

#### 4. Conversion Mode (Sequential Writes per File)

```rust
pub fn convert_files_parallel(
    file_paths: Vec<PathBuf>,
    rules: Vec<Arc<dyn ContextAwareRule>>,
    in_place: bool,
) -> Result<Vec<ConvertResult>> {

    // Files processed in parallel
    file_paths.par_iter().map(|path| {
        let ctx = FileTaskContext::new(path.clone());

        // Within a file: sequential rule application
        // (Need to reparse between rules if content changes)
        let mut cumulative_result = ConvertResult::default();

        for rule in &rules {
            let result = rule.convert_with_context(
                &ctx,
                false, // Don't write yet
                false,
                verbose
            )?;

            cumulative_result.merge(result);

            // If content changed, invalidate context for next rule
            // (This is a limitation - see "Future Enhancements")
            ctx.invalidate(); // New method needed
        }

        // Final write (one per file)
        if in_place {
            write_result_to_file(&cumulative_result, path)?;
        }

        Ok(cumulative_result)
    }).collect()
}
```

**Note:** Conversion mode is more complex because:
- Rules modify content
- Later rules need updated content
- Must maintain sequential semantics within a file
- But files are still processed in parallel

---

### Performance Model

**Current (Sequential):**
```
Time = N_files × (T_read + T_parse × N_parse_rules + T_regex × N_regex_rules)
```

With 100 files, 2 parse rules, 2 regex rules:
```
Time = 100 × (10ms + 50ms × 2 + 5ms × 2) = 100 × 120ms = 12 seconds
```

**Proposed (Parallel with Sharing):**
```
Time = (N_files / N_threads) × (T_read + T_parse + max(T_rule1, T_rule2, ...))
```

With 100 files, 8 threads, shared parse:
```
Time = (100 / 8) × (10ms + 50ms + max(5ms, 5ms, 5ms, 5ms))
     = 12.5 × 65ms
     = 812ms
```

**Expected Speedup:** ~14.8x (near-linear with threads + parse sharing)

---

## Implementation Strategy

### Phase 1: Add Context Infrastructure (Non-breaking)

**Tasks:**
1. Create `FileTaskContext` struct with OnceCell fields
2. Implement lazy `content()`, `parse()`, `diagnostics()` methods
3. Add unit tests for context caching behavior
4. No changes to existing rules yet

**Validation:** Context works in isolation, caching verified

---

### Phase 2: Migrate ParseChecker and DivWhitespace (High Value)

**Tasks:**
1. Implement `ContextAwareRule` for `ParseChecker`
   - Change `check_with_context()` to call `ctx.parse()`
   - Remove internal `fs::read_to_string()` and parse call
2. Implement `ContextAwareRule` for `DivWhitespaceConverter`
   - Change `check_with_context()` to call `ctx.diagnostics()`
   - Remove internal `get_parse_errors()` calls
3. Update main.rs to use context when available
4. Benchmark: measure parse sharing benefit

**Expected Benefit:** Eliminate 2-3x redundant parsing

---

### Phase 3: Add Parallel Execution

**Tasks:**
1. Add rayon dependency to Cargo.toml
2. Create `process_files_parallel()` function
3. Add CLI flag `--parallel` (default: auto-detect based on file count)
4. Wire up in main.rs
5. Benchmark: measure speedup vs sequential

**Expected Benefit:** 4-8x speedup on multi-core systems

---

### Phase 4: Migrate Remaining Rules (Optional)

**Tasks:**
1. Implement `ContextAwareRule` for GridTableConverter
   - Use `ctx.content()` instead of direct file read
2. Implement for DefinitionListConverter
   - Use `ctx.content()` instead of direct file read
3. Deprecate old `Rule` trait (with warning)

**Expected Benefit:** Eliminate redundant file reads (minor, ~10ms/file)

---

### Phase 5: Advanced Task Parallelism (Future)

**Tasks:**
1. Parallel pandoc conversion calls (per table/list)
2. Parallel regex detection (split file into chunks)
3. Async subprocess execution for pandoc

**Expected Benefit:** 2-4x additional speedup for conversion-heavy workloads

---

## Alternative: Full Task Graph (Future Enhancement)

If we later need finer-grained parallelism (e.g., parallel pandoc calls), we can evolve to:

```rust
pub trait Task: Send + Sync {
    type Output: Clone + Send + Sync + 'static;

    fn execute(&self, executor: &TaskExecutor) -> Self::Output;
    fn dependencies(&self) -> Vec<TaskId>;
    fn cache_key(&self) -> TaskId;
}

pub struct TaskExecutor {
    cache: DashMap<TaskId, Arc<dyn Any + Send + Sync>>,
    thread_pool: rayon::ThreadPool,
}

impl TaskExecutor {
    pub fn run<T: Task>(&self, task: T) -> T::Output {
        let key = task.cache_key();

        if let Some(cached) = self.cache.get(&key) {
            return cached.downcast_ref::<T::Output>().unwrap().clone();
        }

        // Execute dependencies in parallel
        let deps: Vec<_> = task.dependencies()
            .into_par_iter()
            .map(|dep_id| self.get_task(dep_id).execute(self))
            .collect();

        // Execute this task
        let result = task.execute(self);
        self.cache.insert(key, Arc::new(result.clone()));
        result
    }
}
```

**Example Tasks:**

```rust
struct ParseTask(PathBuf);
impl Task for ParseTask {
    type Output = Arc<ParseResult>;
    fn dependencies(&self) -> Vec<TaskId> { vec![] }
    fn execute(&self, _: &TaskExecutor) -> Self::Output {
        Arc::new(parse_qmd(&fs::read_to_string(&self.0).unwrap()))
    }
}

struct DivWhitespaceDetectionTask {
    parse_task: ParseTask,
}
impl Task for DivWhitespaceDetectionTask {
    type Output = Vec<Fix>;
    fn dependencies(&self) -> Vec<TaskId> {
        vec![self.parse_task.cache_key()]
    }
    fn execute(&self, executor: &TaskExecutor) -> Self::Output {
        let parse = executor.run(self.parse_task.clone());
        find_div_whitespace_errors(&parse)
    }
}

struct GridTableConversionTask {
    table: GridTable,
    resource_mgr: Arc<ResourceManager>,
}
impl Task for GridTableConversionTask {
    type Output = ConvertedTable;
    fn dependencies(&self) -> Vec<TaskId> { vec![] }
    fn execute(&self, _: &TaskExecutor) -> Self::Output {
        // Call pandoc subprocess
        convert_table_via_pandoc(&self.table, &self.resource_mgr)
    }
}
```

**Benefits:**
- Explicit dependency graph
- Automatic parallelization
- Fine-grained caching
- Can parallelize pandoc calls

**Trade-offs:**
- More complex
- More code to maintain
- Overkill for current needs

**Decision:** Defer to Phase 5 if benchmarks show need

---

## Trade-offs and Considerations

### Pros of Recommended Approach

1. **Incremental:** Can migrate rules one at a time
2. **Simple:** OnceCell + Rayon, both well-tested libraries
3. **Type-safe:** Rust's type system prevents data races
4. **Performant:** Near-linear speedup with core count
5. **Debuggable:** Can add logging to context methods
6. **Backward compatible:** Old Rule trait still works

### Cons and Mitigations

1. **Conversion mode complexity**
   - **Issue:** Rules modify content, need reparsing
   - **Mitigation:** Keep sequential within file, parallel across files
   - **Future:** Investigate transaction-based approach

2. **Memory usage**
   - **Issue:** Parsing N files in parallel = N parse results in memory
   - **Mitigation:** Process files in batches (chunked parallel iterator)
   - **Future:** Stream-based processing for large files

3. **Error handling**
   - **Issue:** Parallel errors harder to aggregate
   - **Mitigation:** Rayon collects errors into Result<Vec<_>>
   - **Future:** Better error context (which file failed?)

4. **Pandoc resource extraction**
   - **Issue:** ResourceManager creates temp dirs per instance
   - **Mitigation:** Share single ResourceManager across all rules
   - **Future:** Resource pool if needed

### Non-Goals (Out of Scope)

1. **Distributed execution:** Stay single-machine
2. **Persistent cache:** Don't cache between runs
3. **Incremental processing:** Process all files each run
4. **Streaming:** Load full files into memory

---

## Success Metrics

### Performance Targets

| Metric | Current (Sequential) | Target (Parallel) |
|--------|----------------------|-------------------|
| 100 files, check mode | ~12 seconds | <1 second |
| Parse reuse rate | 0% (3 parses/file) | 100% (1 parse/file) |
| CPU utilization | ~12% (1/8 cores) | >80% (7-8/8 cores) |
| Memory usage | ~50MB | <200MB |

### Correctness

1. **Identical results:** Parallel execution produces same CheckResults as sequential
2. **No data races:** All Miri tests pass
3. **No deadlocks:** All tests complete in bounded time

### Code Quality

1. **Test coverage:** >80% for new context code
2. **Documentation:** All public APIs documented
3. **Benchmarks:** Criterion benchmarks for critical paths

---

## Open Questions for Discussion

1. **Thread count:** Auto-detect (num_cpus) or user-configurable?
2. **Batch size:** Process files in batches to limit memory? What size?
3. **Progress reporting:** How to show progress with parallel execution?
4. **Conversion mode:** Keep sequential within file, or attempt parallel?
5. **Resource sharing:** Share ResourceManager, or pool of managers?
6. **Task graph:** Implement now (more complex) or later (simpler first)?

---

## Appendix: Rust Crates for Task Graphs

For reference, existing Rust task/dataflow libraries:

1. **rayon** (recommended for Phase 3)
   - Pros: Mature, simple, great for data parallelism
   - Cons: No automatic memoization, limited dependency expression

2. **tokio**
   - Pros: Great for async I/O, task spawning
   - Cons: Async overhead, not ideal for CPU-bound work

3. **crossbeam**
   - Pros: Low-level primitives, very flexible
   - Cons: More manual work, no high-level abstractions

4. **salsa** (from rust-analyzer)
   - Pros: Incremental computation, automatic caching
   - Cons: Designed for query systems, might be overkill

5. **timely dataflow**
   - Pros: Academic research project, very powerful
   - Cons: Complex, learning curve, overkill for our use case

6. **moka** or **cached**
   - Pros: Simple memoization caches
   - Cons: Need to manually wire up dependencies

**Recommendation:** Start with OnceCell (stdlib) + Rayon. If we need more, evaluate task graph in Phase 5.

---

## References

- **Rayon book:** https://github.com/rayon-rs/rayon
- **OnceCell docs:** https://docs.rs/once_cell/
- **Dask (Python):** https://docs.dask.org/en/latest/
- **Salsa (Rust):** https://github.com/salsa-rs/salsa

---

## Next Steps

1. **Review this design** with stakeholder (Carlos)
2. **Prototype Phase 1** (FileTaskContext) in branch
3. **Benchmark** parse sharing benefit
4. **Decide** on full task graph vs. hybrid approach
5. **Implement** incrementally with tests

**Estimated Effort:**
- Phase 1: 4-6 hours
- Phase 2: 6-8 hours
- Phase 3: 4-6 hours
- Total: ~14-20 hours

**Estimated Benefit:**
- 10-15x faster check mode
- 3x fewer parses (eliminates duplication)
- Better CPU utilization

