# Why Are DAG Task Executors Sparse in Rust?

**Date:** 2025-10-22
**Context:** Looking for libraries that do "plan DAG → execute with parallelism + work sharing"

---

## The Gap

We're looking for libraries that:
1. Let you define a DAG of tasks with dependencies
2. Execute with maximum parallelism
3. **Deduplicate shared work** (e.g., "parse file A" task is shared by multiple downstream tasks)
4. One-shot execution (not incremental/persistent memoization)

**What exists:**
- ✅ **Simple DAG executors** (`async_dag`, `dagrs`) - but they're not super popular
- ✅ **Incremental computation** (`salsa`, `comemo`) - but that's overkill for one-shot runs
- ✅ **Data parallelism** (`rayon`) - but no automatic task deduplication
- ✅ **Build systems** (`ninja`, `cargo`) - but not general-purpose libraries

**What's missing:**
- A popular, general-purpose "task DAG executor with work sharing" library

---

## Why This Gap Exists

### Hypothesis 1: Rayon Is "Good Enough" for Most Cases

Most Rust developers solve this with **Rayon + manual deduplication**:

```rust
// Compute shared work first (sequential)
let parsed: Vec<_> = files.iter()
    .map(|f| (f, parse(f)))  // Pre-compute parses
    .collect();

// Then parallelize downstream work
parsed.par_iter()
    .flat_map(|(file, parse)| {
        rules.par_iter().map(|rule| {
            rule.check(parse)  // Reuses pre-computed parse
        })
    })
    .collect()
```

**Why this works:**
- Rayon handles parallelism
- Manual staging handles deduplication
- Simple mental model
- No external dependencies

**Why it's not great:**
- Requires manual "staging" of work
- Not composable (hard to express complex dependency graphs)
- Memory usage (all intermediate results in memory)

### Hypothesis 2: Build Systems Cover the Use Case

When you need sophisticated DAG execution, you often just use a build system:
- **Cargo** for Rust builds
- **Ninja/Make** for C++ builds
- **Bazel** for polyglot builds

