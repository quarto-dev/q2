# JavaScript Execution Performance Considerations

**Created**: 2026-01-12
**Status**: Active design document
**Related code**: `crates/quarto-system-runtime/src/js_native.rs`

## Overview

This document captures the architectural decision to create a fresh V8 JsRuntime
per JavaScript operation in `NativeRuntime`, the performance implications, and
the optimization path if/when performance becomes a concern.

## Current Implementation

```rust
// In native.rs
async fn render_ejs(&self, template: &str, data: &serde_json::Value) -> RuntimeResult<String> {
    // Create a fresh JsEngine for each call.
    // V8's JsRuntime is not Send+Sync, so we can't store it in NativeRuntime.
    let mut engine = JsEngine::new()?;
    engine.render_ejs(template, data)
}
```

**Why this approach?**

1. **Correctness**: V8's `JsRuntime` is not `Send + Sync`. Our `SystemRuntime` trait
   requires `Send + Sync` for use across async contexts. Storing JsRuntime in the
   struct would make `NativeRuntime` non-Send.

2. **Simplicity**: Creating on demand is straightforward to understand and test.

3. **Adequate for immediate use case**: Project scaffolding involves 1-10 templates.

## Performance Characteristics

### What happens per call

Each `render_ejs()` or `js_render_simple_template()` call:

1. **Creates V8 isolate** (~15-25ms)
   - Allocates heap (lazy, but setup is not)
   - Initializes garbage collector
   - Creates built-in objects (Object, Array, Function, etc.)
   - Sets up JIT compilation infrastructure

2. **Loads JS bundle** (~5-10ms)
   - Parses JavaScript (~50KB for EJS bundle)
   - Compiles to bytecode
   - Creates global `ejs` object

3. **Renders template** (~1-5ms)
   - The actual work - typically fast

**Total: ~20-35ms per operation**

### Scaling estimates

| Templates | Current Approach | With Thread-Local | With Batch API |
|-----------|------------------|-------------------|----------------|
| 1         | 30ms             | 30ms              | 30ms           |
| 10        | 300ms            | 75ms              | 40ms           |
| 100       | 3s               | 130ms             | 60ms           |
| 1,000     | 30s              | 1s                | 350ms          |
| 10,000    | 5 min            | 10s               | 3s             |

### Real-world scenarios

**Project scaffolding** (immediate use case):
- 1-10 templates per project creation
- Current approach: 30-300ms
- **Verdict: Acceptable**

**Listing pages in Quarto websites**:
- Every page could have a "read also" listing
- Large sites: 100-10,000 pages
- If each page needs template rendering during build: problematic
- **Verdict: Would need optimization**

**Live preview in hub-client**:
- Uses WASM runtime, different implementation
- Not affected by native performance
- **Verdict: N/A for this document**

## Optimization Strategies

### Strategy 1: Thread-Local Storage (Recommended First Step)

Store JsEngine in thread-local storage, reuse across calls on same thread.

```rust
use std::cell::RefCell;

thread_local! {
    static JS_ENGINE: RefCell<Option<JsEngine>> = RefCell::new(None);
}

impl NativeRuntime {
    fn with_js_engine<F, R>(&self, f: F) -> RuntimeResult<R>
    where
        F: FnOnce(&mut JsEngine) -> RuntimeResult<R>,
    {
        JS_ENGINE.with(|cell| {
            let mut borrow = cell.borrow_mut();
            if borrow.is_none() {
                *borrow = Some(JsEngine::new()?);
            }
            f(borrow.as_mut().unwrap())
        })
    }
}

#[async_trait]
impl SystemRuntime for NativeRuntime {
    async fn render_ejs(&self, template: &str, data: &serde_json::Value) -> RuntimeResult<String> {
        self.with_js_engine(|engine| engine.render_ejs(template, data))
    }
}
```

**Characteristics**:
- First call per thread: ~30ms (creates JsEngine)
- Subsequent calls: ~5ms (reuses JsEngine)
- Thread-safe by construction (each thread has its own)
- Works naturally with thread pools (tokio, rayon)
- ~20 lines of change
- **No API changes required**

**Caveats**:
- JsEngine memory persists for thread lifetime
- Need to handle potential JsEngine corruption (unlikely but possible)
- Could add `js_engine_reset()` method for explicit cleanup if needed

### Strategy 2: Dedicated JS Thread

Run all JS on a single dedicated thread, communicate via channels.

