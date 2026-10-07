# Parallelize Pass-2 render loop (bd-3gj56)

## Overview

Pass 2 (qmd→HTML render-to-file) is a serial `for` loop at
`crates/quarto-core/src/project/orchestrator.rs:1013` — \~98% of
full-project render wall time. Pass 1 is already rayon-parallel
(bd-m7x9s, plan `2026-05-22-parallelize-pass-one.md`). This plan
parallelizes Pass 2 by **mirroring the established Pass-1 pattern**.

The 16-thread parse experiment from bd-2ercw measured **7.1× scaling
with zero lock contention** on this exact corpus — the upside is real
and the locale-lock trap is gone (PR #247). See
`2026-06-01-render-perf-profiling.md`.

## Why this is low-risk (findings from code audit)

1. **No cross-document data dependency.** In `render_to_file.rs`, the
   `project_artifacts: Option<&mut ArtifactStore>` parameter is used in
   exactly one place (`dest.merge_into_project(drained)`, line 342) —
   **write-only**. No render reads the accumulator. The disk `sink` is
   created per-document *inside* `render_document_to_file`. So each
   render is fully independent except for one write-merge of its own
   artifacts.

2. **The artifact merge is order-independent.**
   `ArtifactStore::merge_into_project` (artifact.rs:344) dedups
   byte-equal entries and errors only on true content conflicts for the
   same key. Merging per-worker stores in any order yields an identical
   result ⇒ **determinism is preserved for free.**

3. **`SystemRuntime: Send + Sync`** (traits.rs:243) — `Arc<dyn
   SystemRuntime>` is shareable across rayon workers directly; no
   per-worker runtime needed. Native async methods produce `Send`
   futures.

4. **`render_document_to_file` is synchronous** — a rayon worker calls
   it directly (no `?Send` future crossing threads, no `pollster` even
   needed for the call itself), sidestepping the async-trait `?Send`
   tension entirely on the parallel path.

5. **`RenderToFileRenderer` holds only `&options`** (immutable) — no
   mutable per-render state to contend on. The `&mut self` on the
   `Pass2Renderer::render` trait method is unused by the native impl.

6. **rayon is already a native dep** of quarto-core, and
   `pass1_worker_count()` (the `QUARTO_JOBS` knob, capped at 16) is
   directly reusable.

## Design — mirror `pass_one_dispatch_parallel`

Add a native `pass_two_dispatch` with sequential/parallel split,
modeled exactly on `pass_one_dispatch` (orchestrator.rs:1111):

- **Worker count**: reuse `pass1_worker_count()` (rename to
  `worker_count()` — shared by both passes; `QUARTO_JOBS` honored).
- **Small-N / WASM / `QUARTO_JOBS=1`**: keep today's serial loop
  (`pass_two`), which already drives `Pass2Renderer::render` (preserves
  WASM single-threaded path unchanged).
- **Parallel path** (`pass_two_dispatch_parallel`):
  - Local `rayon::ThreadPoolBuilder` sized to `workers`, thread name
    `quarto-pass2-{i}`. Degrade to serial on pool-build failure.
  - `par_iter()` over the filtered file list, `collect_into_vec` to
    **preserve input order** for the `outputs` vec (feeds sitemap).
  - Each worker:
    - honors `skip` / `render_set` (Mode B) filtering → `Skipped`;
    - `fail_fast` `AtomicBool` short-circuit (rayon doesn't cancel
      in-flight — accepted, same as Pass 1);
    - `catch_unwind` → per-file `FileFailure` isolation;
    - calls `render_document_to_file(... Some(&mut local_store))` with
      a **per-worker `ArtifactStore`**;
    - returns `(index, Result<RenderToFileResult>, local_store)`.
  - After the parallel section, in **input order**, the orchestrator
    merges each worker's `local_store` into `self.project_artifacts`
    (commutative; deterministic) and collects outputs/failures.

### Trait shape (refined after audit — important)

**Constraint discovered:** the WASM hub-client drives `run()`/`pass_two`
too (`with_renderer` + `RenderToHtmlRenderer`, lib.rs:1538). So we
**cannot** add `R: Sync` / `R::Output: Send` bounds to the shared
generic `pass_two`/`run` — `RenderToHtmlRenderer`/`WasmPassTwoOutput`
are not `Send`/`Sync` (wasm-bindgen types), and the bounds would break
the WASM build.

**Resolution — `render_batch` on the trait, threading bounds confined
to the native impl:**

```rust
// Pass2Renderer trait — default SERIAL impl (WASM + tests use this,
// no Send/Sync needed anywhere):
async fn render_batch(
    &mut self, docs: &[&DocumentInfo], format_str: &str,
    project: &ProjectContext, index: Arc<ProjectIndex>,
    runtime: Arc<dyn SystemRuntime>, project_artifacts: &mut ArtifactStore,
    workers: usize, fail_fast: bool,
) -> (Vec<Self::Output>, Vec<FileFailure>) { /* serial loop over render() */ }
```

`RenderToFileRenderer` (native) **overrides** `render_batch` with the
rayon fan-out. Inside that override `Self::Output = RenderToFileResult`
is concrete and `Send`, so `collect_into_vec` + per-worker stores
compile with no trait-level bounds. The override extracts the `Send +
Sync` references it needs (`options`, `&project`, `format_str`, Arc'd
index/runtime) before the parallel section. The default serial impl
preserves WASM and any serial-only renderer untouched.

`pass_two` does the `skip`/`render_set` filtering (builds `docs:
Vec<&DocumentInfo>`), computes `workers`, then calls `render_batch`
once, then nothing else changes (post_render etc.).

### Testability

Worker count = `self.jobs.unwrap_or_else(worker_count)` where
`worker_count()` reads `QUARTO_JOBS` (cap 16) like Pass 1. Add a
`ProjectPipeline::with_jobs(n)` builder so tests pin parallelism
(jobs=1 vs jobs=16) **without mutating process-global env** (env
mutation is `unsafe` in edition 2024 and racy). Production CLI behavior
is unchanged (no override ⇒ `QUARTO_JOBS`/auto).

### Decision (resolved by user)

Per-worker `ArtifactStore` + merge-at-end (no lock during render,
matches the "orchestrator is the only mutator" invariant). Chosen over
`Mutex<&mut ArtifactStore>`.

## TDD test plan (write first)

1. **`pass_two_preserves_input_order`** — render a multi-file project,
   assert `outputs` order matches `project.files` order regardless of
   thread count (run with `QUARTO_JOBS` high). Mirror
   `pass_one_preserves_input_order`.
2. **`pass_two_parallel_matches_serial`** — render the same fixture
   project with `QUARTO_JOBS=1` and with `QUARTO_JOBS=16`; assert
   byte-identical on-disk outputs *and* identical accumulated
   `project_artifacts` (site_libs). This is the determinism guard.
3. **`pass_two_fail_fast_stops`** — project with a doc that errors;
   assert fail-fast reports the failure and (best-effort) does not
   render all subsequent docs. Tolerate in-flight completions (same
   contract as Pass 1).
4. **`pass_two_panic_isolated`** — a doc that panics maps to a
   `FileFailure`, render of others still succeeds.
5. End-to-end: `q2 render claude-notes/qmd-plans/` byte-compare a
   serial (`QUARTO_JOBS=1`) vs parallel `_site/` tree → identical.

## Verification & expected impact

- `cargo nextest run --workspace`, then `cargo xtask verify` (pampa /
  quarto-core feed the WASM leg).
- Expected: end-to-end `q2 render` of qmd-plans drops from \~3.5 s
  (serial, post-bd-2ercw) toward \~0.7–1.0 s on a 16-core box if Pass 2
  scales like the parse experiment (7.1×) — Pass 2 is \~98% of wall, so
  the project-level speedup tracks the per-doc render speedup minus the
  serial post-render tail (sitemap/favicon/site_libs flush).
- Measure with the same median-of-5 methodology, `QUARTO_JOBS` ∈
  \{1, 2, 4, 8, 16\}, to confirm the scaling curve and find the knee.

## Work items

- [x] Generalize `pass1_worker_count` → `worker_count` (shared; +wasm stub)
- [x] Add `Pass2Renderer::render_batch` with default **serial** impl;
  rewrite `pass_two` to filter → `render_batch` (behavior identical)
- [x] Add `ProjectPipeline::with_jobs(n)` + `jobs` field; wire worker count
- [x] TDD tests (green on serial baseline AND parallel path):
  - [x] `pass_two_preserves_input_order`
  - [x] `pass_two_parallel_matches_serial` (determinism: jobs=1 vs 16, byte-exact `_site/`)
  - [x] `pass_two_parallel_renders_all_pages` (sanity)
  - [ ] fail-fast / panic: deferred — a deterministic *Pass-2* failure is
        hard to construct (parse errors fail in Pass 1). The `AtomicBool`
        fail-fast + `catch_unwind` logic mirrors Pass 1's tested pattern.
- [x] Override `render_batch` in `RenderToFileRenderer` — rayon fan-out,
  **per-document** `ArtifactStore` (refined from per-worker for exact
  serial conflict-attribution parity), `collect_into_vec` ordering,
  `AtomicBool` fail-fast, `catch_unwind` isolation, serial degrade
- [x] Sanity: deliberate output-reverse break → `pass_two_preserves_input_order`
  caught it (FAIL); reverted, green again
- [x] `perf.pass2` gauge (docs / threads_used / wall_ms), mirror `perf.pass1`,
  wired into `q2 render` alongside `perf.pass1`
- [x] Workspace tests + `cargo xtask verify` — all green (incl. WASM leg)
- [x] End-to-end byte-equivalence (serial vs parallel) + scaling table

## Results (qmd-plans, 565 files, release)

End-to-end `q2 render` scaling sweep (median-of-5 total wall; Pass-2
wall from the `perf.pass2` gauge):

| jobs | total wall | Pass-2 wall | total speedup |
|---|---|---|---|
| 1  | 3650 ms | 3551 ms | 1.0× |
| 2  | 2010 ms | 1921 ms | 1.82× |
| 4  | 1120 ms | 1120 ms | 3.26× |
| 8  |  750 ms |  696 ms | 4.87× |
| 16 |  500 ms |  446 ms | **7.3×** |

Pass-2 itself: **3551 → 446 ms (≈8× on 16 threads)**. The total's
diminishing return at 16 is the serial tail (Pass-1 \~20 ms + post_render
sitemap/site_libs flush + resource copy) plus this box being 8-core /
16-thread.

**Correctness:** `diff -r` of the serial (`QUARTO_JOBS=1`) vs parallel
(`QUARTO_JOBS=16`) `_site/` trees on the full 565-page corpus →
**byte-for-byte identical** (571 files incl. `site_libs/`).

### Progress log
- 2026-06-01: design audit complete, plan refined (render_batch trait
  method + with_jobs), per-worker-store decision confirmed by user.
- 2026-06-01: core implemented. Refined per-worker → **per-document**
  artifact stores for exact serial-parity on the (pathological)
  cross-doc artifact-key-conflict case. Trait `render_batch` default is
  serial (WASM/tests untouched); native override does the rayon fan-out
  with all `Send`/`Sync` bounds confined to the concrete
  `RenderToFileResult`. 3 correctness tests green; deliberate-break
  sanity check confirmed the order guard bites. Next: regression suite,
  perf.pass2 gauge, end-to-end scaling measurement.
