# Rust Libraries for Task DAG / Incremental Computation

**Date:** 2025-10-22
**Purpose:** Survey of Rust libraries implementing "plan → optimize → execute" pattern for task graphs

---

## Executive Summary

There are several mature Rust libraries for DAG-based task execution and incremental computation, ranging from simple task schedulers to sophisticated query systems. They fall into three categories:

1. **Task DAG Executors** - Execute explicit task graphs with dependencies
   - `async_dag`, `dagrs`, `RustDagcuter`, `futures-dagtask`

2. **Incremental Computation Frameworks** - Automatic memoization with dependency tracking
   - `salsa`, `comemo`, `adapton`, `granularity`

3. **Dataflow Systems** - Stream processing and distributed computation
   - `timely-dataflow`, `differential-dataflow`

**For qmd-syntax-helper**, I recommend exploring:
- **Simple approach:** `async_dag` (minimal, focused)
- **Sophisticated approach:** `comemo` (automatic memoization, from Typst compiler)
- **Full query system:** `salsa` (like rust-analyzer, more complex)

---

## Category 1: Task DAG Executors

These libraries let you explicitly build a DAG of tasks and execute them with automatic parallelization.

### 1.1 async_dag ⭐ (Simplest, Recommended for Learning)

**Repository:** https://docs.rs/async_dag/
**Maturity:** Stable
**Complexity:** Low

#### What It Does

Schedules async tasks with dependencies, ensuring maximum parallelism. Very minimal API.

#### API Pattern

```rust
use async_dag::Graph;

// Create graph
let mut graph = Graph::new();

// Add tasks (closures returning async blocks)
let task1 = graph.add_task(|| async {
    // Read file A
    let content = read_file("a.qmd").await;
    content
});

let task2 = graph.add_task(|| async {
    // Read file B
    read_file("b.qmd").await
});

// Define function for dependent task
async fn parse_content(content: String) -> Ast {
    parse_qmd(&content)
}

// Add task3 that depends on task1's output
// The '0' means task1's output goes to parse_content's 0th parameter
let task3 = graph.add_child_task(task1, parse_content, 0).unwrap();

// Run the graph (executes with maximum parallelism)
graph.run().await;

// Get result
let ast = graph.get_value::<Ast>(task3).unwrap();
```

#### Key Features

- **Automatic parallelization** - Independent tasks run concurrently
- **Type-safe** - Values retrieved with explicit types
- **Error handling** - `TryGraph` variant for fallible tasks (fail-fast)
- **Simple** - No macros, minimal API surface

#### For qmd-syntax-helper

```rust
let mut graph = Graph::new();

// Add file read tasks (parallel)
let file_tasks: Vec<_> = file_paths.iter().map(|path| {
    graph.add_task(|| async move {
        fs::read_to_string(path).await
    })
}).collect();

// Add parse tasks (depend on file reads, share parse results)
let parse_tasks: Vec<_> = file_tasks.iter().map(|&file_task| {
    graph.add_child_task(file_task, parse_qmd, 0).unwrap()
}).collect();

// Add rule check tasks (depend on parse results)
for (i, &parse_task) in parse_tasks.iter().enumerate() {
    for rule in &rules {
        let rule_clone = rule.clone();
        let check_task = graph.add_task(|| async move {
            // Use parse result here
        });
        graph.update_dependency(parse_task, check_task, 0).unwrap();
    }
}

graph.run().await;
```

**Pros:**
- ✅ Very simple to understand and use
- ✅ Minimal dependencies
- ✅ Good for learning DAG concepts
- ✅ Automatic parallelization

**Cons:**
- ❌ Manual dependency wiring (verbose for complex graphs)
- ❌ Async overhead for CPU-bound work (parsing)
- ❌ No automatic memoization/caching
- ❌ No optimization of the graph itself

---

### 1.2 dagrs (Flow-Based Programming)

**Repository:** https://github.com/open-rust-initiative/dagrs
**Maturity:** Active (v0.5.0, June 2025)
**Complexity:** Medium

#### What It Does

Async task orchestration framework based on Flow-Based Programming (FBP). Treats applications as networks of "black box" processes communicating through channels.

#### API Pattern