```rust
use std::sync::mpsc;
use std::thread;

enum JsRequest {
    RenderEjs { template: String, data: serde_json::Value, reply: oneshot::Sender<RuntimeResult<String>> },
    RenderSimple { template: String, data: serde_json::Value, reply: oneshot::Sender<RuntimeResult<String>> },
}

struct JsExecutor {
    tx: mpsc::Sender<JsRequest>,
}

impl JsExecutor {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut engine = JsEngine::new().expect("Failed to create JsEngine");
            while let Ok(request) = rx.recv() {
                match request {
                    JsRequest::RenderEjs { template, data, reply } => {
                        let _ = reply.send(engine.render_ejs(&template, &data));
                    }
                    // ...
                }
            }
        });
        Self { tx }
    }
}
```

**Characteristics**:
- Single JsEngine, maximum reuse
- All JS serialized through one thread
- Good for consistent memory usage
- More complex implementation
- Channel overhead per call (~1-2ms)

**When to use**: If memory is constrained and you want predictable JS memory usage.

### Strategy 3: Batch API

Add explicit batch methods for rendering multiple templates.

```rust
// In SystemRuntime trait
async fn render_ejs_batch(
    &self,
    templates: &[(&str, &serde_json::Value)],
) -> RuntimeResult<Vec<String>> {
    // Default implementation: call render_ejs in loop
    let mut results = Vec::with_capacity(templates.len());
    for (template, data) in templates {
        results.push(self.render_ejs(template, data).await?);
    }
    Ok(results)
}

// Optimized implementation in NativeRuntime
async fn render_ejs_batch(
    &self,
    templates: &[(&str, &serde_json::Value)],
) -> RuntimeResult<Vec<String>> {
    let mut engine = JsEngine::new()?;
    templates
        .iter()
        .map(|(template, data)| engine.render_ejs(template, data))
        .collect()
}
```

**Characteristics**:
- Explicit about performance characteristics
- Callers must batch their work
- Best performance for known batch sizes
- Requires trait API addition (but with default impl, backward compatible)

**When to use**: When callers naturally have batches (e.g., rendering all listing pages).

### Strategy 4: JsEngine Pool

Maintain a pool of pre-initialized JsEngines.

```rust
use crossbeam::queue::ArrayQueue;
use std::sync::Arc;

struct JsEnginePool {
    engines: ArrayQueue<JsEngine>,
    max_size: usize,
}

impl JsEnginePool {
    fn acquire(&self) -> RuntimeResult<PooledEngine> {
        match self.engines.pop() {
            Some(engine) => Ok(PooledEngine { engine, pool: self }),
            None => Ok(PooledEngine { engine: JsEngine::new()?, pool: self }),
        }
    }
}

struct PooledEngine<'a> {
    engine: JsEngine,
    pool: &'a JsEnginePool,
}

impl Drop for PooledEngine<'_> {
    fn drop(&mut self) {
        // Return to pool if not full
        let _ = self.pool.engines.push(std::mem::take(&mut self.engine));
    }
}
```

**Characteristics**:
- Bounded memory usage
- Good parallelism (multiple concurrent renders)
- More complex implementation
- Need to handle pool exhaustion

**When to use**: High-concurrency scenarios with memory constraints.

## Recommended Migration Path

1. **Now**: Keep current simple implementation. Document this decision (this file).

2. **If >100 templates become common**: Implement thread-local storage (Strategy 1).
   - Transparent change, no API impact
   - ~20 lines of code
   - 10-50x speedup for repeated operations

3. **If batch operations are identified**: Add batch API (Strategy 3).
   - Backward compatible trait addition
   - Callers can opt-in for better performance

4. **If memory is constrained**: Consider dedicated thread (Strategy 2) or pool (Strategy 4).

## Monitoring

To determine if optimization is needed, add timing instrumentation:

```rust
async fn render_ejs(&self, template: &str, data: &serde_json::Value) -> RuntimeResult<String> {
    let start = std::time::Instant::now();
    let result = /* actual work */;
    tracing::debug!(
        elapsed_ms = start.elapsed().as_millis(),
        template_len = template.len(),
        "render_ejs completed"
    );
    result
}
```

Look for:
- Many sequential render_ejs calls in logs
- Total JS time dominating build time
- User reports of slow project creation or builds

## Decision Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-01-12 | Fresh JsEngine per call | Simplicity, correctness, adequate for scaffolding |
| | | Optimization path documented, no API lock-in |

## References

- `crates/quarto-system-runtime/src/js_native.rs` - Implementation
- `crates/quarto-system-runtime/src/native.rs` - NativeRuntime integration
- deno_core documentation: https://docs.rs/deno_core
- V8 isolate model: https://v8.dev/docs/embed
