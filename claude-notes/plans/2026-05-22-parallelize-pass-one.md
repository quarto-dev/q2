# Parallelize Pass-1 across project files

**Issue:** bd-m7x9s — discovered from bd-9eltv (quarto-web profile).

## Overview

`ProjectPipeline::pass_one` (`crates/quarto-core/src/project/orchestrator.rs:731`)
walks `self.project.files` sequentially, running the head pipeline
(`profile_with_cache → profile_single_file_live`) on each document and
collecting the resulting `DocumentProfile`s. After the engine-discovery fix
landed in bd-c5u2g, the remaining wall time on multi-document projects like
`external-sources/quarto-web` is dominated by per-doc CPU work — tree-sitter
parse, AST walk for include expansion, profile extraction, body-link
resolution. The work is **embarrassingly parallel from a
data-dependency standpoint** (see "State audit" below), so a multi-thread
fan-out should produce a near-linear speedup on multi-core machines.

This plan is the next perf win after bd-c5u2g. It is scoped strictly to
**Pass-1**. Pass-2 parallelization is a separate, larger problem (mutable
project artifacts, sink, post-render ordering) and is explicitly out of
scope here.

## Evidence base

- **`claude-notes/research/2026-05-21-quarto-web-render-profile.md`** —
  pre-fix profile (3.28 s wall, posix_spawn dominant).
- **Same note, 2026-05-22 follow-up** — post-engine-discovery profile
  (2.30 s wall, `_platform_memmove` / tree-sitter machinery dominant).
- The post-fix top symbols include `tree-sitter ts_lexer_advance`,
  `tree_sitter::Parser::parse`, and AST walk routines. These are pure
  CPU on a single thread today; parallelizing across documents lets a
  modern laptop (8–10 performance cores) work them concurrently.

Expected ceiling on quarto-web (574 files, \~2.3 s today):
- 4 cores: \~0.6–0.8 s
- 8 cores: \~0.4–0.5 s
- diminishing returns past that due to Amdahl (the cache write step,
  pre/post-render hooks, and Pass-2 are still serial).

## State audit (why Pass-1 is parallelizable)

The body of `pass_one`\'s loop is:

```rust
for doc_info in &self.project.files {
    match self.profile_with_cache(doc_info).await { ... }
}
```

What each iteration reads from `self`:
- `self.runtime: Arc<dyn SystemRuntime>` — `Send + Sync` on native
  (`crates/quarto-system-runtime/src/traits.rs:243`).
- `self.project: &ProjectContext` — immutable; all fields are
  `PathBuf`/`String`/`Vec` (data, `Send + Sync`).
- `self.format: Format` — cloned per iteration; pure data.

What each iteration **writes**:
- Nothing on `self`. `profiles` and `failures` are local accumulators.

What `profile_single_file_live` does inside:
- Builds a `RenderContext` (per-doc), a `StageContext` (per-doc),
  fresh stages, runs `run_pipeline`. All state is owned by the call.
- The Pass-1 stages (`ParseDocumentStage`, `MetadataMergeStage`,
  `IncludeExpansionStage`, `DocumentProfileStage`,
  `LinkResolutionStage`) carry no statics, no thread-locals, no
  shared mutable state — confirmed by grep.
- Tree-sitter parsers are created fresh per call
  (`crates/pampa/src/readers/qmd.rs:37,65`).

What `profile_cache::load/save` does:
- Native cache uses atomic file writes (temp file + rename),
  `crates/quarto-system-runtime/src/native.rs:493-506`. Different
  keys → different files → no contention. Same key from two
  threads is benign (last writer wins, content is identical because
  the key is content-derived).

**Conclusion:** no per-doc work mutates shared state. The only
not-fully-trivial concern is collection order — see § Determinism.

## What blocks `tokio::spawn`

Pipeline stages are `#[async_trait(?Send)]` (per `.claude/rules/wasm.md`)
because `StageContext` carries `Option<Rc<RefCell<dyn UserGrammarProvider>>>`
(`crates/quarto-core/src/stage/context.rs:198`) — needed by the
hub-client's Pass-2 path. The trait bound is global; it makes
`run_pipeline`\'s future `!Send` even though Pass-1 stages don't actually
use the `!Send` fields. We can't multi-thread `tokio::spawn` it.