```rust
use dagrs::*;

// Define custom task (implement Action trait)
struct ParseTask;

#[async_trait]
impl Action for ParseTask {
    async fn run(&self, input: &mut InChannels, output: &OutChannels, _: Arc<EnvVar>) {
        // Read from input channel
        let content: String = input.recv("file_content").await;

        // Parse
        let ast = parse_qmd(&content);

        // Send to output channel
        output.send("ast", ast).await;
    }
}

// Define graph
let mut graph = Graph::new();

// Add nodes
let read_node = graph.add_node(Box::new(ReadFileTask));
let parse_node = graph.add_node(Box::new(ParseTask));
let check_node = graph.add_node(Box::new(CheckTask));

// Connect nodes (channels)
graph.add_edge(read_node, parse_node, "file_content", "file_content");
graph.add_edge(parse_node, check_node, "ast", "ast");

// Execute
graph.run().await;
```

#### Key Features

- **Flow-based architecture** - Data flows through channels
- **Conditional nodes** - Control flow based on runtime data
- **Loop DAGs** - Cyclic execution for iterative algorithms
- **Custom parsers** - Define graphs in JSON/YAML/TOML
- **Built on Tokio** - Async execution

#### For qmd-syntax-helper

```rust
// Define tasks
struct ReadFileTask(PathBuf);
struct ParseTask;
struct CheckRuleTask { rule: Arc<dyn Rule> }

// Build graph
let mut graph = Graph::new();

for path in file_paths {
    let read = graph.add_node(Box::new(ReadFileTask(path.clone())));
    let parse = graph.add_node(Box::new(ParseTask));

    // Connect read → parse
    graph.add_edge(read, parse, "content", "content");

    // For each rule, connect parse → check
    for rule in &rules {
        let check = graph.add_node(Box::new(CheckRuleTask {
            rule: rule.clone()
        }));
        graph.add_edge(parse, check, "ast", "ast");
    }
}

graph.run().await;
```

**Pros:**
- ✅ FBP model natural for pipelines
- ✅ Conditional and loop support
- ✅ Config-driven graph building (YAML/JSON)
- ✅ Good documentation and examples

**Cons:**
- ❌ More complex than async_dag
- ❌ Channel overhead for CPU-bound tasks
- ❌ Still requires manual graph construction
- ❌ No automatic memoization

---

### 1.3 RustDagcuter

**Repository:** https://github.com/busyster996/RustDagcuter
**Maturity:** Early
**Complexity:** Low

#### What It Does

Executes DAGs of tasks with dependency management, circular dependency detection, and customizable lifecycles (PreExecution, Execute, PostExecution).

#### API Pattern

```rust
// Define task
struct ParseTask {
    file_path: PathBuf,
}

impl Task for ParseTask {
    fn pre_execution(&mut self) {
        println!("About to parse: {:?}", self.file_path);
    }

    fn execute(&mut self) -> Result<()> {
        // Parse logic
        Ok(())
    }

    fn post_execution(&mut self) {
        println!("Finished parsing");
    }
}

// Build DAG
let mut dag = DAG::new();
dag.add_task("parse_a", Box::new(ParseTask { ... }));
dag.add_task("check_a", Box::new(CheckTask { ... }));
dag.add_dependency("check_a", "parse_a"); // check depends on parse

// Execute
dag.run()?;
```

**Pros:**
- ✅ Lifecycle hooks (pre/post execution)
- ✅ Circular dependency detection
- ✅ Simple API

**Cons:**
- ❌ Less mature than others
- ❌ Sync only (no async)
- ❌ Limited documentation

---

## Category 2: Incremental Computation Frameworks

These frameworks automatically track dependencies and memoize results, eliminating manual DAG construction.

### 2.1 salsa ⭐ (Most Mature, Used by rust-analyzer)

**Repository:** https://github.com/salsa-rs/salsa
**Used By:** rust-analyzer, rustc query system, Mun language
**Maturity:** Production-grade
**Complexity:** Medium-High

#### What It Does

A generic framework for on-demand, incrementalized computation. You define a database of queries (like functions), and Salsa automatically:
- Memoizes results
- Tracks dependencies between queries
- Incrementally recomputes only what changed

#### Core Concepts

**Queries** - Functions from key K → value V
- **Input queries** - Base inputs (set by user)
- **Derived queries** - Pure functions of other queries

**Database** - Stores all query results and dependency tracking

**Incremental recomputation** - When inputs change, Salsa recomputes only affected queries

#### API Pattern

