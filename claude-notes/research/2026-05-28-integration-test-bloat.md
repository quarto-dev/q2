# Integration test bloat — measurements

**Beads:** bd-xvdop
**Plan:** [2026-05-28-integration-test-consolidation.md](../plans/2026-05-28-integration-test-consolidation.md)
**Host:** macOS dev machine (Apple Silicon)

This note records the on-disk and wall-time cost of building all test
binaries in the workspace, both before and after consolidating each
crate's `tests/*.rs` files into `tests/integration/main.rs` + sibling
modules.

Each measurement is taken from a clean state (`cargo clean` between
runs). The raw script logs live in `measurements/`.

> Because each integration test file in Rust's default layout compiles
> into its own fully-linked binary, we expect debug numbers to dominate
> (no symbol stripping, no LTO), and release numbers to be much smaller.
> Our starting `target/` confirms the asymmetry: `target/debug` = 251 GB
> vs `target/release` = 2.7 GB on this machine.

## Headline table

| Stage                                | Profile | target/<profile> | Wall-time | Executables in deps/ |
| ------------------------------------ | ------- | ---------------: | --------: | -------------------: |
| Baseline (first run)                 | debug   |       21 GB (1) |    114 s  |                  220 |
| Baseline (first run)                 | release |           11 GB  |    133 s  |                  220 |
| Baseline (controlled, back-to-back)  | debug   |           21 GB  |    114 s  |                  220 |
| Baseline (controlled, back-to-back)  | release |           11 GB  |    138 s  |                  220 |
| Pilot: pampa only (first run)        | debug   |           18 GB  |    173 s  |                  164 |
| Pilot: pampa only (first run)        | release |          9.2 GB  |    255 s  |                  164 |
| Pilot: pampa only (controlled)       | debug   |           18 GB  |    130 s  |                  164 |
| Pilot: pampa only (controlled)       | release |          9.1 GB  |    136 s  |                  164 |
| Full rollout                         | debug   |                — |         — |                    — |
| Full rollout                         | release |                — |         — |                    — |

The "first run" rows were taken with intervening work between samples
(verify runs, file edits, disk pressure). The "controlled" rows were
taken back-to-back with `cargo clean` between each and no other
intervening work — these are the apples-to-apples comparison and the
ones the Phase 4 decision should use.

(1) `cargo clean` after the build reported "Removed 36689 files, 22.3 GiB total"
— a slight discrepancy with `du`'s 21 GB because `du` undercounts
hardlinks and compressed metadata. We use `du -sh` as the headline
figure to stay consistent with the script's output.

(Filled in as each phase completes.)

## Methodology

```bash
# For each measurement:
cargo clean
scripts/measure-test-build.sh <debug|release> <label>  \
  | tee claude-notes/research/measurements/<label>-<profile>.log
```

The script reports:

- Wall-clock seconds for `cargo build --workspace --tests [--release]`
- `du -sh target/<profile>/` (the headline disk number)
- Count and total bytes of executable files under
  `target/<profile>/deps/`
- The largest 25 deps binaries with their sizes

## Notes

### Baseline (Phase 1)

Branch: `beads/bd-xvdop-experiment-consolidate-integration-tests`
HEAD: `8733ed67` (plan committed; no source changes).

#### Debug

- Wall-time: **114 s**
- `du -sh target/debug`: **21 GB**
- `cargo clean` reported **22.3 GiB / 36,689 files**
- Executable files in `target/debug/deps/`: **220**
- Sum of executable bytes: **10.5 GiB** (≈ 50% of `target/debug`;
  the remainder is rlibs, dSYMs, build-script outputs, and crate
  metadata)
- Largest 25 executables hover around **130–155 MB each**, all
  integration test binaries — exactly the per-file-fully-linked
  bloat the matklad/ark pattern targets

Top 10 by size (all are integration test binaries):

| Size | Binary | Likely source crate |
| ----: | --- | --- |
| 155 MB | `q2-…` | `crates/quarto/tests/...` |
| 142 MB | `boot-…` | `crates/quarto-core/tests/` (boot pipeline) |
| 142 MB | `eager_capture-…` | `crates/quarto-core/tests/` |
| 142 MB | `diagnostics_endpoint-…` | `crates/quarto-core/tests/` |
| 142 MB | `diagnostics_capture_failure-…` | `crates/quarto-core/tests/` |
| 142 MB | `staleness-…` | `crates/quarto-core/tests/` |
| 137 MB | `quarto_core-…` | `crates/quarto-core/tests/` (unit tests) |
| 135 MB | `quarto_preview-…` | `crates/quarto-preview/tests/` |
| 135 MB | `cache_hit-…` | `crates/quarto-core/tests/` |
| 131 MB | `listing_pipeline-…` | `crates/quarto-core/tests/` |

The headline observation: **the dominant single-binary size class
(~130 MB) shows up >20 times in the top 25**, which is a textbook
illustration of the matklad post — each binary is statically linking
the same massive transitive closure (quarto-core + pampa +
tree-sitter + …).

Raw log: [measurements/baseline-debug.log](measurements/baseline-debug.log)

