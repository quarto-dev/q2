# Profiling `q2 render external-sources/quarto-web`

**Date:** 2026-05-21
**Issue:** bd-9eltv
**Plan:** [`claude-notes/plans/2026-05-21-q2-render-website-profile.md`](../plans/2026-05-21-q2-render-website-profile.md)

## TL;DR

Wall time is 3.28s on a 573-doc website that ultimately errors out
before producing any HTML. Two hotspots account for \~80 % of CPU on
the main thread:

1. **Per-document subprocess spawn to find `jupyter`** — \~37 % of
   main-thread CPU. `EngineRegistry::new()` is called inside
   `build_html_pipeline_stages_with_options`, *which runs once per
   document*. Each construction calls `JupyterEngine::new()`, which
   runs `sh -c "command -v jupyter"` via `std::process::Command`.
   573 docs × one spawn ≈ 573 subprocess invocations.
2. **Tree-sitter logger callback formatting every lexer / parser
   step** — \~45 % of CPU lives in the `snprintf` → `__vfprintf` →
   `_platform_memmove` chain. `crates/pampa/src/readers/qmd.rs`
   attaches a `set_logger` callback unconditionally; tree-sitter
   formats the log message via `snprintf` for every state
   transition before invoking the callback, regardless of whether
   the callback filters the message out.

Both are independent: a fix to either would land independently of
the other. Together they account for most of the wall-time silence
before any output is printed.

A secondary observation: **all output is batched to the end of the
render.** `print_render_diagnostics` in `crates/quarto/src/commands/
render.rs:704` only runs *after* `pipeline.run().await` returns, so
the user sees 3.28 s of silence even though the first parse failure
is determined almost immediately. This is a UX bug independent of
the perf hotspots — fixing only it would already make Q2 *feel*
faster for projects that error out fast.

## Method

Native-proxy-first per `claude-notes/instructions/performance-profiling.md`:

1. Built `target/release/q2` and `target/release-perf/q2`
   (release with debug symbols for sampling).
2. Baseline wall + RSS via `/usr/bin/time -l`.
3. Time-to-first-output via Perl `Time::HiRes`.
4. CPU sample via macOS `/usr/bin/sample` at 1 ms intervals.
5. Self-time per symbol via parsing the call graph in Python
   (script inline below).
6. Geometric-scale fixture series (1, 10, 50, 100, 300 docs) to
   confirm linearity.

samply / cargo-flamegraph are not installed on this machine; the
macOS `sample` output served the same purpose for this initial
pass.

## Numbers

### Baseline (quarto-web)

```
$ /usr/bin/time -l target/release/q2 render external-sources/quarto-web
...
exit=1
        3.32 real         2.28 user         0.71 sys
            94666752  maximum resident set size  (≈ 90 MiB)
              144950  page reclaims
         51385274460  instructions retired
         10544767833  cycles elapsed
```

- Wall: **3.28 s** (separate Perl run confirmed)
- Peak RSS: **90 MiB**
- Exit: 1 (every doc fails on dark-mode theme — bd-0pic6)

### Time-to-first-output

```
FIRST_OUTPUT_AT: 3.277s (warning: profile-pass skipped …/bug-reports.qmd)
TOTAL:           3.280s
```

The first byte of output appears 3.28 s into the run. Cause:
`print_render_diagnostics` runs *after* `pipeline.run().await`
returns (`crates/quarto/src/commands/render.rs:704`). The render
fails per-doc, the failures accumulate into `summary.pass1_failures`,
and the warnings are dumped at the end.

### Geometric scaling on clean fixtures

Tiny generated docs, no theme failures (no error path):

| N    | wall (s) | per-doc (ms) |
|------|---------:|-------------:|
| 1    |    0.024 |        24    |
| 10   |    0.062 |         6.2  |
| 50   |    0.206 |         4.1  |
| 100  |    0.401 |         4.0  |
| 300  |    1.163 |         3.9  |
| 573 (quarto-web) | 3.28 |   5.7  |

Linear in N, with a \~20 ms baseline. quarto-web's higher per-doc
(5.7 ms) is consistent with larger documents = more parse work.

### Self-time top symbols (1290 samples, 1 ms each)

From the macOS `sample` call graph, computing
`self = parent_count − sum(immediate_children)` per node and
aggregating by symbol:

| self | symbol                                                    |
|-----:|-----------------------------------------------------------|
| 419  | `poll`  (syscall; mostly under subprocess `read_output`)   |
| 209  | `__vfprintf`                                              |
| 147  | `_platform_memmove`                                       |
| 106  | `__sfvwrite`                                              |
| 76   | `_xzm_free`                                               |
| 66   | `ts_parser__advance`                                       |
| 59   | `__posix_spawn`                                            |
| 53   | `core::str::converts::from_utf8`                           |
| 51   | `__open`                                                   |
| 47   | `_vsnprintf`                                               |
| 46   | `snprintf`                                                 |
| 44   | `drop_in_place<ConfigValueKind>`                           |
| 36   | `stat`                                                     |
| 35   | `tree_sitter::Parser::set_logger::log`                     |
| 34   | `pampa::filters::topdown_traverse_blocks::walk_vec`        |
| 29   | `SplitWhitespace::next`                                    |
| 28   | `ts_subtree_summarize_children`                            |
| 28   | `ConfigValue::clone`                                       |
| 27   | `ts_tree_cursor_child_iterator_next`                       |
| 26   | `ts_parser_parse`                                          |
| 26   | `__open_nocancel`                                          |
| 25   | `pampa::filters::topdown_traverse_block`                   |
| 25   | `mkdir`                                                    |
| 24   | `drop_in_place<ConfigMapEntry>`                            |
| 23   | `__ultoa`                                                  |
| 22   | `ts_lexer__advance`                                        |
| 21   | `ts_language_next_state`                                   |

### Confirmed call paths

**Per-doc subprocess spawn (482 of 1290 samples ≈ 37 %).**

```
execute_project → ProjectPipeline::run → Pass2Renderer::render
  → render_document_to_file → render_qmd_to_html
    → build_html_pipeline_stages_with_options    [489 samples]
      → EngineRegistry::new                       [483]
        → JupyterEngine::new                      [483]
          ├── std::process::Command::output       [419 → poll]
          └── std::process::Command::output       [63 → posix_spawn]
```

Source:
- `crates/quarto-core/src/engine/registry.rs:64` registers
  `JupyterEngine::new()` (and `KnitrEngine::new()`).
- `crates/quarto-core/src/engine/jupyter/mod.rs:84-92`
  unconditionally calls `find_jupyter()` →
  `Command::new("sh").args(["-c", "command -v jupyter"])`.
- `crates/quarto-core/src/pipeline.rs:242-249`
  `build_html_pipeline_stages_with_options` calls
  `EngineExecutionStage::new()`, which calls
  `EngineRegistry::new()` whenever no `engine_registry` override
  is supplied. The orchestrator's per-doc path does not pass an
  override.

KnitrEngine likely does the same `command -v R` shell-out but it
didn't dominate this profile — possibly because `R` is found
faster on this machine, or its spawn timing landed outside the
sample window.

**Tree-sitter logger formatting (\~580 of 1290 samples ≈ 45 %).**

```
qmd::read → MarkdownParser::parse → ts_parser_parse_with_options
  → ts_parser_parse → ts_parser__advance
    → ts_lex / parse_pipe_table / parse_plus / external_scanner
      → ts_lexer__advance
        → snprintf → _vsnprintf → __vfprintf
        → tree_sitter::Parser::set_logger::log (Rust callback)
```

Source: `crates/pampa/src/readers/qmd.rs:69-73` and `:106-110`
unconditionally attach `parser.parser.set_logger(...)`. Even when
the callback no-ops for non-`Parse` log types, tree-sitter still
formats the log message via `snprintf` before calling the
callback. The cost is paid per lexer state transition.

The codebase uses the logger for error-recovery: when a parse
produces an error tree, the log of state transitions feeds
quarto-parse-errors to generate a diagnostic. On the success path
(the common case), the logger output is discarded.

## Independent UX observation

