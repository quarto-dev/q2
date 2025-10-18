# Performance Strategy for Rust LSP

## Context

The quarto-markdown parser previously had performance issues (String clone() overhead) that have been resolved. However, comprehensive performance measurement and profiling must be integrated into the LSP development plan to ensure the Rust LSP meets latency targets.

## Performance-First Approach

### Phase 0: Baseline Benchmarks (Critical)

**Before any LSP work begins**, establish performance baseline:

1. **Benchmark quarto-markdown parsing**
   - File sizes: 1KB, 10KB, 100KB, 1MB
   - Measure: parse time, memory usage, AST size
   - Test with real Quarto documents from quarto-web

2. **Profile parsing hot paths**
   - Use `cargo flamegraph` or `perf`
   - Identify any remaining bottlenecks
   - Establish baseline for optimization

3. **Set clear targets** for LSP use case
   - Document open: <100ms
   - Incremental update: <50ms
   - Memory per cached AST: <1MB

## Performance Targets

| Operation | Target | Rationale |
|-----------|--------|-----------|
| Parse small (1KB) | <10ms | Near-instant feedback |
| Parse medium (10KB) | <50ms | Typical document |
| Parse large (100KB) | <200ms | Acceptable for large docs |
| Incremental update | <50ms | Smooth typing |
| Document symbols | <30ms | Fast outline refresh |
| Go to definition | <50ms | Responsive navigation |
| Completions | <50ms | No typing lag |
| Diagnostics | <200ms | Acceptable delay |
| LSP startup | <100ms | Fast activation |

## Instrumentation Strategy

### Built-In from Day 1

```rust
use tracing::{info_span, instrument};

#[instrument(skip(self))]
async fn did_open(&self, params: DidOpenTextDocumentParams) {
    let _parse = info_span!("parse_document").entered();
    // ... parsing
    drop(_parse);

    let _diag = info_span!("compute_diagnostics").entered();
    // ... diagnostics
}
```

### Continuous Measurement

Every phase includes performance validation:

- **Phase 1**: Measure parse times in LSP context
- **Phase 2**: Benchmark AST-based features (symbols, folding, links)
- **Phase 3**: Profile navigation features
- **Phase 4**: Measure completion latency
- **Phase 5**: Benchmark diagnostic computation
- **Phase 7**: Comprehensive profiling and optimization

## Profiling Tools

1. **cargo-flamegraph**: CPU time visualization
2. **perf** (Linux): Performance counters
3. **Instruments** (macOS): Time profiler, allocations
4. **valgrind/massif**: Memory profiling
5. **tracing**: Runtime instrumentation with spans

## Benchmarking Infrastructure

### Criterion.rs Setup

```rust
// benches/lsp_benchmarks.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_parse_typical(c: &mut Criterion) {
    let content = include_str!("../fixtures/typical.qmd");
    c.bench_function("parse_typical_10kb", |b| {
        b.iter(|| parse_document(black_box(content)))
    });
}

fn bench_incremental_edit(c: &mut Criterion) {
    // Setup: parse initial document
    // Bench: apply small edit and re-parse
}

criterion_group!(benches, bench_parse_typical, bench_incremental_edit);
criterion_main!(benches);
```

### CI Integration

```yaml
# .github/workflows/benchmarks.yml
name: Performance Regression Check
on: [pull_request]
jobs:
  bench:
    runs-on: ubuntu-latest
    steps:
      - run: cargo bench
      - uses: benchmark-action/github-action-benchmark@v1
        with:
          fail-on-alert: true
          alert-threshold: '120%'  # Fail if >20% slower
```

## Comparison with TypeScript LSP

### Methodology

**Apples-to-apples comparison**:
- Same documents from quarto-web
- Same operations (symbols, goto def, completions)
- Same VS Code version
- Warm cache (measure steady-state)
- Multiple runs, report median

### Expected Improvements

Based on Rust vs Node.js characteristics:

| Metric | TypeScript | Rust (Target) | Improvement |
|--------|------------|---------------|-------------|
| Startup | 500-1000ms | <100ms | 5-10x |
| Memory | 50-100MB | 20-40MB | 2-3x |
| Parse latency | ~50ms | <30ms | 1.5-2x |
| Response time | ~30ms | <20ms | 1.5x |

## Optimization Strategy

### Phase 7: Comprehensive Optimization

1. **Profile with flamegraph**
   - Identify allocation hot paths
   - Find unnecessary clones
   - Locate cache misses

2. **Incremental parsing**
   ```rust
   // tree-sitter reuses unchanged parse nodes
   let new_tree = parser.parse_with(Some(&old_tree), input_callback);
   ```

3. **Cache tuning**
   - Measure hit rates
   - Optimize eviction policies
   - Balance memory vs speed

4. **Memory optimization**
   - Use `Arc` for shared data
   - Reduce AST node size
   - Pool allocations where beneficial

## Success Criteria

✅ **Performance**:
- All operations meet target latencies
- Memory usage scales linearly with project size
- Startup time <100ms
- No performance regressions in CI

✅ **Measurability**:
- Comprehensive benchmarks in place
- Profiling infrastructure working
- CI catches regressions

✅ **Documentation**:
- Performance characteristics documented
- Comparison with TypeScript LSP published
- Optimization techniques recorded

## Risk Mitigation

### Risk: Parser too slow for LSP

**Likelihood**: Low (String clone() issues resolved)
**Mitigation**:
- Benchmark in Phase 0 before LSP work
- If issues found, profile and optimize parser first
- Fallback: Use TypeScript LSP during parser optimization

### Risk: AST caching uses too much memory

**Likelihood**: Medium (large projects with many docs)
**Mitigation**:
- Profile memory usage early (Phase 1)
- Implement cache eviction (LRU)
- Consider compressed AST representation

### Risk: Incremental parsing doesn't help enough

**Likelihood**: Low (tree-sitter designed for this)
**Mitigation**:
- Prototype incremental parsing in Phase 1
- Benchmark incremental vs full re-parse
- Optimize based on measurements

## Next Steps

1. **Immediate** (Phase 0):
   - Set up criterion benchmarks for quarto-markdown
   - Create test corpus (small/medium/large .qmd files)
   - Run baseline benchmarks
   - Document results

2. **Early LSP** (Phase 1):
   - Add tracing instrumentation
   - Measure parse times in LSP context
   - Set up CI benchmarks

3. **Continuous** (All phases):
   - Profile each new feature
   - Compare against targets
   - Optimize as needed

4. **Final** (Phase 7):
   - Comprehensive profiling
   - Optimization sprint
   - Document performance characteristics
   - Compare with TypeScript LSP
