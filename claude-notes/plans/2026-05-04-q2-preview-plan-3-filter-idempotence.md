# Plan 3 — Filter idempotence verification

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** M2 verification gate (no new milestone — locks in property
on what's already shipped)

## Goal

Verify and lock in the **idempotence + structural-hash-stability** contract
for the q2-preview pipeline. This is the contract the user has stated must be
the foundation: every transform and every built-in Lua filter must produce the
same structural output when run twice on the same input. Without this, the
incremental writer's reconciliation cannot reliably preserve untouched
regions.

This plan ships:
- A canonical fixture set covering the q2-preview transforms.
- A test that runs each fixture through the q2-preview pipeline twice and
  asserts the resulting ASTs hash equal.
- Coverage for the built-in Lua filters that ship with Quarto (those in
  `resources/extensions/`).

When this plan lands, we have CI-enforced confidence that the q2-preview
round-trip story (Plans 4-8) rests on a stable foundation.

## Scope

### In scope

- Canonical fixture set: small `.qmd` files exercising:
  - Meta shortcode (single-inline resolution): `{{< meta foo >}}` where `foo`
    is a single string.
  - Meta shortcode (multi-inline resolution): `{{< meta foo >}}` where `foo`
    contains markdown like `**Bold** title`.
  - Include shortcode: `{{< include child.qmd >}}` (with a trivial child file).
  - Lua filter (mutating): a filter that uppercases all `Str.text`.
  - Lua filter (synthesizing): a filter that adds a `pandoc.Str("decoration")`
    to each paragraph.
  - Callout: `::: {.callout-warning} Body :::`.
  - Theorem: `::: {.theorem #thm-foo} Math here :::`.
  - Figure with cross-ref target: `:::: {#fig-foo} ![caption](img.png) ::::`.
  - Cross-reference: `See @thm-foo`.
  - Sectionized doc: a doc with `## Section A`, content, `### Subsection`,
    content, `## Section B`, content.
  - Combined: a doc with several of the above interacting.
- Idempotence test runner: takes a fixture, runs the q2-preview pipeline
  twice, hashes both ASTs via
  `quarto_ast_reconcile::compute_blocks_hash_fresh`, asserts equality.
- Coverage of the built-in extensions' filters (those in
  `resources/extensions/`):
  - For each shipped filter, run the test against a fixture that triggers
    that filter.
  - Document which built-in filters pass / fail (in case any are
    non-idempotent — flag for follow-up).
- Documentation in `claude-notes/instructions/`: a short note on the
  idempotence contract for filter authors and transform authors.

### Out of scope

- Verification of *user-supplied* filters. They're per-document; the contract
  is enforced at runtime via the idempotence test pattern, but we don't
  pre-verify every possible user filter.
- Rust-vs-React rendering parity (different contract; later plan).
- Performance / debouncing — idempotence verification doesn't measure runtime.

## Design decisions (settled in conversation)

- **The hash is already source-info-agnostic** (verified during research).
  `compute_block_hash_fresh` excludes `source_info`. Two runs producing nodes
  with different source_info but identical content/attr/plain_data hash
  identically. This is what makes the idempotence test work cleanly.
- **The contract's load-bearing property** is "double-pipeline-run produces
  hash-equal AST." Equivalent to "every transform is idempotent, every filter
  is idempotent, no transform is non-deterministic about plain_data or attr
  ordering."
- **Filter mutation provenance stays Original** (settled during conversation).
  Lua filter mutations don't change source_info. Constructions are tagged
  `Synthetic { by: By::filter(...) }` (post-Plan 5). Idempotence test sees
  consistent shape across runs.
- **Built-in filters in scope; user filters out**. Built-in filters ship with
  Quarto and the contract applies to them at CI time. User filters are
  enforced at edit-time (a non-idempotent user filter breaks q2-preview's
  round-trip; the user sees corruption).

## What gets tested concretely

For each fixture:

```
let pipeline = build_q2_preview_pipeline_stages();
let runtime = create_test_runtime();

let ast_1 = run_pipeline(fixture, pipeline.clone(), runtime.clone());
let ast_2 = run_pipeline(fixture, pipeline, runtime);

let hash_1 = compute_blocks_hash_fresh(&ast_1.blocks);
let hash_2 = compute_blocks_hash_fresh(&ast_2.blocks);

assert_eq!(hash_1, hash_2, "fixture {} non-idempotent", fixture_name);
```

Failure modes the test catches:

- A filter that's truly non-idempotent (e.g., `Str.text + "!"` produces
  growing text on each run).
- A transform that emits non-deterministic attributes or plain_data
  (e.g., HashMap iteration order in a sloppy implementation).
- A transform that mutates inputs differently across runs (probably
  indicates a bug).

Failure modes the test does NOT catch:

- A transform that's idempotent but produces *wrong* output (wrong-but-
  consistent — needs other testing).
- A filter that's idempotent for one input but non-idempotent for another
  (need representative fixtures).

## Open questions for implementation

