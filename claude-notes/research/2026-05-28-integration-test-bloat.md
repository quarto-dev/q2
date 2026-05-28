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

| Stage                | Profile | target/<profile> | Wall-time | Executables in deps/ |
| -------------------- | ------- | ---------------: | --------: | -------------------: |
| Baseline             | debug   |       21 GB (1) |    114 s  |                  220 |
| Baseline             | release |           11 GB  |    133 s  |                  220 |
| Pilot: pampa only    | debug   |                — |         — |                    — |
| Pilot: pampa only    | release |                — |         — |                    — |
| Full rollout         | debug   |                — |         — |                    — |
| Full rollout         | release |                — |         — |                    — |

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