```rust
use salsa::{Database, Query};

// Define input
#[salsa::input]
pub struct SourceFile {
    pub path: PathBuf,
    #[return_ref]
    pub contents: String,
}

// Define derived queries
#[salsa::tracked]
fn parse_file(db: &dyn Db, file: SourceFile) -> Ast {
    let contents = file.contents(db);
    parse_qmd(contents)  // Expensive operation, but memoized!
}

#[salsa::tracked]
fn check_rule(db: &dyn Db, file: SourceFile, rule: RuleId) -> CheckResult {
    let ast = parse_file(db, file);  // Reuses cached parse if available!
    rule.check(&ast)
}

// Define database
#[salsa::database]
struct Database {
    storage: salsa::Storage<Self>,
}

impl salsa::Database for Database {}

// Usage
let mut db = Database::default();

// Set inputs
let file = SourceFile::new(&db,
    PathBuf::from("test.qmd"),
    "# Hello".to_string()
);

// Query (computed and cached)
let ast = parse_file(&db, file);

// Query again (reuses cached result!)
let ast2 = parse_file(&db, file);

// Modify input
file.set_contents(&mut db, "# Changed".to_string());

// Query (recomputes because input changed)
let ast3 = parse_file(&db, file);
```

#### Key Features

- **Automatic memoization** - No manual cache management
- **Dependency tracking** - Salsa tracks which queries read which other queries
- **Incremental** - Only recomputes affected queries when inputs change
- **Early cutoff** - If a query result doesn't change, dependent queries aren't recomputed
- **Durability** - Categorize queries by change frequency (stdlib vs user code)
- **Parallel** - `ParallelDatabase` trait for concurrent queries

#### For qmd-syntax-helper

```rust
// Define inputs
#[salsa::input]
struct QmdFile {
    path: PathBuf,
    #[return_ref]
    contents: String,
}

// Parse query (shared across rules)
#[salsa::tracked]
fn parse_qmd_file(db: &dyn Db, file: QmdFile) -> Arc<ParseResult> {
    let contents = file.contents(db);
    Arc::new(parse_qmd(contents))
}

// Rule-specific queries
#[salsa::tracked]
fn check_parse_rule(db: &dyn Db, file: QmdFile) -> CheckResult {
    let parse = parse_qmd_file(db, file);  // Shared!
    CheckResult {
        has_issue: !parse.success,
        // ...
    }
}

#[salsa::tracked]
fn check_div_whitespace(db: &dyn Db, file: QmdFile) -> CheckResult {
    let parse = parse_qmd_file(db, file);  // Reuses same cached parse!
    let diagnostics = &parse.diagnostics;
    // Check for div whitespace issues
    // ...
}

// Database setup
#[salsa::database]
struct HelperDb {
    storage: salsa::Storage<Self>,
}

impl salsa::Database for HelperDb {}

// Usage
fn main() {
    let db = HelperDb::default();

    // Add files
    for path in glob("**/*.qmd") {
        let contents = fs::read_to_string(&path)?;
        let file = QmdFile::new(&db, path, contents);

        // Check all rules (parse happens ONCE, shared across all!)
        let parse_check = check_parse_rule(&db, file);
        let div_check = check_div_whitespace(&db, file);
        let grid_check = check_grid_tables(&db, file);
        // ...
    }
}
```

**How It Eliminates Your Duplication:**

Currently in qmd-syntax-helper:
- `ParseChecker.check()` → parses file
- `DivWhitespaceConverter.check()` → parses same file again
- `DivWhitespaceConverter.convert()` → parses same file AGAIN

With Salsa:
- First call to `parse_qmd_file(db, file)` → computes and caches
- All subsequent calls → return cached result (no recomputation!)
- If file changes → automatic recomputation, but only for that file

**Pros:**
- ✅ Production-tested (rust-analyzer, rustc)
- ✅ Automatic dependency tracking
- ✅ Perfect for incremental compilation scenarios
- ✅ Eliminates manual cache invalidation
- ✅ Parallel query execution
- ✅ Natural "describe the computation" model

**Cons:**
- ❌ Learning curve (query-oriented thinking)
- ❌ Macro-heavy API (`#[salsa::tracked]`, etc.)
- ❌ More complex than simple DAG executors
- ❌ Overkill if you don't need incrementality

---

### 2.2 comemo ⭐ (Modern, from Typst compiler)

**Repository:** https://docs.rs/comemo/
**Used By:** Typst (modern typesetting system)
**Maturity:** Stable (v0.5.0)
**Complexity:** Medium

#### What It Does

**Constrained memoization** - Like regular memoization, but tracks *which specific data accesses* occur during computation. Only invalidates cache if accessed data changed.

#### The Problem It Solves

