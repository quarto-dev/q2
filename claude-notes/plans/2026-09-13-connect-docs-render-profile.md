# Time-profile `q2 render` on the Connect docs (docs-quarto-2)

**Date:** 2026-09-13
**Strand:** bd-fq44dlnm
**Playbook:** `claude-notes/instructions/performance-profiling.md`
**Previous whole-project runs:**
`claude-notes/research/2026-05-21-quarto-web-render-profile.md` (bd-9eltv),
`claude-notes/plans/2026-06-01-render-perf-profiling.md` (qmd-plans site).

## Overview

It has been ~3 months since the last whole-project time profile. This
run re-characterizes where `q2 render` spends its time on a real,
large-ish, *non-Quarto-team* project: the Posit Connect documentation
at `~/repos/github/cscheid/q2-connect-docs/docs-quarto-2`.

Fixture shape (measured 2026-09-13, `_site/` and `.quarto/` excluded):

| section  | files | bytes     | notes                                   |
|----------|------:|----------:|-----------------------------------------|
| admin    |   173 | 1,634,947 | deep sidebar, generated config appendix |
| api      |     2 | 1,144,693 | one 1.1 MB generated `api/index.qmd`    |
| cookbook |   117 |   382,056 |                                         |
| user     |    76 |   536,598 |                                         |
| how-to   |     5 |    35,459 |                                         |
| news     |     3 |     1,199 |                                         |
| **total**| **352 rendered inputs (206 .qmd + .md)** | **2.3 MB** | `posit-docs` project type, `quarto-openapi` + `mermaid-zoom` + `quarto-tiers` extensions, `llms-txt: true` |

This is a *profiling investigation*, not a fix. Output: a research note
with numbers + ranked hotspots, and one follow-up strand per hotspot
worth acting on. No optimizations are committed to up front.

Not in the repo: the fixture lives outside `q2`, so this cannot run in
CI. It is run-on-demand; durability comes from the written note.

## Checklist

### Phase 1: baseline

- [x] Build `target/release/q2` and `target/release-perf/q2`.
- [x] Confirm the render completes (or record how far it gets). Note
      the project's local `_quarto.yml` diff (pre/post-render scripts
      commented out) — profile without the scripts.
- [x] Cold (`--clean-cache`) vs warm wall time + peak RSS,
      `/usr/bin/time -l`, median of 3, release build.
- [x] `QUARTO_PERF_STATS=1` gauges: `perf.pass1`, `perf.pass2`,
      engine discovery, vfs-write, intern.
- [x] `QUARTO_JOBS=1` serial wall vs default parallel wall
      (parallel efficiency on this machine).

### Phase 2: samply profiles

- [x] Serial profile (`QUARTO_JOBS=1`, release-perf, 1 kHz) → self-time
      top-40 via `crates/perf-harness/scripts/analyze_profile.py`.
- [x] Parallel profile (default jobs) → same table; check lock-wait
      share (`__ulock_wait2`, `os_unfair_lock`) vs the 2026-06-01
      baseline of ~4.7 %.
- [x] ~~If any hot frame is an unsymbolicated system-library address, re-sample with macOS `sample`~~ — not needed; the hot frames were all in `q2` (grass), malloc addresses attributable via callers.

### Phase 3: per-document distribution

- [x] Is time dominated by the 1.1 MB `api/index.qmd`, or spread
      evenly? Time that single document alone vs the project total.
- [ ] Scale check on the heaviest doc shape (2×, 4×, 8× concatenation)
      to confirm complexity class — deferred to bd-is4q72tt / bd-5yektmwt; the
      dominant hotspot (SCSS) is a per-document constant, not a per-size term.

### Phase 4: write-up

- [x] Research note `claude-notes/research/2026-09-13-connect-docs-render-profile.md`
      with commands, tables, verbatim gauge output, ranked findings.
- [x] One strand per actionable hotspot, linked
      `discovered-from:bd-fq44dlnm`.
- [x] Compare against the 2026-06-01 bucket table (tree-sitter ~29 %,
      memmove/AST ~13 %, fs ~14 %) — what moved?

## Findings

Full write-up: `claude-notes/research/2026-09-13-connect-docs-render-profile.md`.

- **78 % of serial render time is grass compiling Bootstrap SCSS** — the sass
  cache key hashes the document-relative theme path, so 349 directories →
  ~698 keys for identical content; the 10 MB LRU thrashes.
  `perf.sass hits=22 compiles=682 uncached=0`. → **bd-79c4do6g (P1)**.
- LRU index lost-updates under parallel Pass 2 leak orphan cache files
  (31 → 70 → 93 files across two parallel runs; 315 / 101 MB pre-existing).
  → **bd-ddahjqr1 (P2)**.
- Peak RSS ~1000× source: 1.1 MB `api/index.qmd` → 1.1 GB; project 2.5 GB
  serial / 3.6 GB parallel. → **bd-is4q72tt (P2)**.
- Single-page render re-renders all 16 listing pages (17 docs for one page).
  → **bd-j0hmi3rx (question)**.
- Post-SCSS projection: memmove/AST construction 28 % + malloc 17.5 %,
  tree-sitter 15 %. → **bd-5yektmwt (P3)**.

### Code changes made during the investigation (on `braid/bd-fq44dlnm-connect-docs-profile`)

- `crates/quarto-core/src/stage/stages/compile_theme_css.rs`: `perf.sass`
  gauge (hits / compiles / uncached; atomics, printed under
  `QUARTO_PERF_STATS=1`), exported via `stage/stages/mod.rs`, printed from
  `crates/quarto/src/commands/render.rs` with the other gauges.
- `crates/perf-harness/scripts/bucket_profile.py`: per-crate / per-stage /
  caller bucketing companion to `analyze_profile.py`.
- Profiles committed next to the research note (serial + parallel, with
  `.syms.json` sidecars), following the 2026-05-22 precedent.
- clippy (`-D warnings`) clean on `quarto-core` + `quarto`; 152 theme/cache
  tests pass. Full `cargo xtask verify --skip-hub-build` **not yet run**.

### Smoke run (release-perf binary, `--clean-cache`, default jobs, 2026-09-13)

```
$ q2 render --clean-cache      # in docs-quarto-2, _environment sourced
Rendering project: .../docs-quarto-2 (type: posit-docs (website))
Rendered 352 of 352 files to .../docs-quarto-2/_site
        9.56 real        68.44 user         3.66 sys
          3656384512  maximum resident set size      # 3.4 GiB
        947793860864  instructions retired
```

- Exit 0, no warnings. `_site/` is 78 MB; `api/index.html` alone is 2.0 MB.
- **68 s of CPU for 2.3 MB of markdown** (~190 ms CPU per input) vs. the
  2026-06-01 qmd-plans baseline of ~8 ms/file — and **3.4 GiB peak RSS**.
  Both are the first things the profile has to explain.