This is a hard constraint we should not relax. Working around it (Option
C below) keeps the WASM single-thread contract intact.

## Parallelization options

### A. `futures::stream::buffer_unordered` (single-thread concurrency)
Compatible with `?Send`. Interleaves I/O but offers **zero CPU
parallelism**. Pass-1 is CPU-bound, so the win is near-zero. **Rejected.**

### B. Carve out a `Send`-bounded subset of `PipelineStage`
Create a `Pass1Stage: PipelineStage + Send` super-trait and a parallel
`Pass1Context` that drops the `Rc<RefCell<…>>` fields. Tokio multi-thread
can then spawn Pass-1 futures. Pros: idiomatic async, no new threading
primitive. Cons: large surface change (two parallel context types,
trait split, downstream `cfg`/dispatch churn), and the WASM build needs
yet another `cfg`-shaped subset. **Rejected** — too invasive for the win.

### C. `std::thread::scope` + `pollster::block_on` per worker  *(recommended)*
Each OS worker thread runs its own `pollster::block_on` driver. Futures
stay `!Send` because they never cross thread boundaries — only owned/`Send`
state crosses the `thread::spawn` boundary (`Arc<dyn SystemRuntime>`,
`&ProjectContext`, `Format`, `&DocumentInfo`).

```rust
let runtime = self.runtime.clone();
let project = self.project; // &
let format = self.format.clone();
let mode = self.mode.clone();
let docs = &self.project.files;

let (results, _failures): (Vec<_>, _) = std::thread::scope(|s| {
    let chunks: Vec<&[DocumentInfo]> = chunked(docs, num_threads);
    let handles: Vec<_> = chunks.into_iter().map(|chunk| {
        let runtime = runtime.clone();
        let format = format.clone();
        s.spawn(move || {
            chunk.iter().enumerate().map(|(i, doc)| {
                let result = pollster::block_on(profile_with_cache_static(
                    &runtime, project, &format, doc,
                ));
                (chunk_offset + i, doc.input.clone(), result)
            }).collect::<Vec<_>>()
        })
    }).collect();
    handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
});
```

Pros: scoped threads don't need `'static` (they can borrow `project`,
`docs`); no new dep; the change is contained to one function in
orchestrator.rs; `?Send` invariant intact.

Cons: requires extracting `profile_with_cache` into a free function
(or static-`self`-less method) so the worker can call it without
sharing `&self` across threads. Modest refactor.

### D. `rayon::par_iter` (work-stealing)  *(chosen)*
```rust
self.project.files
    .par_iter()
    .map(|doc| pollster::block_on(profile_with_cache_static(...)))
    .collect()
```

Same shape as C, with work-stealing across uneven-cost docs (large
QMDs vs trivial ones) — a real win on quarto-web where doc sizes vary
3 orders of magnitude. `IndexedParallelIterator::collect()` preserves
order automatically (no per-doc index bookkeeping).

Pros: nicer API, work-stealing, ordered output for free.
Cons: adds a workspace dep. Some churn in `Cargo.toml`.

**WASM story.** Rayon does not compile on `wasm32-unknown-unknown`
(no OS threads). The dep is `cfg`-gated to native:

```toml
# crates/quarto-core/Cargo.toml
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
rayon = "1"
```

`pass_one` then has two implementations:

```rust
#[cfg(not(target_arch = "wasm32"))]
async fn pass_one(&self) -> (...) { /* rayon par_iter */ }