These handle:
- Parallel execution
- Dependency tracking
- Work deduplication (don't rebuild if inputs unchanged)
- Incremental builds

**Why this doesn't help us:**
- Build systems are for build workflows, not general computation
- Hard to embed in applications
- Overkill for in-process task scheduling

### Hypothesis 3: Async + Future Combinators Cover the Use Case

For I/O-bound workloads, people use `tokio` or `async-std` with future combinators:

```rust
use tokio::sync::Mutex;
use std::sync::Arc;

// Shared parse task (runs once, shared by all)
let parse_task = Arc::new(Mutex::new(None));

let checks = futures::join_all(
    rules.iter().map(|rule| {
        let pt = parse_task.clone();
        async move {
            // Get or compute parse
            let parse = {
                let mut guard = pt.lock().await;
                if guard.is_none() {
                    *guard = Some(parse_file().await);
                }
                guard.as_ref().unwrap().clone()
            };

            rule.check(&parse)
        }
    })
).await;
```

**Why this works:**
- Built into async ecosystem
- Natural for I/O-bound work
- No external dependencies

**Why it's not great:**
- Manual work sharing (Arc<Mutex<Option<T>>>)
- Async overhead for CPU-bound work
- Still not DAG-oriented

### Hypothesis 4: The Problem Is Actually Rare

Maybe the "DAG with work sharing" problem doesn't come up often enough to justify popular libraries?

**Counter-examples where it DOES come up:**
- **Compilers** - Parse once, run multiple passes
  - Solution: `salsa` (incremental), or custom task graphs
- **Build systems** - Compile each file once, link together
  - Solution: Cargo, Ninja, Make
- **Data pipelines** - Transform data through stages
  - Solution: `timely-dataflow` (distributed), `rayon` (data-parallel)
- **Testing frameworks** - Run tests in parallel, share setup
  - Solution: Custom test harnesses (`cargo test` does this internally)

So it DOES come up, but solutions are domain-specific.

### Hypothesis 5: The API Is Hard to Get Right

What would the perfect API look like?

**Option A: Explicit DAG construction** (like `async_dag`)
```rust
let mut graph = Graph::new();
let t1 = graph.add_task(|| parse());
let t2 = graph.add_child_task(t1, check_rule1, 0);
let t3 = graph.add_child_task(t1, check_rule2, 0);  // Shares t1!
graph.run();
```

**Pros:** Explicit, obvious what's happening
**Cons:** Verbose, manual wiring

**Option B: Automatic memoization** (like `comemo`)
```rust
#[memoize]
fn parse(file: &File) -> Parse { ... }

#[memoize]
fn check(file: &File, rule: RuleId) -> Result {
    let parse = parse(file);  // Automatically shared!
    rules[rule].check(parse)
}
```

**Pros:** Automatic, less boilerplate
**Cons:** Hidden magic, requires framework

**Option C: Futures with caching** (like `deduplicate`)
```rust
let dedup = Deduplicate::new(parse_fn);
let parses = futures::join_all(
    files.iter().map(|f| dedup.get(f))  // Automatically deduped!
);
```

**Pros:** Composable with async ecosystem
**Cons:** Async overhead, still manual composition

**None of these are obviously "the right answer"**, which might explain why there's no dominant library.

### Hypothesis 6: Rust's Ownership Makes DAGs Hard

DAGs with shared work are actually **somewhat at odds with Rust's ownership model**.

Consider:
- Task A produces value V
- Tasks B and C both consume V
- In a DAG, B and C "share" V

**In Rust:**
- Does A own V? (No, it returns it)
- Do B and C own V? (Can't both own it)
- Is V cloned? (Expensive)
- Is V behind Arc? (Runtime overhead)

Most DAG executors end up using `Arc<T>` everywhere, which adds runtime overhead and hides the ownership model.

**Example:**
```rust
// Task returns Arc to enable sharing
fn parse() -> Arc<ParseResult> {
    Arc::new(expensive_parse())
}

// Downstream tasks take Arc
fn check(parse: Arc<ParseResult>) -> CheckResult {
    check_impl(&parse)  // Deref through Arc
}
```

This works, but it's not as ergonomic as languages with GC (where sharing is free).

---

## So What Should We Use?

### For qmd-syntax-helper specifically:

Given the constraints:
- One-shot execution (not incremental)
- Coarse-grained tasks (parsing is expensive)
- Clear dependency structure
- Want learning value

**I recommend: Custom DAG executor using `DashMap` + `tokio::sync::OnceCell`**

Here's why:
1. **Simple to implement** (~200 lines of code)
2. **Explicit DAG construction** (learning value)
3. **Work sharing via OnceCell** (first caller computes, others wait)
4. **Rayon for parallelism** (no async overhead)
5. **Full control** (can optimize for your use case)

---

## Sketch: Custom DAG Executor

```rust
use dashmap::DashMap;
use std::sync::Arc;
use rayon::prelude::*;

// Task ID (unique identifier)
type TaskId = String;

// Task graph
struct TaskGraph {
    // Task ID → Task implementation
    tasks: DashMap<TaskId, Box<dyn Task>>,

    // Task ID → Cached result (OnceCell ensures single execution)
    cache: DashMap<TaskId, Arc<tokio::sync::OnceCell<Box<dyn Any>>>>,
}

trait Task: Send + Sync {
    fn dependencies(&self) -> Vec<TaskId>;
    fn execute(&self, deps: &[Box<dyn Any>]) -> Box<dyn Any>;
}

impl TaskGraph {
    fn add_task(&self, id: TaskId, task: Box<dyn Task>) {
        self.tasks.insert(id.clone(), task);
        self.cache.insert(id, Arc::new(tokio::sync::OnceCell::new()));
    }

    fn run(&self, task_id: &TaskId) -> Box<dyn Any> {
        let cell = self.cache.get(task_id).unwrap();

        // OnceCell ensures only first caller executes, others wait
        cell.get_or_init(|| {
            let task = self.tasks.get(task_id).unwrap();

            // Recursively run dependencies (parallel!)
            let dep_results: Vec<_> = task.dependencies()
                .par_iter()  // Parallel!
                .map(|dep_id| self.run(dep_id))
                .collect();

            // Execute this task with dependency results
            task.execute(&dep_results)
        }).clone()
    }
}
```

**Usage:**

```rust
// Define tasks
struct ParseTask(PathBuf);
impl Task for ParseTask {
    fn dependencies(&self) -> Vec<TaskId> { vec![] }

    fn execute(&self, _: &[Box<dyn Any>]) -> Box<dyn Any> {
        let content = fs::read_to_string(&self.0).unwrap();
        Box::new(Arc::new(parse_qmd(&content)))
    }
}

struct CheckTask {
    rule: Arc<dyn Rule>,
    parse_task_id: TaskId,
}
impl Task for CheckTask {
    fn dependencies(&self) -> Vec<TaskId> {
        vec![self.parse_task_id.clone()]
    }

    fn execute(&self, deps: &[Box<dyn Any>]) -> Box<dyn Any> {
        let parse = deps[0].downcast_ref::<Arc<ParseResult>>().unwrap();
        Box::new(self.rule.check(parse))
    }
}

// Build graph
let graph = TaskGraph::new();

for path in file_paths {
    let parse_id = format!("parse:{}", path.display());
    graph.add_task(parse_id.clone(), Box::new(ParseTask(path.clone())));

    for rule in &rules {
        let check_id = format!("check:{}:{}", path.display(), rule.name());
        graph.add_task(check_id.clone(), Box::new(CheckTask {
            rule: rule.clone(),
            parse_task_id: parse_id.clone(),  // Share parse task!
        }));
    }
}

// Execute all check tasks (parallelized automatically!)
let results: Vec<_> = graph.tasks.keys()
    .filter(|id| id.starts_with("check:"))
    .par_iter()
    .map(|id| graph.run(id))
    .collect();
```

**Why this works:**
- ✅ Explicit DAG (you see all tasks and dependencies)
- ✅ Automatic work sharing (OnceCell deduplicates)
- ✅ Parallel execution (Rayon)
- ✅ Type-safe-ish (Box<dyn Any> + downcasting)
- ✅ No async overhead (uses blocking OnceCell, not async)
- ✅ ~200 lines of code (maintainable)

**Downsides:**
- ❌ Type erasure (Box<dyn Any>)
- ❌ Manual graph construction
- ❌ No built-in error handling (need to add)

---

## Alternative: Just Use async_dag

If you don't want to write custom code, `async_dag` is actually fine:

```rust
let mut graph = Graph::new();

// Parse tasks
let parse_tasks: HashMap<_, _> = file_paths.iter()
    .map(|path| {
        let p = path.clone();
        let task = graph.add_task(|| async move {
            let content = fs::read_to_string(&p).await?;
            Arc::new(parse_qmd(&content))
        });
        (path.clone(), task)
    })
    .collect();

// Check tasks
for (path, parse_task) in &parse_tasks {
    for rule in &rules {
        let r = rule.clone();
        graph.add_child_task(*parse_task, move |parse: Arc<ParseResult>| async move {
            r.check(&parse)
        }, 0).unwrap();
    }
}

graph.run().await;
```

**Pros:**
- ✅ Already exists (no custom code)
- ✅ Simple API
- ✅ Work sharing (parse tasks reused)

**Cons:**
- ❌ Async overhead for CPU-bound parsing
- ❌ Manual wiring

---

## Recommendation

For qmd-syntax-helper:

1. **If you want learning value:** Implement custom DAG executor (~200 lines)
   - Good for understanding the pattern
   - Full control
   - Transfers to other projects

2. **If you want fast implementation:** Use `async_dag`
   - Already exists
   - Simple API
   - Good enough for your use case

3. **If you want elegance:** Use `comemo` (even though it's overkill)
   - Automatic memoization is nice
   - Modern Rust patterns
   - No manual DAG wiring

I'd actually lean toward **option 1 (custom)** because:
- It's the pattern you want to learn
- It's not that much code
- You get exactly what you need
- Transferable to your future project

Want me to prototype the custom DAG executor?
