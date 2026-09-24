# Cache engine-discovery so we don't re-spawn per document

## Overview

`JupyterEngine::new()` (and to a lesser extent `KnitrEngine::new()`)
runs at the **top of every document render**, via
`build_html_pipeline_stages_with_options → EngineRegistry::new`. The
2026-05-21 quarto-web profile (`bd-9eltv`) measured this at \~37 % of
main-thread CPU on a 573-doc render — see
`claude-notes/research/2026-05-21-quarto-web-render-profile.md`.

Two factors compound:

1. **Subprocess spawn on every call.** `JupyterEngine::find_jupyter`
   shells out via `Command::new("sh").args(["-c", "command -v
   jupyter"])` on Unix and `Command::new("where")` on Windows. Each
   spawn is a `posix_spawn` plus blocking `read_output`.
2. **No memoization.** Whether jupyter exists on the system does not
   change inside a single `q2 render` invocation. There's no reason
   to re-discover it 573 times.

KnitrEngine already avoids the subprocess: it uses `which::which`
(in-process PATH walk). That's the pattern to converge on, plus a
process-wide cache so even the in-process walk happens once.

## Why we're confident this is a win

From the quarto-web sample (1290 samples on the main thread at 1 ms
each):

- 483 samples in `EngineRegistry::new → JupyterEngine::new`
  (combined `posix_spawn` + `poll` waiting on the child).
- Geometric scaling on tiny fixtures: ~4 ms/doc steady state, of
  which the spawn is at least ~2–3 ms.

The fix is the cheapest thing that could possibly work and yields
the largest expected win in the profile. It also unblocks more
honest profiling: as long as `posix_spawn` dominates, every other
hotspot is harder to read.

## Design

### Phase A — replace the `sh -c` spawn with `which::which`

In `crates/quarto-core/src/engine/jupyter/mod.rs`,
`find_executable(name)` currently does:

```rust
#[cfg(unix)]
let output = Command::new("sh")
    .args(["-c", &format!("command -v {}", name)])
    .output().ok()?;
// …

#[cfg(windows)]
let output = Command::new("where").arg(name).output().ok()?;
```

Replace both branches with `which::which(name).ok()`. The `which`
crate is already a workspace dependency
(`crates/quarto-core/src/engine/knitr/subprocess.rs:107`). It walks
PATH in-process, is cross-platform, and matches what
`subprocess::find_rscript` does. No subprocess spawn, no shell
parsing.

This phase alone removes the spawn. Memoization (Phase B) reduces
even the in-process walk to one call per process.

### Phase B — memoize discovery process-wide

Both `find_jupyter` and `find_rscript` should be memoized:

```rust
use std::sync::OnceLock;
use std::path::PathBuf;

fn find_jupyter() -> Option<PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(|| which::which("jupyter").ok()).clone()
}
```

`Option<PathBuf>` is cheap to `.clone()` (one heap allocation when
present, none when `None`). Sharing the `PathBuf` via `Arc` is
overkill for the cardinality of callers.

Same pattern for `find_rscript` in
`crates/quarto-core/src/engine/knitr/subprocess.rs`. Its cost is
lower (no spawn) but the principle is the same and the savings are
free.

**Correctness concerns.**

- `QUARTO_R` env var (read inside `find_rscript`) — we cache the
  *result* of looking it up. Changing `QUARTO_R` mid-process would
  not be reflected; this is acceptable because nothing inside a
  single `q2 render` invocation changes `QUARTO_R`.
- Tests that construct `JupyterEngine { jupyter_path: None }`
  directly are unaffected — they bypass `find_jupyter` entirely.
- Tests that call `JupyterEngine::new()` and expect a behavior
  conditional on whether jupyter is installed (e.g.
  `test_jupyter_engine_is_available_depends_on_jupyter`) get
  consistent results across the test run, which they already
  depend on implicitly.

### Phase C — instrumentation

Add an env-gated counter on the `find_*` functions so future
profile runs can confirm at a glance:

```rust
static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

fn find_jupyter() -> Option<PathBuf> {
    CALL_COUNT.fetch_add(1, Ordering::Relaxed);
    // …
}

// One-shot drop printout, or just expose for tests.
```

Counter is on a `static AtomicUsize` so it survives across the
engine registry tear-down. Print on process exit gated by
`QUARTO_PERF_STATS=1`, matching the convention in
`crates/pampa/src/writers/json.rs::SourceInfoSerializer`.

Output format (per the perf-profiling playbook's `perf.<gauge>`
convention):

```
perf.engine-discover jupyter_find_calls=N rscript_find_calls=N
```

The test in Phase D's checklist asserts the counter post-render is
1, not 573 (or the doc count).

### Phase D — verification

In order:

1. Add the counter (Phase C) before the fix. Run quarto-web.
   Confirm we see `jupyter_find_calls=573` (or thereabouts). This
   is the "test fails" step from TDD.
2. Apply Phase A (which::which replacement). Re-run. We expect
   `jupyter_find_calls=573` still (the count is unchanged; only the
   per-call cost dropped). Wall time should drop by \~25–40 % on
   quarto-web.