The 3.28 s of silence before output is *also* a property of how
diagnostics are flushed. `crates/quarto/src/commands/render.rs:704`
prints `pass1_failures` after `pipeline.run().await` returns, not
as each failure happens. Fixing only the silence (streaming
diagnostics) would not reduce wall time but would make the render
*feel* responsive, which directly answers the user's concern
("the relatively long time between issuing the command and seeing
the output").

## Suggested follow-ups (file as separate issues)

These are *candidate* follow-up issues — none should be folded into
bd-9eltv (per the plan, this issue produces analysis only). Each
deserves its own beads issue, on its own branch, with its own
verification.

1. **Hoist engine-registry construction to project scope.**
   `EngineRegistry::new()` is currently per-doc inside
   `build_html_pipeline_stages_with_options`. Build it once per
   project (or once per process) and inject. Expected win: \~37 %
   of main-thread CPU + most of the I/O wait.

2. **Make `find_executable` lazy / cached.**
   Even if (1) is unwieldy, the cheaper fix is to memoize
   `JupyterEngine::find_jupyter()` and `KnitrEngine::find_*` with
   a `OnceLock<Option<PathBuf>>` so the subprocess spawn happens
   at most once per process. This is the minimum-blast-radius
   variant of (1).

3. **Stop attaching tree-sitter `set_logger` on the success path.**
   Parse without a logger first. If the resulting tree has error
   nodes, re-parse with the logger attached for diagnostic
   generation. Expected win: \~45 % of main-thread CPU on
   error-free renders.

4. **Stream `pass1_failures` / `pass2_failures` to stderr as they
   happen**, instead of batching to end-of-render. UX-only;
   doesn't change wall time but eliminates the silence.

5. **Investigate `ConfigValueKind` drop / `ConfigValue::clone`
   weight** (\~70 samples combined). Likely tied to per-doc
   `Pandoc.meta` cloning during pass_one or merge. Probably
   sub-10 % win — lower priority than (1)-(3).

## Reproducibility

Build:

```bash
cargo build --release --bin q2
cargo build --profile=release-perf --bin q2
```

Baseline:

```bash
/usr/bin/time -l target/release/q2 render external-sources/quarto-web
```

Sample:

```bash
target/release-perf/q2 render external-sources/quarto-web > /dev/null 2>&1 &
PID=$!
sample $PID 4 1 -file /tmp/q2-sample.txt -mayDie
```

Self-time tally:

```python
# Parse macOS sample output, computing self = count - sum(immediate_children)
# (see this note's `Method` section for the inline script used)
```

Scaling fixtures:

```bash
for N in 1 10 50 100 300; do
  rm -rf /tmp/q2-perf-scale/$N
  mkdir -p /tmp/q2-perf-scale/$N
  cp /tmp/q2-perf-scale/_quarto.yml /tmp/q2-perf-scale/$N/
  for i in $(seq 1 $N); do
    printf -- '---\ntitle: "Doc %s"\n---\n\n# Doc %s\n\nText.\n' "$i" "$i" \
      > /tmp/q2-perf-scale/$N/doc$i.qmd
  done
done
for N in 1 10 50 100 300; do
  perl -MTime::HiRes=time -e '
    my $s = time; system("target/release/q2 render /tmp/q2-perf-scale/'$N' > /dev/null 2>&1");
    printf "N='$N'  %.3fs\n", time - $s'
done
```

Sample file used here: `/tmp/q2-sample.txt` (not committed; 425 KB,
1290 samples on the main thread).

## 2026-05-22 follow-up: post-fix profile after bd-c5u2g

`bd-c5u2g` replaced `JupyterEngine`'s `sh -c "command -v jupyter"`
subprocess spawn with `which::which` (in-process PATH walk),
matching what `KnitrEngine`/`find_rscript` already did, and added
`OnceLock`-backed memoization for both. See
`claude-notes/plans/2026-05-22-engine-discovery-cache.md`.

### Wall-time impact

| Fixture                | Pre-fix | Phase A | Phase B | A+B vs Pre |
|------------------------|--------:|--------:|--------:|-----------:|
| 10-doc synthetic       |   62 ms |   37 ms |   39 ms |       −37 %|
| 50-doc synthetic       |  206 ms |   89 ms |   99 ms |       −52 %|
| 100-doc synthetic      |  401 ms |  153 ms |  162 ms |       −60 %|
| 300-doc synthetic      | 1163 ms |  452 ms |  463 ms |       −60 %|
| 573-doc **quarto-web** | 3280 ms | 2350 ms | 2300 ms |       −30 %|

Phase A (replacing the spawn with `which::which`) captures
essentially all of the wall-time gain — each cached lookup is
already fast enough that memoization adds little measurable
saving. Phase B remains valuable as the regression tripwire: the
`perf.engine-discover` counter now reads `jupyter=1 rscript=1`
on every render regardless of doc count, so a future change that
re-introduces per-doc discovery work is detectable from the gauge
alone.

### Post-fix samply top self-time

Sample file: `claude-notes/research/2026-05-22-quarto-web-postfix-profile.json.gz`
plus the `.syms.json` sidecar.

```
total_samples: 2434  (vs. 1290 pre-fix in the same window — main
                      thread now busy with real work instead of
                      blocked on subprocess `poll`)

pct  samples  symbol
9.0  220      _platform_memmove
4.0   98      mkdir
4.0   97      libsystem_c.dylib (unresolved — probably vfprintf chain)
3.3   81      __open
3.2   77      core::str::converts::from_utf8
2.9   71      ts_parser__advance
2.6   62      ts_subtree_summarize_children
2.4   59      ts_language_next_state
2.4   58      SplitWhitespace::next
2.3   56      stat
2.0   49      TreeSitterLogObserver::log
1.6   39      __open_nocancel
1.4   35      ts_tree_cursor_child_iterator_next
1.3   32      __getdirentries64
1.3   32      stack__iter
1.2   29      ts_parser__recover
0.9   23      fun_374a9d0 (unresolved Rust frame)
0.9   22      ts_subtree_release
0.8   20      libsystem_malloc.dylib (unresolved)
0.8   19      ts_parser_parse
0.8   19      ts_lex
```

### What the post-fix profile tells us

1. **The subprocess spawn is gone.** `posix_spawn`, `Command::output
   → poll`, and the entire 482-sample chunk attributed to
   `EngineRegistry::new → JupyterEngine::new` no longer appear.
   `print_render_diagnostics` reports `perf.engine-discover
   jupyter=1 rscript=1` for the full 573-doc render.
2. **The tree-sitter logger snprintf chain mostly evaporated.**
   Pre-fix had `__vfprintf` (209), `__sfvwrite` (106), `_vsnprintf`
   (47), `snprintf` (46) — \~478 self-samples combined. Post-fix
   has roughly 97+26+16+15+14+12+11 ≈ 191 in unresolved
   `libsystem_c.dylib` frames (some of which may not even be the
   snprintf chain). This is a substantial drop and possibly hints
   that some of the apparent "logger cost" in the pre-fix profile
   was actually *contention* or sampling skew tied to the spawn
   wait. The original write-up's prediction that the logger would
   become the new top after the spawn fix turned out to be
   *partly* wrong: it didn't take the top spot, although it's
   still measurable (49 samples on the Rust-side log callback).
3. **The new top is `_platform_memmove` (9 %).** memcpy work,
   probably tree-sitter buffer growth or AST cloning. Worth a
   focused follow-up if we want to cut more — but it's not in
   bd-c5u2g's scope.
4. **File-system syscalls take \~12 % combined** (`mkdir 4.0`,
   `__open 3.3`, `stat 2.3`, `__open_nocancel 1.6`,
   `__getdirentries64 1.3`). Plenty of small I/O per document.
   Candidates for batching or for caching project-walk results.
5. **Tree-sitter parsing machinery is now the dominant
   identifiable Rust cluster** (`ts_parser__advance 2.9`,
   `ts_subtree_summarize_children 2.6`, `ts_language_next_state
   2.4`, plus 5–6 others smaller). This is the *expected*
   hotspot for a Markdown parser; reducing it requires either
   fewer parses or a faster grammar, both more involved than the
   engine-discovery fix.

### Suggested follow-ups (updated)

Of the candidates listed in the pre-fix section, the order of
attractiveness changes after this fix:

- (kept) **Stream `pass1_failures` to stderr as they happen.**
  UX-only; wall time unchanged but the user no longer sees a
  silent 2.3 s before any feedback. Independent of perf.
- (refined) **Investigate `_platform_memmove` weight.** Now the
  top self-time symbol; figure out who's calling it most and
  whether it's avoidable.
- (refined) **Stop attaching tree-sitter `set_logger` on the
  success path.** Smaller expected win than the original
  prediction (logger is no longer dominant), but the change is
  cheap and removes a class of overhead entirely.
- (kept) **Parallelize `pass_one` / `pass_two`.** Both are
  currently sequential `for ... .await` loops. With per-doc cost
  down to \~4 ms and no shared state poisoned by `posix_spawn`
  waiting, parallelization should now yield close to a linear
  speedup on multi-core machines. Worth a separate plan.
- (kept) **Audit `ConfigValueKind` drop / clone weight** — still
  visible in the profile, lower priority than the items above.

## 2026-05-22 follow-up #2 — Pass-1 parallelized via rayon (bd-m7x9s)

After landing bd-m7x9s (Phase 2: rayon `par_iter` fan-out over
`ProjectPipeline::pass_one`):

### Wall-time comparison

Measured on the same 18-core M-series Mac as the post-engine-discovery
follow-up. Cold cache means `rm -rf _site .quarto/cache` before each
run. Numbers are means of 3 runs; release-perf binary.

| Scenario                                                     | Pass-1 wall | Total wall | vs sequential Pass-1 | vs sequential total |
|--------------------------------------------------------------|------------:|-----------:|---------------------:|--------------------:|
| Sequential (`QUARTO_JOBS=1`), cold cache                     |     2230 ms |    3.73 s  |       — (baseline)   |      — (baseline)   |
| Parallel (default 18 workers), cold cache                    |     1280 ms |    2.81 s  |             1.74×    |            1.33×    |
| Sequential (`QUARTO_JOBS=1`), warm cache                     |      800 ms |    2.29 s  |       — (baseline)   |      — (baseline)   |
| Parallel (default), warm cache                               |      520 ms |    2.03 s  |             1.54×    |            1.13×    |

### Scaling with `QUARTO_JOBS`

Cold cache, varying worker count:

| QUARTO_JOBS | Pass-1 wall | Total wall |
|------------:|------------:|-----------:|
|           1 |     2230 ms |     3.73 s |
|           2 |     1446 ms |     2.94 s |
|           4 |     1326 ms |     2.85 s |
|           8 |     1671 ms |     3.21 s |
|          18 |     1284 ms |     2.81 s |

Returns diminish sharply past 2 workers; FS-bound work (cache file
atomic rename per doc, source file reads, layered `_metadata.yml`
walks) serializes at the file-system level. The 8-worker outlier is
within run-to-run noise.

### Post-parallel profile (samply, symbolicated)

Captured at `2026-05-22-quarto-web-parallel-pass1-profile.json.gz`.

Top self-time symbols (cold-cache, default worker count, 21 393
samples ≈ 21 s of CPU time across all threads):

| pct    | symbol                                   |
|-------:|------------------------------------------|
| 53.15% | `__ulock_wait2` (libsystem_kernel.dylib) |
| 20.36% | `__ulock_wake`                           |
|  2.37% | `os_unfair_lock_lock` (libsystem_platform) |
|  2.24% | `_platform_memmove`                      |
|  1.30% | `_os_unfair_lock_lock_slow`              |
|  0.71% | `__open` (libsystem_kernel)              |
|  0.70% | `core::str::converts::from_utf8` (q2)    |
|  0.66% | `ts_parser__advance` (q2)                |
|  0.65% | `mkdir`                                  |
|  0.49% | `ts_language_next_state` (q2)            |
|  0.45% | `ts_subtree_summarize_children`          |
|  0.43% | `__psynch_cvwait`                        |
|  0.35% | `stat`                                   |
|  0.30% | `TreeSitterLogObserver::log` (q2)        |

74% of CPU-time samples are now in kernel wait/wake (rayon worker
idle time at the work-stealing barrier — the workers finish Pass-1
quickly and then idle while the still-sequential Pass-2 + post-render
run on the main thread). The actual Rust work is < 5% of samples.

This is consistent with the wall-time scaling pattern: Pass-1 itself
is no longer a bottleneck per-thread — the limiter is now the
fraction of total work that's still sequential (Amdahl).

### Suggested follow-ups (after Pass-1 parallel lands)

- (new top candidate) **Parallelize Pass-2** — the remaining 1.5 s of
  the 2.8 s cold-cache wall time. Larger refactor (per-doc Pass-2
  drains into shared `project_artifacts`, `resource_copies`; the
  orchestrator currently mutates these serially). File as a separate
  beads issue.
- (kept) **Stream `pass1_failures` to stderr as they happen.** UX
  win, no perf change.
- (kept) **`_platform_memmove` audit.** 2.24% of samples now, mostly
  in PathBuf/String allocations downstream of cache-key construction.
- (kept) **Stop attaching tree-sitter `set_logger` on success path.**
  \~0.3% of samples; still cheap to remove.
- (new) **Reduce per-doc temp-dir creation.** Each
  `StageContext::new` does `runtime.temp_dir(...)` which lands a
  `mkdir` syscall. Pass-1 stages don't appear to use the temp dir at
  all; making the temp-dir creation lazy would remove `mkdir`,
  `__open`, and the FS contention they cause.
- (new) **Batch profile-cache writes.** Atomic-rename per doc is
  \~4 syscalls each. Holding writes in memory and committing at
  pass_one return would cut FS syscalls by 4×.