Regular memoization:
```rust
// Cached by entire Files object - if ANY file changes, cache invalidates!
fn evaluate(script: &str, files: Files) -> i32 { ... }
```

Constrained memoization:
```rust
// Cached by which specific files were accessed
// If script only reads "a.txt", changes to "b.txt" don't invalidate!
#[memoize]
fn evaluate(script: &str, files: Tracked<Files>) -> i32 { ... }
```

#### API Pattern

```rust
use comemo::{memoize, track, Tracked};

// Make type trackable
#[track]
impl Files {
    fn read(&self, path: &str) -> String {
        // Implementation
        // Accesses automatically recorded!
    }
}

// Memoized function
#[memoize]
fn evaluate(script: &str, files: Tracked<Files>) -> i32 {
    // When we call files.read("a.txt"), comemo records this access
    let content = files.read(script);

    // Parse and evaluate
    parse_and_eval(&content)
}

// Usage
let files = Files::new();
let files_tracked = Tracked::new(&files);

// First call - computed and cached
let result1 = evaluate("a.txt", files_tracked);

// Second call with same args - returns cached result
let result2 = evaluate("a.txt", files_tracked);

// Modify unrelated file
files.write("b.txt", "new content");

// Third call - STILL uses cached result!
// (because evaluate("a.txt") never accessed "b.txt")
let result3 = evaluate("a.txt", files_tracked);

// Modify accessed file
files.write("a.txt", "changed");

// Fourth call - recomputes (accessed file changed)
let result4 = evaluate("a.txt", files_tracked);
```

#### For qmd-syntax-helper

```rust
use comemo::{memoize, track, Tracked};

// Trackable file system
#[track]
impl FileSystem {
    fn read(&self, path: &PathBuf) -> String {
        fs::read_to_string(path).unwrap()
    }
}

// Memoized parse (shared across rules!)
#[memoize]
fn parse_qmd(path: &PathBuf, fs: Tracked<FileSystem>) -> Arc<ParseResult> {
    let content = fs.read(path);  // Access tracked!
    Arc::new(quarto_markdown_pandoc::read(&content))
}

// Rule checks also memoized
#[memoize]
fn check_parse_rule(path: &PathBuf, fs: Tracked<FileSystem>) -> CheckResult {
    let parse = parse_qmd(path, fs);  // Calls memoized function
    CheckResult {
        has_issue: !parse.success,
        // ...
    }
}

#[memoize]
fn check_div_whitespace(path: &PathBuf, fs: Tracked<FileSystem>) -> CheckResult {
    let parse = parse_qmd(path, fs);  // Reuses cached parse!
    find_div_whitespace_errors(&parse)
}

// Usage
fn main() {
    let fs = FileSystem::new();
    let fs_tracked = Tracked::new(&fs);

    for path in glob("**/*.qmd") {
        // All rules share the same memoized parse!
        let parse_check = check_parse_rule(&path, fs_tracked);
        let div_check = check_div_whitespace(&path, fs_tracked);
        // First rule triggers parse, subsequent rules reuse
    }
}
```

**Key Insight:** You don't build a DAG explicitly. You just write functions and annotate them with `#[memoize]`. Comemo automatically:
1. Tracks function calls
2. Records data accesses
3. Caches results
4. Invalidates only when accessed data changes

**Pros:**
- ✅ Simpler than Salsa (fewer concepts)
- ✅ Automatic fine-grained dependency tracking
- ✅ No manual cache invalidation
- ✅ Modern, actively maintained (Typst team)
- ✅ Good documentation
- ✅ Minimal API surface

**Cons:**
- ❌ Less mature than Salsa (but used in production by Typst)
- ❌ Requires `Tracked<T>` wrapper (changes function signatures)
- ❌ No built-in parallelism (you combine with rayon yourself)

---

### 2.3 adapton

**Repository:** https://docs.rs/adapton/
**Maturity:** Research project
**Complexity:** High

The original research project that inspired Salsa. Implements demanded computation graphs (DCG) with memoization. More academic and less ergonomic than Salsa or comemo.

**Verdict:** Use Salsa or comemo instead (more modern, better APIs).

---

### 2.4 granularity

**Repository:** https://github.com/pragmatrix/granularity
**Maturity:** Experimental
**Complexity:** Medium

Fine-grained reactive graph with automatic dependency tracking. Similar to Salsa/comemo but incomplete.

**Verdict:** Too experimental for production use.

---

## Category 3: Dataflow Systems

These are for stream processing and distributed computation. Overkill for qmd-syntax-helper.