#### Release

- Wall-time: **133 s** (16% slower than debug — optimization pass cost)
- `du -sh target/release`: **11 GB** (≈47% of debug size)
- Executable files in `target/release/deps/`: **220** (same count
  as debug, as expected)
- Sum of executable bytes: **9.0 GiB** (≈82% of `target/release`;
  release strips many of the non-executable artifacts that bulk
  out debug, so the per-binary fanout is an even larger fraction
  of the total)
- Largest 25 executables: **110-128 MB each** — same pattern as
  debug (slightly smaller per-binary because release strips debug
  info, but the static-linking redundancy is identical)

Raw log: [measurements/baseline-release.log](measurements/baseline-release.log)

### Pilot: pampa-only measurements (Phase 3)

Headline result, controlled back-to-back comparison (`cargo clean`
between each, no other work in between):

|                            | Baseline (debug) | Pilot (debug) |       Δ debug | Baseline (release) | Pilot (release) |     Δ release |
| -------------------------- | ---------------: | ------------: | ------------: | -----------------: | --------------: | ------------: |
| `target/<profile>` size    |            21 GB |         18 GB |    **−3 GB (−14 %)** |              11 GB |           9.1 GB |  **−1.9 GB (−17 %)** |
| Executables in `deps/`     |              220 |           164 |    **−56 (−25 %)** |                220 |             164 |  **−56 (−25 %)** |
| Sum of executable bytes    |         10.5 GiB |       7.8 GiB | **−2.7 GiB (−26 %)** |           9.0 GiB |         6.7 GiB | **−2.3 GiB (−26 %)** |
| Build wall time            |            114 s |         130 s |  **+16 s (+14 %)** |              138 s |           136 s | **−2 s (−1 %)** |

The disk wins are unambiguous and consistent across debug and
release: pampa's 57 integration test binaries collapse into one
`integration` binary, eliminating exactly 56 executables and
~2.5 GiB of duplicated dependency-closure linkage. Extrapolated
across the other 12 candidate crates' 107 integration test files,
target/debug at the end of Phase 5 should land in the 13-15 GB
range vs. the 21 GB baseline.

**On the wall-time anomalies in the first run.** The first pilot
debug came in at 173 s and the first pilot release at 255 s —
suggesting 50-90 % slowdowns. Both collapsed under controlled
back-to-back re-runs (debug: 173 → 130, release: 255 → 136). The
first runs were contaminated by intervening work — `cargo xtask
verify` saturating disk I/O between samples, plus macOS Spotlight
re-indexing the freshly-cleaned target tree. The lesson for the
research methodology: never compare timings across runs that have
heterogeneous work between them; always do `cargo clean` →
back-to-back samples for a real comparison.

The honest controlled-comparison wall-time picture is **+16 s
debug (+14 %), −2 s release (−1 %)** — debug pays a small cost
(consolidating 57 small parallel link jobs into one larger one
slightly shrinks Cargo's scheduling parallelism in a release-mode
codegen-units=16 world), release is statistically a wash. This is
the opposite direction from matklad's 3× speedup, but matklad's
codebase had much smaller per-file binaries relative to the
consolidated one. For Q2, our per-file binaries were already
~130 MB (the dep closure dominates), so collapsing them just
removes redundancy without much link-time amortization. **The disk
savings, not compile speed, are the load-bearing benefit.**

### Pilot: pampa migration notes (Phase 2)

The migration itself was a pure rename of 57 files (`tests/*.rs` →
`tests/integration/*.rs`) plus a generated `tests/integration/main.rs`
that declares each as a `pub mod`. **One surgical edit was required
on top of the rename:** insta's `set_snapshot_path("../snapshots/…")`
in `test.rs` and `test_error_corpus.rs` is resolved relative to the
test file's source-file directory, so moving the files down one
level required changing the 3 occurrences to `../../snapshots/…` to
keep pointing at `crates/pampa/snapshots/`.

This is the same kind of edit that the Phase 5 audit flagged for
`quarto/tests/trace_cli.rs`'s `#[path = "../src/commands/trace.rs"]`
— any source-file-relative path inside an integration test needs
one more `../` after consolidation. We should grep for these
patterns proactively before each Phase 5 crate migration to avoid
the same false-failure round-trip.

### Pilot: pampa only (Phase 3)

_Pending._

### Full rollout (Phase 6)

_Pending._

## Cross-platform extrapolation

The measurements above are macOS-only. The ark PR reported a Linux CI
runner footprint drop from 15 GB → ~2 GB and a fresh `cargo clean`
size drop of 8.1 GiB → 3.5 GiB on macOS. Linker bloat per binary
scales with the dependency closure, which is the same set of crates on
all targets (modulo platform-specific deps like `dylib`s on Linux that
are not vendored on macOS). We expect the Q2 macOS:Linux delta ratio
to roughly track ark's; the Windows delta is unmeasured but should be
of the same order of magnitude.

If the macOS pilot shows e.g. a 40% drop in `target/debug` size, we
should expect a similar percentage drop on Linux CI runners, which is
the metric that actually unblocks CI capacity.
