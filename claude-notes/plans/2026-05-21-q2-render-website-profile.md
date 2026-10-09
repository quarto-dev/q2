# Profile `q2 render` on a large website (quarto-web)

## Overview

Rendering large websites quickly is a core goal for Q2. Today,
`q2 render external-sources/quarto-web` errors out in \~4.8s before
producing any output. Once the resource-path issues (plans
`2026-05-21-resource-path-leading-slash.md` and
`2026-05-21-resource-path-diagnostic.md`) are resolved, the project
will progress further into the pipeline. Either way — error or
full render — there is several seconds of work to characterize.

This issue is a *profiling investigation*, not a fix. The output is
a written analysis plus, very likely, a fan-out of follow-up beads
issues for any hotspots we find that warrant action. We do not
commit to specific optimizations up front.

## Why quarto-web

- Real, large, public-facing website with hundreds of documents.
- Mixed content: many `.qmd` files, a website navbar with deep
  sidebars, listings, blog posts, includes, citations.
- TS Quarto already renders it routinely, so we have a ground-truth
  reference for what "complete" looks like.
- Lives in `external-sources/quarto-web/` — committed and consistent
  across developer machines.

Note: per the External Sources Policy, `external-sources/` is *not*
checked into Q2 directly; this means CI cannot run this profile
unattended. The profile work is run-on-demand and produces written
artifacts in `claude-notes/plans/` (or `claude-notes/research/`) for
durability.

## Workflow (per `claude-notes/instructions/performance-profiling.md`)

The "native-proxy-first" workflow:

1. **Build a representative fixture.** First try is the full
   quarto-web checkout. If too noisy, distill a smaller fixture that
   reproduces the same shape (e.g. N documents, M blog posts).
2. **Scale geometrically.** Where possible, parametrize fixture
   size — N = 10, 30, 100, 300 docs — and measure wall time + key
   counters. Geometric scaling is what lets us *see* complexity
   class instead of guessing.
3. **Add env-gated counters.** The codebase already uses
   `QUARTO_PERF_STATS=1` with `perf.<gauge-name>` prefixes (see
   `crates/pampa/src/writers/json.rs` for the reference pattern).
   Add counters where we suspect work is happening (per-document
   render time, YAML re-parses, tree-sitter parse count, etc.) —
   *not* end-to-end timing alone.
4. **Confirm complexity class empirically.** Plot or table the
   per-scale numbers. Linear, quadratic, super-linear?
5. **Only then design a fix.** Each candidate fix becomes its own
   beads issue. Do not bundle "investigate" and "fix" into the same
   ticket.

The instruction file is explicit about *not* iterating perf fixes
in the browser; this is a CLI investigation start to finish.

## Steps

### Phase 1: baseline + flamegraph

- [ ] Confirm the resource-path fixes have landed (or work on a
  branch that has them). Without them, this issue cannot proceed
  past the 4.8s error.
- [ ] Time a single `cargo run --release --bin q2 -- render
  external-sources/quarto-web` end-to-end. Record:
  - wall time;
  - peak RSS;
  - whether the render completes or errors out (we expect *some*
    further errors since quarto-web isn't a Q2 project yet —
    triage those into separate issues, don't fold the fixes into
    this profile).
- [ ] Generate a flamegraph (cargo-flamegraph or samply, whichever
  is wired up in this tree — check `claude-notes/instructions/`).
  Commit the SVG and the analysis notes under
  `claude-notes/plans/` or `claude-notes/research/`.

### Phase 2: counter instrumentation

Walk down the flame graph; for each top frame, add a
`QUARTO_PERF_STATS=1`-gated counter. Examples we already suspect
will pay off:

- Documents discovered vs. documents rendered.
- Number of tree-sitter parses (qmd block + inline).
- Number of YAML parses (the same file shouldn't be re-parsed).
- Profile-stage cache hits/misses.
- I/O calls per document.

After each set of counters lands, re-run with
`QUARTO_PERF_STATS=1 cargo run --release ... 2>&1 | grep ^perf.`
and write the numbers into the analysis doc.

### Phase 3: complexity-class checks

Where a counter looks suspicious — particularly anything that grows
faster than linearly in document count — scale the input down to
a smaller fixture and confirm the relationship empirically:

```bash
for N in 10 30 100 300; do
  build-fixture --docs $N --out /tmp/q2-perf/$N
  QUARTO_PERF_STATS=1 cargo run --release --bin q2 -- render /tmp/q2-perf/$N
done
```

Tabulate. *Then* write a follow-up issue for any hotspot that's
super-linear, with a concrete fix proposal.

### Phase 4: synthesis

- [ ] Single written analysis at
  `claude-notes/research/2026-05-21-quarto-web-render-profile.md`
  with:
  - the methodology used,
  - the raw numbers,
  - the top-N hotspots,
  - one follow-up beads issue per actionable hotspot (linked).
- [ ] Update this plan with the final list of follow-up issues.

## Work Items

- [x] Phase 1 baseline + sample profile
      (`claude-notes/research/2026-05-21-quarto-web-render-profile.md`).
      `samply` not installed; used macOS `/usr/bin/sample` for the
      initial pass — sufficient to identify the two dominant
      hotspots. If we want a flamegraph SVG, we should `cargo
      install samply` and re-run.
- [ ] Phase 2 counter instrumentation (deferred — the sample
      already identified two large hotspots; counters become useful
      once we want to verify a *fix* didn't regress something else).
- [x] Phase 3 geometric-scale confirmation (linear: \~4 ms/doc).
- [x] Phase 4 written synthesis at
      `claude-notes/research/2026-05-21-quarto-web-render-profile.md`.
      Follow-up issues *suggested* in the synthesis but not yet
      filed in beads — pending user direction on which to prioritize.

## Constraints / caveats

- **No iterative perf fixes inside this issue.** Discoveries become
  separate beads issues. This one only produces an analysis.
- **Release builds only.** Debug builds have wildly different
  profiles; don't waste time profiling them.
- **Triage real bugs separately.** While running quarto-web through
  Q2 we will discover features Q2 doesn't implement yet (custom
  shortcodes, particular listing options, etc.). Those become their
  own beads issues, not part of this profile.

## Dependencies

- Blocked by: `2026-05-21-resource-path-leading-slash.md`. Without
  it, the render terminates at \~4.8s before we ever reach the
  interesting work.
- The diagnostic plan (`2026-05-21-resource-path-diagnostic.md`) is
  *not* a blocker for this; it can land in parallel.

## Result

See `claude-notes/research/2026-05-21-quarto-web-render-profile.md`
for the full write-up. Headlines:

- Two hotspots account for \~80 % of main-thread CPU on the 3.28 s
  render:
  - per-doc `JupyterEngine::new()` subprocess spawn (\~37 %);
  - per-doc tree-sitter `set_logger` formatting (\~45 %).
- Both can be addressed independently and the fixes are small.
- A *separate* UX bug — batching diagnostics to end-of-render —
  explains the 3.28 s of silence the user observed; fixing it
  alone would make Q2 *feel* fast even before the perf hotspots
  are touched.
- Five candidate follow-up issues identified, none filed yet
  pending user direction.