- **Test infrastructure location**: probably `crates/quarto-core/tests/` as
  a workspace-level integration test crate. New test file like
  `q2_preview_idempotence.rs`. Confirm during implementation.
- **Fixture format**: just `.qmd` files in a fixtures dir, or in-source
  literal strings? Files are easier to maintain and review; literal strings
  are easier to keep with the test. Probably files for the substantial cases,
  literals for trivial ones.
- **How to drive the pipeline twice**: the natural approach is to build the
  pipeline once and run it twice, OR build two identical pipelines and run
  each on a fresh AST. Pipeline construction includes Lua engine setup which
  may be stateful — confirm the second-run pipeline starts fresh.
- **Built-in filter inventory**: enumerate the filters in
  `resources/extensions/`. Probably ~10-20. Each gets a fixture (or a
  shared fixture if the trigger pattern is similar).
- **CI failure expectation**: does the test fail noisily if any built-in
  filter is non-idempotent? Probably yes — that's the point. But we may
  discover at first run that one or more is non-idempotent, requiring a
  pre-existing fix before this plan can land.

## References

- `crates/quarto-ast-reconcile/src/hash.rs::compute_blocks_hash_fresh` — the
  hash function we use. Verified excludes source_info.
- `crates/quarto-ast-reconcile/src/hash.rs:768` — existing test
  `test_same_content_same_hash` — confirms hash excludes source_info.
- `crates/quarto-core/src/pipeline.rs::build_q2_preview_pipeline_stages` —
  the pipeline under test (created by Plan 1).
- `resources/extensions/` — built-in extensions with their Lua filters.
- `claude-notes/plans/lua-filter-pipeline/` — Carlos's earlier analysis of
  which filters are pure vs. side-effecting.

## Test plan

The plan IS the test plan. The deliverable is a test crate.

- Per-fixture idempotence assertion (the main loop above).
- Per-built-in-filter idempotence assertion.
- Combined fixture (sectionized doc with callouts and shortcodes) as a
  stress test.
- Documentation: when a future contributor adds a new transform or filter,
  they should add a fixture covering it. Document this expectation in
  `claude-notes/instructions/`.

## Dependencies

- Depends on: Plan 1 (`build_q2_preview_pipeline_stages` exists and runs).
- Blocks: implicitly Plans 4-8 (round-trip work assumes this contract holds).
  We don't need this to *implement* those plans, but landing it before
  reviewing them gives us confidence the foundation is solid.

### What happens when a fixture fails

Plan 3 reports failures; the *fix* lands in the appropriate downstream
plan, not in Plan 3. Three failure modes and where their fixes go:

- **Non-idempotent built-in Lua filter**. The filter's contract is
  broken. Fix: edit the filter's Lua source. Lands wherever the
  filter lives (typically `resources/extensions/...`). Plan 3 just
  surfaces the test.
- **Non-deterministic transform attribute ordering**. A transform that
  iterates a HashMap or similar and emits attrs in non-deterministic
  order. Fix: change the transform to emit deterministically. Lands
  in the transform's source file (typically a Plan 6-shaped fix even
  though it's not strictly a provenance issue — provenance audit and
  determinism audit are sister concerns).
- **Source-info-related instability**. Should NOT happen because the
  hash function excludes source_info. If somehow it does, Plan 4's
  type changes are the place to investigate.

If a fixture fails on first run, document the failure as a known issue
in Plan 3's commit message and file the fix as a follow-up against the
appropriate plan. Don't silently disable failing fixtures.

## Risk areas

- **A built-in filter might fail the test on first run**. If so, we either
  (a) fix the filter before this plan lands or (b) document the failure as
  a known issue and defer the fix. Plan should not silently disable failing
  filters from the test set.
- **Hash stability across binary versions**: `FxHasher`'s output is stable
  within a Rust process but not across versions. Tests should compare hashes
  computed in the same process, not stored as constants. This is the natural
  shape of "run pipeline twice and compare" anyway.
- **Pipeline construction non-determinism**: if the pipeline picks up extension
  paths in OS-dependent order, attributes could differ on different machines.
  Mitigated by fixture isolation — fixtures don't reference real OS paths
  unless explicitly testing a path-aware feature.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| Test runner harness | ~80 |
| Per-fixture qmd files | ~100 (across ~10 fixtures) |
| Per-fixture test assertions | ~150 |
| Built-in filter coverage | ~150 |
| Documentation | ~50 |
| **Total** | **~530** |

Probably one focused session. Risk: if a built-in filter fails idempotence,
fixing the underlying issue may push this into two sessions.

## Notes

The user said: "Yes, idempotency and stable structural hash have to be the
base contract — so we have to work that out as part of this complex of plans.
Everything existing must be verified to have those properties." This plan
encodes that contract as a CI-enforced test.

The hash function excluding source_info means that future plans (4-8) that
change source_info don't risk breaking idempotence — even if a transform
produces different source_info on different runs (e.g., a Sectionize that
generates synthetic source_info from current timestamps; not what we do, but
illustrative), the hash stays stable.