3. Apply Phase B (memoization). Re-run. We expect
   `jupyter_find_calls=1`. Wall time drops further.
4. `cargo nextest run --workspace` and `cargo xtask verify
   --skip-hub-build` must both pass.
5. Capture a samply profile of the post-fix render. Compare top
   self-time to the pre-fix table. Save the flamegraph SVG and the
   top-N table to the research note.

## Test plan (TDD)

Add a unit-or-integration test that, in a single process:

1. Calls `JupyterEngine::new()` N times (say 10) and reads a
   `find_jupyter_call_count()` instrumentation accessor.
2. Asserts the count is **1**, not 10.

The accessor is `#[cfg(test)] pub fn find_jupyter_call_count() ->
usize { CALL_COUNT.load(Ordering::Relaxed) }`. Keeping it
`#[cfg(test)]` keeps it out of the public surface while still being
visible to integration tests in the same crate (via `#[cfg(test)]`
on the module — let's verify the existing pattern in the crate
before locking that in).

Same shape for KnitrEngine.

We also need an **end-to-end check**: a small project fixture of N
docs that asserts the discover counter is 1 after a render. Either
parse `perf.engine-discover` from stderr or expose the counter via
a Rust API on `EngineRegistry`.

## Work Items

- [x] **Phase C, but first**: add counter + accessor for
      `find_jupyter` and `find_rscript`. Print `perf.engine-discover
      jupyter=N rscript=N` on process exit when
      `QUARTO_PERF_STATS=1`. Hooked into the render command via
      `print_render_diagnostics`. Verified on quarto-web:
      `perf.engine-discover jupyter=346 rscript=346` (some docs
      error before reaching the engine-execution stage, so it's
      less than the 573 doc count).
- [x] Add unit tests
      `find_jupyter_is_memoized_across_engine_construction` and
      `find_rscript_is_memoized_across_calls`. Both fail pre-fix
      with `delta = 10`; assertion is `delta <= 1` so they're
      stable regardless of cross-test cache warmth.
- [x] **Phase A**: replace `find_executable` in
      `crates/quarto-core/src/engine/jupyter/mod.rs` with
      `which::which(name).ok()`. Drop the cfg-gated `Command::new`
      branches. Counter still increments per call (cache not yet
      added); the underlying cost per call falls from a subprocess
      spawn to an in-process PATH walk.
- [x] Re-run quarto-web baseline: 3.28 s → 2.35 s (−28 %).
      Scaling fixture per-doc cost halves. Counter still equals
      `jupyter=346 rscript=346` on quarto-web.
- [x] **Phase B**: wrap `find_jupyter` and `find_rscript` in
      `OnceLock<Option<PathBuf>>` memoization. Counter increments
      moved *inside* the `get_or_init` closures so the gauge tracks
      the *expensive* cache-miss work, not every entry.
- [x] Re-run baseline: counters drop to `jupyter=1 rscript=1` on
      every render regardless of doc count. Wall-time delta vs
      Phase A is within noise (`which::which` was already cheap
      enough that memoization adds little measurable saving) but
      the counter is now the regression tripwire.
- [x] `cargo nextest run --workspace` — 9346 tests, 0 failed,
      196 skipped.
- [x] `cargo xtask verify --skip-hub-build` clean.
- [x] Capture samply profile of post-fix render
      (`claude-notes/research/2026-05-22-quarto-web-postfix-profile.json.gz`).
      Updated `2026-05-21-quarto-web-render-profile.md` with
      before/after tables and commentary. Notable: the tree-sitter
      logger did *not* become the new top hotspot — the
      `_platform_memmove`/syscall cluster did. Logger work dropped
      substantially relative to its pre-fix share; possible that
      some of the apparent pre-fix logger cost was sampling skew
      tied to the spawn-wait.

## Constraints / caveats

- **Do not change the engine selection logic** — only the discovery.
  `is_available()` and `execute()` continue to use the cached path.
- **Do not introduce `lazy_static` / `once_cell`** as new deps —
  `std::sync::OnceLock` is in std since 1.70 and is what the
  workspace already uses (see e.g. `quarto-core` source).
- **Do not parallelize Pass-2 in this issue.** That's a separate
  fan-out and would muddy the perf comparison. Sequential
  per-doc renders, one process, single memoization cache.
- **Counter stays.** Per the perf-profiling playbook ("Don't remove
  diagnostic counters after the fix lands unless they have real
  cost"), leave the counter in place as a regression tripwire.

## Follow-ups likely to fall out

These are not in scope for this plan, but the profile predicts they
become the next visible hotspots once engine-discovery is fixed:

- **Tree-sitter `set_logger` formatting overhead.** \~45 % of main-
  thread CPU pre-fix; will rise to the top once spawn cost is gone.
- **Streaming diagnostics instead of batching to end-of-render.**
  Independent UX bug; doesn't change wall time but eliminates the
  silence.

Each gets its own issue / plan if we choose to pursue them.

## Dependencies

- Discovered-from: `bd-9eltv` (the profile). The research note in
  that issue is the evidence base for this plan.