#[cfg(target_arch = "wasm32")]
async fn pass_one(&self) -> (...) { /* existing sequential for-loop */ }
```

No rayon symbol ever reaches the WASM build. The browser keeps the
current single-threaded behavior (it has no choice — wasm32 is
single-threaded). WASM working version is preserved verbatim.

## Rayon pitfalls (and how we'll address each)

1. **Global thread pool is process-wide.** `par_iter` uses a static
   pool. Today nothing else in the workspace uses rayon (verified via
   grep), so there's no surprise sharing. If we ever want isolation we
   can build a private `ThreadPoolBuilder::new().num_threads(N).build()`
   pool — slightly more code, no behavioral risk. Sticking with the
   global pool for now.
2. **Workers aren't tokio tasks.** Each rayon worker runs
   `pollster::block_on(profile_with_cache(...))`. Any async code
   transitively reached must not require a tokio runtime context
   (e.g. `tokio::time::sleep`, `tokio::spawn`, `tokio::fs`). Native
   `SystemRuntime::cache_get/set/file_read/canonicalize` use plain
   `std::fs` (verified in `crates/quarto-system-runtime/src/native.rs`),
   but the broader Pass-1 stage graph must be audited as Phase 0
   sub-step "pollster compatibility audit" before Phase 2 swap-in.
3. **Panic propagation.** A panic in one rayon worker propagates out
   of `.collect()` and would abort the whole render. The current
   sequential code only converts `Result::Err` → `FileFailure`; a
   panicking stage already aborts today. To match the per-file
   isolation we already have for `Err`, wrap each worker body in
   `std::panic::catch_unwind` and convert a caught panic to a
   `FileFailure` with a "internal error during Pass-1" message.
4. **Stack size.** Rayon's default worker stack is \~2 MB. Tree-sitter
   on pathological inputs can recurse deeply. The sequential path uses
   the main-thread stack today, which is typically larger. If we see
   stack overflows in CI, `ThreadPoolBuilder::stack_size` is the knob.
   No pre-emptive change — flag if it bites.
5. **Order-preserving collect.** `IndexedParallelIterator::collect()`
   preserves input order; non-indexed adaptors don't. We stay on the
   indexed variants and assert order in
   `pass_one_preserves_input_order`.
6. **Cooperative cancellation.** Rayon doesn't preempt in-flight
   tasks; Ctrl+C lets running workers finish, then drops pending. The
   same effective UX as today (sequential code can't abort the running
   doc either) — no regression. Documented for ops.
7. **`available_parallelism()` can fail** (rare, e.g. cgroups with no
   info). Default to 4 on error.
8. **Debug-build overhead.** Per-`par_iter` dispatch is \~10–100 µs in
   debug. Invisible for hundreds of docs; flag for tiny unit tests not
   to over-interpret debug-mode wall times.
9. **Test isolation across binaries.** nextest already runs each
   `[[test]]` binary in its own process. Within a binary, tests share
   the global rayon pool. If we see flakiness we'll scope a per-test
   pool with `ThreadPoolBuilder`.

## Design (assuming Option D — rayon)

### Concurrency primitive
- `crates/quarto-core/Cargo.toml`: add `rayon = "1"` (native only, behind
  `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` since rayon
  doesn't compile on `wasm32-unknown-unknown`).
- On WASM, Pass-1 stays the existing sequential `for` loop (single-thread
  environment; no parallelism to gain).

### Refactor `profile_with_cache` to a thread-safe form
Today: `async fn profile_with_cache(&self, doc_info: &DocumentInfo)`.
Reads `self.runtime`, `self.project`, `self.format`, `self.project.dir`,
`self.project.is_single_file`. All borrow-able / clone-able.

Refactor into a free function or static method that takes them as
arguments. The existing async body (which uses `pollster::block_on`-able
futures from `run_pipeline`) stays unchanged.

### Worker count
- Default: `std::thread::available_parallelism().unwrap_or(NonZeroUsize::new(4).unwrap()).get()`.
- Cap (e.g. at 16) to avoid pathological behavior on big servers.
- Env override: `QUARTO_JOBS=N` (mirrors `make -j` / `cargo --jobs`).
  Generic name chosen deliberately so Pass-2 parallelism can reuse
  the same knob when it lands.
- `N=1` short-circuits to the current sequential path (preserves the
  exact pre-change behavior for debugging / reproducibility).

### Determinism
Pass-1 outputs (the `Vec<DocumentProfile>` passed into `ProjectIndex`) must
be in stable order to avoid spurious churn in dependency-graph output,
diagnostics ordering, and downstream pass-2 dispatch. Two options:

1. `IndexedParallelIterator::collect()` on `par_iter()` preserves
   input-order. With rayon this is free.
2. Manual: collect `(index, result)` pairs, sort by `index`.

We use option 1 — rayon gives this for free with `par_iter()`.

The Pass-1-failures vec is similarly preserved in input-order.

### Cancellation
`StageContext` carries a `Cancellation` token. We thread a single
cloneable token into each worker. (Today: each per-doc `StageContext`
gets its own token from `StageContext::new`, so this is already per-doc.
We may need to plumb a parent token down if the orchestrator wants to
abort all in-flight workers on Ctrl+C — file as a sub-issue if it's not
already wired.)

### Instrumentation
Per the perf-profiling playbook ("don't remove diagnostic counters"):

- New gauge `perf.pass1`, emitted from `print_render_diagnostics` when
  `QUARTO_PERF_STATS=1`:
  ```
  perf.pass1 docs=574 threads_used=8 wall_ms=412
  ```
- `threads_used` is the count of *distinct thread IDs* that executed at
  least one `profile_with_cache` during this run. This is the regression
  tripwire — if a future refactor breaks the rayon dispatch, this drops
  to 1.
- Implementation: a small `Mutex<HashSet<ThreadId>>` (only locked once
  per doc; insignificant overhead on 574 docs).

### Tests (TDD shape)
1. **Failing test first**: a unit test
   `pass_one_uses_multiple_threads_when_parallelism_available` that:
   - Builds a synthetic `ProjectContext` with N docs (>= num_cpus).
   - Runs `pass_one`.
   - Asserts the recorded `threads_used` is `>= 2` on a machine with
     `available_parallelism() >= 2`. (Gated; trivial / skipped on
     uniprocessor.)
   - Pre-fix: `threads_used == 1`, fails.
   - Post-fix: passes.
2. **Determinism test**: same N-doc project, run pass_one twice, assert
   the returned `Vec<DocumentProfile>` is in `project.files` order both
   times.
3. **Cache-correctness test**: run pass_one twice on the same project;
   second run should produce identical profiles, no cache file
   corruption (assert all cache files parse).
4. **No-regression test**: existing pass_one snapshot/integration tests
   in `crates/quarto-core/src/project/tests/*` must still pass.
5. **End-to-end**: `q2 render external-sources/quarto-web` — render
   produces the same output set as today (same file count, same hashes
   for unchanged inputs). `cargo xtask verify --skip-hub-build` clean.
6. **Re-profile with samply**: capture post-parallelization profile.
   Top-N self-time table goes back into
   `claude-notes/research/2026-05-21-quarto-web-render-profile.md`
   under a "2026-05-22 follow-up #2 (Pass-1 parallel)" section. New top
   hotspots → file as discovered follow-ups under bd-m7x9s.

## Risks & caveats

- **Tempdir storm.** Each `StageContext::new` calls
  `runtime.temp_dir("quarto-pipeline")` (mkdtemp-style on native). N
  workers in flight ⇒ N concurrent mkdtemps. `tempfile`\'s `TempDir::new`
  is thread-safe and uses unique paths, so this is fine — but profile to
  confirm the inode-creation cost doesn't dominate.
- **Tree-sitter logger memory.** Each parser allocates its own
  `TreeSitterLogObserver`. Multiplied by `num_threads`, this is a small
  memory bump per worker — irrelevant on quarto-web sizes.
- **Cache write contention** (already audited above): atomic rename per
  key, no shared state, safe.
- **Diagnostic emission ordering**: pass1_failures stays in input order
  (rayon's `IndexedParallelIterator::collect`). No user-visible change.
- **Single-doc / single-thread short-circuit**: when `files.len() == 1`
  or `QUARTO_PASS1_THREADS == 1`, fall back to the existing sequential
  path — keeps the trivial case zero-overhead.
- **WASM target**: unchanged; the sequential path remains. The rayon
  dep is `cfg(not(target_arch = "wasm32"))`-gated.
- **Test isolation**: nextest already runs each `[[test]]` binary in its
  own process. Inside a binary, tests may now contend on the global rayon
  pool. If we see flakiness, scope a per-test pool with
  `ThreadPoolBuilder`.

## Out of scope

- **Pass-2 parallelization**. The orchestrator's `project_artifacts`
  store, the per-doc resource_copies merge, sitemap accumulation, and
  the sink/output-file emit are not yet thread-safe in the same way
  Pass-1 is. A separate plan / beads issue.
- **Stage-level concurrency** (running e.g. `IncludeExpansionStage` and
  `LinkResolutionStage` in parallel for one doc). Negligible savings;
  large refactor; ignore.
- **Async multi-thread Pass-1** (Option B). Would require rewriting the
  `?Send` invariant; we've explicitly chosen not to.

## Work items

### Phase 0: Failing test + threads_used gauge + pollster audit
- [x] **Pollster-compat audit.** Grep the Pass-1 transitive call graph
      for tokio runtime requirements (`tokio::spawn`, `tokio::time::*`,
      `tokio::task::*`, `tokio::fs::*`). All clear — see "Phase 0
      results: Pollster-compat audit" below.
- [x] Add a `Mutex<HashSet<ThreadId>>` accumulator + atomic
      `PASS1_DOCS` and `PASS1_WALL_NANOS` to track which OS threads
      executed `profile_with_cache` and the cumulative wall time.
      Calling `pass1_threads_record()` is now the first thing
      `profile_with_cache` does.
- [x] Expose `pass1_threads_used()`, `pass1_threads_snapshot()`,
      `pass1_docs_seen()`, `pass1_wall_nanos()` accessors.
- [x] Hook `print_pass1_stats_if_enabled()` into
      `print_render_diagnostics` — emits
      `perf.pass1 docs=N threads_used=K wall_ms=W` under
      `QUARTO_PERF_STATS=1`. (Companion to `perf.engine-discover`.)
- [x] Add integration test
      `pass_one_uses_multiple_threads_when_parallelism_available` in
      `crates/quarto-core/tests/project_pipeline.rs`. Verified it
      **fails** pre-Phase-2 with
      `expected ≥ 2 OS threads (available_parallelism = 18), got 1
      new thread ids`.
- [x] Add integration test `pass_one_preserves_input_order` — passes
      pre-Phase-2; guards Phase 2 against unordered rayon `collect`.
- [x] Test hook: `ProjectPipeline::__pass_one_for_test_only` —
      doc-hidden public accessor so integration tests can read
      Pass-1's raw `(profiles, failures)` output. Required because
      `summary.outputs` ordering is a Pass-2 invariant, not a Pass-1
      one (Pass-2 re-iterates `project.files`).

### Phase 1: Refactor `profile_with_cache` to a thread-safe form
- [x] Extract the body of `profile_with_cache` into the free function
      `pass1_profile_with_cache(runtime, project, format, doc_info)`.
- [x] Same for `profile_single_file_live` →
      `pass1_profile_single_file_live(runtime, project, format,
      doc_info, source_bytes)`.
- [x] Extract three private helpers (`project_relative_source_path`,
      `layered_metadata_raw_bytes`, `read_quarto_yml_bytes`) into free
      functions with a `pass1_` prefix. The `&self` wrapper for
      `project_relative_source_path` stays because
      `compute_augmented_render_set` (Pass-2 setup) uses it from an
      `&self` context.
- [x] `pass_one` still calls them sequentially via the free function
      — no behavioral change.
- [x] All 2094 non-tripwire `quarto-core` tests pass.
- [x] The Phase-0 `pass_one_preserves_input_order` regression guard
      passes; the Phase-2 tripwire test still fails as designed.

### Phase 2: Parallelize via rayon (Option D, approved)
- [x] Add `rayon = "1"` to `quarto-core` under
      `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`.
- [x] Replace the `for` loop in `pass_one` with a `pass_one_dispatch`
      helper that branches between native rayon and a sequential
      fallback. Native path uses `par_iter().map().collect_into_vec()`
      via `IndexedParallelIterator` for ordered output.
- [x] Worker count: `pass1_worker_count()` reads `QUARTO_JOBS`
      env var (default = `available_parallelism()`, capped at 16,
      fallback 4 on error).
- [x] Short-circuit to sequential when `files.len() <= 1` or
      `QUARTO_JOBS == 1` (single-doc renders pay zero rayon overhead).
- [x] Wrap each worker body in
      `std::panic::catch_unwind(AssertUnwindSafe(...))` and convert
      a caught panic to a `FileFailure` with an
      "internal error during Pass-1 (panic): ..." message.
- [x] WASM target: `pass_one_dispatch` cfg-branches to the sequential
      path; rayon symbol never reaches `wasm32-unknown-unknown`.
- [x] Failing test from Phase 0 (`pass_one_uses_multiple_threads_when_parallelism_available`)
      now **passes**. All 2095 `quarto-core` tests pass.

### Phase 3: Verification
- [x] `cargo nextest run --workspace` — 9348 tests pass, 196 skipped,
      0 failures.
- [x] `cargo xtask verify --skip-hub-build` — clean.
- [x] Run scaling fixtures (`/tmp/q2-perf-scale/300`) — confirms
      `QUARTO_JOBS` properly constrains the pool (threads_used=N
      tracks the env var; wall time drops 182 → 78 ms going from 1
      → 18 workers on the 300-doc fixture).
- [x] Render `external-sources/quarto-web` — measured. Cold cache:
      Pass-1 2230 → 1280 ms (1.74×), total 3.73 → 2.81 s (1.33×).
      Warm cache: Pass-1 800 → 520 ms (1.54×), total 2.29 → 2.03 s
      (1.13×). Lower than the optimistic 8× target — Pass-1 is now
      FS-bound (cache atomic-rename per doc, source/\_metadata.yml
      reads), and the still-sequential Pass-2 caps overall speedup
      (Amdahl).
- [x] samply profile captured at
      `claude-notes/research/2026-05-22-quarto-web-parallel-pass1-profile.json.gz`.
      74% of samples in `__ulock_wait2`/`__ulock_wake` (rayon worker
      idle time during the still-serial Pass-2 phase).
- [x] Updated `claude-notes/research/2026-05-21-quarto-web-render-profile.md`
      with 2026-05-22 follow-up #2 section including before/after
      wall-time tables, scaling table across `QUARTO_JOBS`, and the
      post-parallel symbolicated profile.
- [x] Bug found and fixed mid-Phase-3: original code used the global
      rayon pool, so `QUARTO_JOBS` only affected the sequential
      short-circuit; non-1 values silently used all cores. Fix:
      build a local `ThreadPoolBuilder::new().num_threads(workers)`
      pool and `.install()` the `par_iter` inside it.
- [ ] File new follow-ups as `br create … --deps
      discovered-from:bd-m7x9s` — pending user direction on which
      to prioritize. Candidates documented in the research note's
      "Suggested follow-ups" section (parallelize Pass-2, batch
      cache writes, lazy temp_dir, stream pass1_failures).

### Phase 4: Commit + push
- [ ] Commit code + plan + research note update.
- [ ] Wait for explicit user approval before pushing.

## Decisions (locked in 2026-05-22)

1. **Primitive: rayon (D).** Work-stealing wins on uneven workloads;
   ordered collect for free; one new workspace dep, cfg-gated to native.
2. **Worker count default = `available_parallelism()` capped at 16,
   fallback 4 on error.** Env override: `QUARTO_JOBS` (generic name,
   reusable for Pass-2 in the future).
3. **`threads_used` accumulator: `Mutex<HashSet<ThreadId>>`.** Simpler;
   \~574 lock acquisitions per render is invisible.
4. **Panic handling:** wrap each worker in `catch_unwind` to convert
   panics to `FileFailure` — matches the per-file `Result::Err`
   isolation we already have.

## Dependencies

- `discovered-from`: bd-9eltv (the original quarto-web profile).
- Builds on bd-c5u2g (per-process engine-discovery cache). Without that
  fix the sequential Pass-1 spawns `posix_spawn` 574 times — the
  parallel version would still do the same and contention on the OS
  spawn path would mask the win.

## Phase 0 results

### Pollster-compat audit (2026-05-22)

Surveyed every transitive call site reachable from Pass-1 for tokio
runtime requirements (`tokio::spawn`, `tokio::time::*`,
`tokio::task::*`, `tokio::fs::*`):

- **Pass-1 stages** (`parse_document`, `metadata_merge`,
  `include_expansion`, `document_profile`, `link_resolution`): zero
  matches. All sync internally, exposed as `async fn` only for
  trait-shape consistency.
- **`pipeline::run_pipeline`**: zero matches.
- **`quarto-system-runtime` native impl**: zero matches. Uses plain
  `std::fs` for cache and file I/O.
- **`pampa` library functions called by Pass-1**: zero matches.
  Tokio-bearing files in pampa are tests (`#[tokio::test]`) and the
  standalone `main.rs` binary — not on the library path.
- **`tokio::task::block_in_place`** appears only in
  `user_filters.rs` and `jupyter::transform.rs` — both Pass-2.
- **`tokio::time::timeout` / `tokio::fs`** appear only in the Jupyter
  engine — Pass-2.

Conclusion: `pollster::block_on` on a rayon worker is safe for the
Pass-1 transitive graph. No workaround required for Phase 2.