### 3.1 timely-dataflow

**Repository:** https://github.com/TimelyDataflow/timely-dataflow

Distributed dataflow runtime where computation is modeled as a graph of operators. Designed for large-scale stream processing (think: distributed systems, big data).

**Verdict:** Massive overkill for file processing.

---

### 3.2 differential-dataflow

**Repository:** https://lib.rs/crates/differential-dataflow

Incremental data-parallel dataflow platform built on timely-dataflow. Updates results when inputs change.

**Verdict:** Also overkill.

---

## Recommendation for qmd-syntax-helper

Given your goals:
1. Learn the pattern for a future project
2. Solve the actual parse duplication problem
3. Support multi-threading

Here are three approaches, ordered by complexity:

### Option A: Start Simple - async_dag

**Pros:**
- ✅ Minimal learning curve
- ✅ Explicit DAG construction (see exactly what's happening)
- ✅ Good for understanding the pattern
- ✅ Low code churn

**Implementation effort:** ~6-8 hours

**Example structure:**
```rust
// Build graph
let mut graph = Graph::new();

// File read tasks (parallel)
let file_tasks = paths.map(|p| graph.add_task(|| async { read(p) }));

// Parse tasks (depend on file reads, run in parallel)
let parse_tasks = file_tasks.map(|ft|
    graph.add_child_task(ft, parse_qmd, 0)
);

// Check tasks (depend on parses, run in parallel)
for (pt, rule) in parse_tasks.iter().cartesian_product(rules) {
    graph.add_child_task(*pt, rule.check, 0);
}

graph.run().await;
```

**Limitations:**
- Manual graph construction (verbose)
- Async overhead for CPU-bound parsing
- No automatic optimization

---

### Option B: Modern and Elegant - comemo

**Pros:**
- ✅ Automatic dependency tracking (no manual DAG)
- ✅ Natural Rust code (just add `#[memoize]`)
- ✅ From production compiler (Typst)
- ✅ Eliminates parse duplication automatically
- ✅ Combines well with Rayon for parallelism

**Implementation effort:** ~10-12 hours (includes learning curve)

**Example structure:**
```rust
#[track]
impl FileSystem {
    fn read(&self, path: &Path) -> String { ... }
}

#[memoize]
fn parse_file(path: &Path, fs: Tracked<FileSystem>) -> Arc<ParseResult> {
    let content = fs.read(path);
    Arc::new(parse_qmd(&content))
}

#[memoize]
fn check_rule(
    path: &Path,
    rule_id: RuleId,
    fs: Tracked<FileSystem>
) -> CheckResult {
    let parse = parse_file(path, fs);  // Automatically reused!
    rules[rule_id].check(parse)
}

// Usage with Rayon
paths.par_iter().flat_map(|path| {
    rules.par_iter().map(|rule| {
        check_rule(path, rule.id, fs_tracked)
    })
}).collect()
```

**Why I like this:**
- Declarative ("these functions are expensive, memoize them")
- No manual DAG wiring
- Plays well with Rayon
- Modern Rust idioms

---

### Option C: Production-Grade - salsa

**Pros:**
- ✅ Most mature (powers rust-analyzer)
- ✅ Automatic everything (memoization, dependency tracking, incremental)
- ✅ Built-in parallelism
- ✅ Incremental compilation support (reuse across runs)
- ✅ Best for long-term scalability

**Implementation effort:** ~16-20 hours (steeper learning curve)

**Example structure:**
```rust
#[salsa::input]
struct QmdFile {
    path: PathBuf,
    #[return_ref]
    contents: String,
}

#[salsa::tracked]
fn parse_file(db: &dyn Db, file: QmdFile) -> Arc<ParseResult> {
    let contents = file.contents(db);
    Arc::new(parse_qmd(contents))
}

#[salsa::tracked]
fn check_all_rules(db: &dyn Db, file: QmdFile) -> Vec<CheckResult> {
    let parse = parse_file(db, file);  // Automatically shared!
    rules.iter().map(|r| r.check(&parse)).collect()
}

#[salsa::database]
struct Db { storage: salsa::Storage<Self> }

// Usage
let db = Db::default();
for path in paths {
    let file = QmdFile::new(&db, path, read_file(path));
    check_all_rules(&db, file);  // Parallelized automatically
}
```

**Why it's powerful:**
- Query-oriented design scales to complex scenarios
- Incremental across runs (cache survives program restart)
- Parallel by default
- Used in production compilers

---

## My Recommendation: comemo + Rayon

For qmd-syntax-helper specifically, I'd go with **comemo** because:

1. **Solves the actual problem:** Parse duplication eliminated automatically
2. **Good learning:** Teaches memoization and dependency tracking patterns
3. **Transferable knowledge:** Similar concepts to Salsa, but simpler
4. **Practical:** From a production compiler (Typst)
5. **Composable:** Works with Rayon for parallelism

**Implementation plan:**

```rust
// Phase 1: Add comemo infrastructure
#[track]
impl FileContext {
    fn read(&self, path: &Path) -> String { ... }
}

// Phase 2: Memoize expensive operations
#[memoize]
fn parse_qmd_file(path: &Path, ctx: Tracked<FileContext>) -> Arc<ParseResult> {
    let content = ctx.read(path);
    Arc::new(parse(content))
}

// Phase 3: Memoize rule checks
#[memoize]
fn check_parse_rule(path: &Path, ctx: Tracked<FileContext>) -> CheckResult {
    let parse = parse_qmd_file(path, ctx);  // Reuses cached parse!
    check_parse_success(&parse)
}

// Phase 4: Combine with Rayon for parallelism
paths.par_iter().flat_map(|path| {
    rules.par_iter().map(|rule| {
        check_rule(path, rule, ctx_tracked)  // Memoized + parallel!
    })
}).collect()
```

**If you want maximum learning value for your future project:** Use **salsa**, because:
- It's the pattern you'll see in compilers and analyzers
- More sophisticated dependency tracking
- Incremental computation across runs
- But be prepared for the learning curve

---

## Comparison Table

| Library | Complexity | Maturity | Auto Memo | Auto Deps | Parallel | Use Case |
|---------|-----------|----------|-----------|-----------|----------|----------|
| **async_dag** | Low | Stable | ❌ | ❌ | ✅ | Simple task graphs |
| **dagrs** | Medium | Active | ❌ | ❌ | ✅ | Flow-based pipelines |
| **comemo** | Medium | Stable | ✅ | ✅ | ➖* | Incremental compilers |
| **salsa** | High | Production | ✅ | ✅ | ✅ | Query-based systems |
| **timely** | Very High | Production | ❌ | ❌ | ✅ | Distributed streams |

*Comemo doesn't provide parallelism itself, but composes with Rayon.

---

## Code Example: How These Would Look in Practice

### Current qmd-syntax-helper (Sequential, Duplicated Parsing)

```rust
for path in paths {
    for rule in rules {
        // Each rule independently reads and parses!
        let result = rule.check(path);  // 3+ parses per file!
    }
}
```

### With async_dag (Explicit DAG)

```rust
let mut graph = Graph::new();

for path in paths {
    let file_task = graph.add_task(|| async { read(path) });
    let parse_task = graph.add_child_task(file_task, parse, 0);

    for rule in rules {
        let check_task = graph.add_child_task(parse_task, rule.check, 0);
    }
}

graph.run().await;  // Automatic parallelism, parse shared per file
```

### With comemo (Automatic Memoization)

```rust
#[memoize]
fn parse(path: &Path, ctx: Tracked<Ctx>) -> Arc<Parse> { ... }

#[memoize]
fn check(path: &Path, rule: RuleId, ctx: Tracked<Ctx>) -> Result {
    let parse = parse(path, ctx);  // First call computes, rest reuse!
    rules[rule].check(parse)
}

// Rayon for parallelism
paths.par_iter().flat_map(|path| {
    rules.par_iter().map(|rule| check(path, rule.id, ctx))
}).collect()
```

### With salsa (Query System)

```rust
#[salsa::tracked]
fn parse(db: &dyn Db, file: File) -> Arc<Parse> { ... }

#[salsa::tracked]
fn check(db: &dyn Db, file: File, rule: RuleId) -> Result {
    let parse = parse(db, file);  // Automatic sharing!
    rules[rule].check(parse)
}

let db = Db::default();
for path in paths {
    let file = File::new(&db, path, read(path));
    for rule in rules {
        check(&db, file, rule);  // Parallel + memoized
    }
}
```

---

## Next Steps

What would you like to do?

1. **Prototype with comemo** - Modernize qmd-syntax-helper with automatic memoization
2. **Learn with async_dag** - Simple explicit DAG for educational value
3. **Go full salsa** - Query system for maximum sophistication
4. **Hybrid approach** - Start with comemo, consider salsa later

I can help implement any of these!
