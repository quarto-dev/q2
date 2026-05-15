# Plan 7a — Runtime user-filter idempotence check + opt-out

**Date:** 2026-05-05
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** none directly — extends M3 with an opt-in safety check;
  doesn't block the milestone

## Goal

Detect when a user's Lua filter chain breaks q2-preview's round-trip
contract — i.e., when running the pipeline, serializing the AST back to
qmd, and re-running the pipeline produces a different AST. Surface the
detection as a `Q-3-44` warning identifying the offending filter when
possible. Provide an `idempotent: false` opt-out per filter for cases
where the user knows their filter is intentionally non-idempotent
(timestamps, counters, randomized output) and accepts the round-trip
implications. Opt-outs surface as `Q-3-45` info diagnostics with
extension-vs-user-config attribution so users see the cause of the
exemption.

This is a separable follow-up to Plan 7. Plan 7 ships the writer with
soft-drop semantics; the writer round-trips correctly **when filters
are idempotent**. Plan 7a adds the check that detects when this
assumption is violated and gives users a way to declare their filter
exempt rather than silently breaking round-trip.

## Two flavors of non-idempotence — naming what we mean

The word "non-idempotent" gets used loosely. In Plan 7a it means
specifically:

- **Pipeline non-determinism**: `pipeline(x)` produces different output
  on repeat calls (filter uses time / RNG / mutable state). Plan 3's
  CI test already catches this: `run_pipeline(fixture)` twice on the
  same source, hash-compare, asserts equal.
- **Round-trip non-idempotence**: `pipeline(write(pipeline(source)))`
  ≠ `pipeline(source)` (filter satisfies `f(f(x)) ≠ f(x)` even when
  `f` is deterministic — e.g., `f(x) = x + "!"`). This is what
  actually breaks q2-preview's writer round-trip, and **Plan 3's
  current test does not catch it**.

Plan 7a's runtime check targets **round-trip non-idempotence** —
parse, pipeline, write, parse, pipeline, hash-compare. See §"Plan 3
strengthening" below.

## Scope

### In scope

- **`FilterMetadata` + `FilterSource`** types in `quarto-core` wrapping
  the existing `pampa::unified_filter::FilterSpec` with extra context:
  ```rust
  pub struct FilterMetadata {
      pub spec: FilterSpec,
      pub idempotent: bool,           // default: true
      pub source: FilterSource,
  }

  pub enum FilterSource {
      UserConfig { config_path: PathBuf },
      Extension { name: String },
  }
  ```
  `FilterSpec` (in `pampa`) stays unchanged — the executor reads only
  it. `FilterMetadata` is the resolver-level shape that carries the
  metadata cache-key construction and diagnostic emission consume.
- **`resolve_filters()` returns `Vec<FilterMetadata>`** instead of
  `Vec<FilterSpec>`. Plumbs source and idempotent flag through from
  `parse_filter_item` (user config) and `expand_extension` (extension
  contribution).
- **`parse_filter_item` extension**: recognizes `idempotent: false`
  alongside the existing `path` / `type` / `at` fields on the map form:
  ```yaml
  filters:
    - { path: timestamp.lua, idempotent: false }
  ```
  String form (`- foo.lua`) defaults to `idempotent: true`. Default
  carries through extension-contributed filters too unless the
  extension's contribution declares otherwise.
- **`filter_sources_hash`**: SHA-256 over each filter file's bytes
  concatenated with a sentinel, sorted by path. Includes the
  `idempotent` flag so toggling it invalidates the cache. Added to
  `Pass1KeyInputs` (`crates/quarto-core/src/cache_key.rs`); flows
  through profile-cache key derivation.
- **Round-trip idempotence check** runs once per document per session,
  cached in IndexedDB-backed profile cache:
  1. Run pipeline on source → AST_1.
  2. Serialize AST_1 via the qmd writer → qmd_1.
  3. Run pipeline on qmd_1 → AST_2.
  4. Compare `compute_blocks_hash_fresh(&AST_1.blocks)` vs
     `compute_blocks_hash_fresh(&AST_2.blocks)`.
- **Per-filter attribution**: when the whole-pipeline check fails, run
  the same round-trip with each filter active in isolation (others
  stubbed). Filters whose isolated round-trip fails are named in the
  Q-3-44 diagnostic. Filter chains are typically 2-5; cost is bounded.
- **`Q-3-44` diagnostic** (Warning severity), registered in
  `crates/quarto-error-reporting/src/error_catalog.json`:
  - Title: `Filter <path> is not idempotent`
  - Problem: `Edits may cause unintended changes elsewhere in the document.`
  - Hint: `Fix the filter to produce stable output, or add idempotent: false to its config in _quarto.yml to silence this check.`
  - Location: filter file path; no document-side range (the warning
    is about the filter, not a place in the active doc).
- **`Q-3-45` diagnostic** (Info severity), three-variant body:
  - Title (all variants): `Filter <path> exempted from idempotence checking`
  - Problem (UserConfig source): `idempotent: false set in <config_path>. Edits may cause unintended changes elsewhere in the document.`
  - Problem (Extension source): `Extension <ext-name> declares this filter non-idempotent. Edits may cause unintended changes elsewhere in the document.`
  - Problem (Unknown source — defensive): `This filter is exempt from idempotence checking. Edits may cause unintended changes elsewhere in the document.`
- **Once-per-session caching**: the verdict (pass / fail-with-attribution
  / opted-out) is cached on `(filter_sources_hash, document_path)`. Cache
  miss on filter source change, opt-out flag toggle, document path
  change.

### Out of scope

- **Strengthening Plan 3's CI test to round-trip flavor**. Plan 3
  currently catches non-determinism (`pipeline(x)` twice); the
  round-trip flavor (`pipeline(write(pipeline(x)))`) catches a
  different class of bug. See §"Plan 3 strengthening" below — flagged
  as a Plan 3 amendment, not Plan 7a.
- **File watchers for filter sources**. Demand-driven invalidation via
  `filter_sources_hash` on next render is sufficient. The user edits
  a filter, opens the document, hash mismatches, check re-runs.
- **Multi-filter interaction analysis**. Per-filter attribution
  identifies filters whose *isolated* round-trip fails. It does not
  catch cases where filter A's output is non-idempotent only when
  filter B has run first. Noted as a follow-up if reports surface.
- **Background / async execution of the check**. Initial implementation
  runs the check synchronously on first edit of a session. A slow
  filter would block the first save by O(filter_count) pipeline
  passes. Acceptable for v1; revisit if reports come in.
- **Idempotence checks on built-in filters at runtime**. Plan 3's CI
  test (after the strengthening noted above) is the right place for
  built-ins. Runtime check is for user-supplied filters specifically.

## Design decisions

- **Round-trip flavor, not pipeline-determinism flavor**. The runtime
  check serializes the first pass's AST through the qmd writer and
  re-parses, mirroring the actual round-trip the writer performs.
  Pipeline determinism is a weaker property; we get that for free
  from Plan 3's CI test (after strengthening).
- **Cache verdict per session, persisted in IndexedDB**. The cache
  key includes `filter_sources_hash` (filter file bytes + opt-out
  flags). Surviving session boundaries is correct: if filter sources
  haven't changed, the verdict hasn't either. IndexedDB is already
  used for the profile cache; piggyback on the existing namespace.
- **Filter source attribution preserves social context**. When an
  extension declares its filter non-idempotent via
  `contributes.filters: - { path: ..., idempotent: false }`, the
  Q-3-45 diagnostic names the extension, not the user. Avoids
  blaming users for choices made on their behalf.
- **Severity choice: Q-3-44 Warning, Q-3-45 Info**. Q-3-44 prompts
  action ("fix or opt out"). Q-3-45 is informational disclosure
  ("FYI, this is happening"). Visual distinction (amber vs. blue)
  matches the action gradient.
- **Suppress-after-N is not needed for Q-3-44 or Q-3-45**. Both fire
  at most once per filter per session because the check result is
  cached. Unlike Q-3-42/Q-3-43 which can re-fire on every debounced
  render as the user keeps typing.
- **Opt-out wording: `idempotent: false`**. Reads parallel to other
  YAML config flags (`toc: false`, `embed-resources: true`). The
  semantic meaning ("don't check me") is implicit, made explicit
  by the Q-3-45 diagnostic message.

## The check, structurally

```rust
fn check_filter_idempotence(
    doc: &DocumentInfo,
    filters: &[FilterMetadata],
    cache: &mut SessionCache,
) -> Vec<DiagnosticMessage> {
    let cache_key = (filter_sources_hash(filters), doc.input.clone());
    if let Some(verdict) = cache.get(&cache_key) {
        return verdict.clone();
    }

    let mut diagnostics = Vec::new();

    // Q-3-45 info for opted-out filters
    for f in filters.iter().filter(|f| !f.idempotent) {
        diagnostics.push(q3_45_for(f));
    }

    // Active set: filters that haven't opted out
    let active: Vec<_> = filters.iter().filter(|f| f.idempotent).collect();
    if active.is_empty() {
        cache.insert(cache_key, diagnostics.clone());
        return diagnostics;
    }

    // Round-trip check on active set
    let ast_1 = run_q2_preview(&doc.source, &active);
    let qmd_1 = qmd_write_to_string(&ast_1);
    let ast_2 = run_q2_preview(&qmd_1, &active);

    if compute_blocks_hash_fresh(&ast_1.blocks)
        == compute_blocks_hash_fresh(&ast_2.blocks)
    {
        // Idempotent — no Q-3-44 diagnostics
        cache.insert(cache_key, diagnostics.clone());
        return diagnostics;
    }

    // Non-idempotent — attribute to specific filter(s)
    let culprits = attribute_non_idempotence(&doc.source, &active);
    for culprit in culprits {
        diagnostics.push(q3_44_for(culprit));
    }
    cache.insert(cache_key, diagnostics.clone());
    diagnostics
}

fn attribute_non_idempotence<'a>(
    source: &str,
    filters: &'a [&FilterMetadata],
) -> Vec<&'a FilterMetadata> {
    let mut culprits = Vec::new();
    for filter in filters {
        let single = std::slice::from_ref(*filter);
        let ast_1 = run_q2_preview(source, single);
        let qmd_1 = qmd_write_to_string(&ast_1);
        let ast_2 = run_q2_preview(&qmd_1, single);
        if compute_blocks_hash_fresh(&ast_1.blocks)
            != compute_blocks_hash_fresh(&ast_2.blocks)
        {
            culprits.push(*filter);
        }
    }
    culprits
}
```

Total cost when an issue is detected: 2 + 2N pipeline runs (one
whole-set check, two per filter for attribution). For 5 filters,
~12 runs. Bounded; acceptable on first edit per session, cached after.

## Plan 3 strengthening (out of scope here, flagged for the team)

Plan 3's idempotence test (currently `run_pipeline(fixture)` twice on
the same source) catches **pipeline non-determinism** — filters that
use time, RNG, or mutable state. It does **not** catch
**round-trip non-idempotence** — the `f(f(x)) ≠ f(x)` case where the
filter is deterministic but produces different output when re-applied
to its own output through the qmd writer.

Plan 7a's runtime check targets round-trip non-idempotence, which is
the property that actually matters for q2-preview's writer. The Plan 3
CI test should be strengthened to also run this flavor, so that
built-in filters are CI-verified for both:

```rust
// Existing Plan 3 test: pipeline non-determinism
let ast_1 = run_pipeline(fixture, ...);
let ast_2 = run_pipeline(fixture, ...);
assert_eq!(blocks_hash(&ast_1), blocks_hash(&ast_2));

// New: round-trip idempotence (the property that breaks q2-preview)
let ast_a = run_pipeline(fixture, ...);
let qmd_a = qmd_write_to_string(&ast_a);
let ast_b = run_pipeline(&qmd_a, ...);
assert_eq!(blocks_hash(&ast_a), blocks_hash(&ast_b));
```

This change is small (~30 lines) and lands in Plan 3's test file.
**Recommend adding to Plan 3's scope as an amendment** rather than to
Plan 7a, since it concerns built-in filter coverage at CI time, not
runtime behavior.

## Open questions for implementation

- **Cross-session cache validity**: the profile cache persists. Should
  the idempotence verdict survive q2 binary upgrades? If the pipeline
  itself changes (filter execution, transform set), a previously
  "idempotent" verdict could be wrong after an upgrade. Mitigation:
  include a pipeline-version fingerprint in the cache key. Confirm
  during implementation whether this is needed for v1 or can defer.
- **Per-filter timeout for attribution**: a slow filter (e.g., one
  that calls out to a network) would slow attribution N-fold. Add a
  short timeout (e.g., 5s per attribution-run)? On timeout, emit a
  generic "filter `<path>` could not be checked for idempotence"
  diagnostic at Info severity. Defer to v2 unless filter chains in
  practice include slow filters.
- **Multi-filter interaction**: filter A is idempotent alone, filter B
  is idempotent alone, but A then B is not. Per-filter attribution
  doesn't catch this. The whole-set check catches it but reports no
  specific filter. In that case the diagnostic would say "filter chain
  is not idempotent (no single filter could be attributed)" — confirm
  the wording.
- **`Unknown` `FilterSource` variant**: in practice every filter goes
  through `resolve_filters` which knows its source. The `Unknown`
  variant is defensive. Confirm during implementation we can drop it
  (use `unreachable!()` instead of a third match arm) — or keep as
  defensive insurance.
- **Cache eviction on filter source change**: when the user edits a
  filter file, the next render must re-run the check. The cache key
  includes filter file bytes via `filter_sources_hash`, so this is
  automatic. Confirm there's no stale-cache failure mode where the
  filter source changes but the hash doesn't (e.g., bytewise-identical
  edit-and-revert sequences).
- **Diagnostic delivery in hub-client**: the existing diagnostic
  pipeline routes through `RenderResponse.warnings`. Q-3-44 / Q-3-45
  flow through the same path. Confirm they reach the diagnostic panel
  and are visually distinguishable from pipeline warnings (or
  acceptably co-mingled — TBD by hub-client UX, same as Q-3-42/Q-3-43).

## References

- `crates/quarto-core/src/filter_resolve.rs` — current filter resolution;
  the `parse_filter_item` function (line ~210) is where the
  `idempotent` field gets parsed; the `resolve_filters` function (line
  ~73) is where the return type changes to `Vec<FilterMetadata>`.
- `crates/quarto-core/src/cache_key.rs` (location TBD; the
  `Pass1KeyInputs` struct and `pass1_key()` function) — extend with
  `filter_sources_hash`.
- `crates/quarto-ast-reconcile/src/hash.rs::compute_blocks_hash_fresh` —
  used to compare `AST_1` and `AST_2` (excludes `source_info`, so
  source-info changes don't trigger false positives).
- `crates/quarto-error-reporting/src/error_catalog.json` — add Q-3-44
  and Q-3-45 entries with their titles, message templates, severities.
- `pampa::unified_filter::FilterSpec` — wrapped by `FilterMetadata`;
  stays unchanged.
- Plan 7 — the q2-preview pipeline + qmd writer this check supports.
  The check uses Plan 7's `pipeline_kind: Some("preview")` machinery
  for both passes.
- Plan 3 — CI-time idempotence verification for built-in filters.
  Plan 3 strengthening (above) extends the test to round-trip flavor.
- Plan 4 — `By` types; `is_atomic_synthesizer()` is unrelated to this
  plan but the runtime check shares the source-info-blind hash.

## Test plan

- **`FilterMetadata` parsing tests**: string form (`- foo.lua` →
  `idempotent: true, source: UserConfig`); map form with
  `idempotent: false` → flag set; extension contribution → source =
  `Extension { name }`.
- **`filter_sources_hash` tests**: same filter sources produce same
  hash; toggling `idempotent` flag changes the hash; reordering
  filters in `_quarto.yml` produces same hash (sort-by-path).
- **Cache miss / hit tests**: first call computes verdict; second call
  same session returns cached result; modifying filter source bytes
  triggers cache miss.
- **Idempotent-filter pass test**: doc with a deterministic, round-trip-
  idempotent filter (e.g., uppercase). Assert the check returns no
  diagnostics, verdict is cached.
- **Non-idempotent-filter detection test**: doc with `f(x) = x + "!"`.
  Assert Q-3-44 fires with the filter's path. Assert it fires once,
  not on every render.
- **Per-filter attribution test**: doc with three filters, only one
  non-idempotent. Assert Q-3-44 names the specific filter, not all
  three.
- **Whole-set non-idempotence (no specific attribution) test**: filter
  A and filter B both pass in isolation but fail together. Assert
  Q-3-44 fires with "filter chain" wording, not a specific filter.
- **Opt-out tests**:
  - Filter declared `idempotent: false` in user config: Q-3-45 (Info)
    fires with `UserConfig` source attribution; Q-3-44 does not fire
    even when filter is non-idempotent.
  - Extension contributes filter with `idempotent: false`: Q-3-45
    (Info) fires with `Extension { name }` source attribution.
  - Multiple opted-out filters: Q-3-45 fires once per filter.
- **Diagnostic content tests**: Q-3-44 problem text matches the spec
  ("Edits may cause unintended changes elsewhere in the document.");
  Q-3-45 variants match their respective bodies; hint text mentions
  the opt-out path.

## Dependencies

- **Depends on**: Plan 7 (the q2-preview transform pipeline + qmd writer
  + `pipeline_kind: Some("preview")` parameter; the check uses these).
- **Soft-depends on**: Plan 4 (the `By` types; the hash function
  excludes `source_info` regardless, but test fixtures may use
  `Synthetic`/`Derived` content for realism).
- **Blocks**: nothing structurally; this is a reliability improvement,
  not a milestone deliverable.
- **Related**: Plan 3 (CI-time test for built-in filters) — would
  benefit from the strengthening proposed in §"Plan 3 strengthening"
  above.

## Risk areas

- **Performance on first edit per session**: the check runs the
  pipeline twice (whole-set) plus 2N times (per-filter attribution
  if non-idempotent). Filter chains are typically 2-5 filters, so
  worst case ~12 pipeline runs. Mitigation: results cached for the
  rest of the session; subsequent edits have zero additional cost.
  If cost proves too high, the check can become opt-in (a config
  flag enables it) or background-async.
- **False positives from non-determinism**: if a built-in filter is
  non-deterministic (Plan 3's existing test should catch this; if it
  doesn't, the bug is elsewhere), our round-trip check would fire
  Q-3-44 spuriously. Pre-condition: Plan 3's test passes.
- **False positives from `source_info` reaching the hash**: if
  `compute_blocks_hash_fresh` accidentally hashes `source_info`,
  every round-trip would look non-idempotent (because source_info
  legitimately differs across runs). Plan 7's foundation test
  asserts this doesn't happen — keep that test running.
- **Cross-session staleness on q2 upgrade**: see open question on
  pipeline-version fingerprint in cache key.
- **Filter chains with intentional state**: timestamps, counters,
  randomized output. The opt-out is the user's recourse. If
  commonly-deployed extensions ship non-idempotent filters without
  declaring them, users will see Q-3-44 spam — pre-emptively work
  with extension authors to declare their filters.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `FilterMetadata` + `FilterSource` types | ~30 |
| `resolve_filters()` extended return + parsing | ~50 |
| `filter_sources_hash` + `Pass1KeyInputs` extension | ~40 |
| Round-trip check (whole-set pass) | ~80 |
| Per-filter attribution pass | ~60 |
| Q-3-44 / Q-3-45 catalog entries + builders | ~50 |
| Session cache integration | ~40 |
| Tests (unit + integration) | ~250 |
| **Total** | **~600** |

Single focused session. Risk: per-filter attribution may surface
unexpected interactions; budget a second session if attribution proves
trickier than the design allows for.

## Notes

This plan was extracted from Plan 7's open-questions section. Splitting
it out keeps Plan 7 focused on the writer's coarsen + soft-drop logic
(the M3 deliverable) and makes Plan 7a a separable PR that doesn't
gate the milestone.

The check is targeted at user-supplied Lua filters. Built-in filters
that ship with Quarto are covered by Plan 3 (CI-time, with the
strengthening proposed above). User filters can't be statically
analyzed for idempotence (uncomputable for arbitrary Lua), so the
runtime check via double-pass-and-hash is the available mechanism.

The opt-out (`idempotent: false`) gives users intentional escape — a
timestamp-emitting filter can declare itself non-idempotent and silence
Q-3-44, while still showing Q-3-45 for awareness. The Q-3-45 message's
effect language ("Edits may cause unintended changes elsewhere in the
document") matches Q-3-44's so the trade-off is visible regardless of
how the exemption was declared.

Filter source attribution (`UserConfig` vs. `Extension`) preserves
social context. An extension that contributes a non-idempotent filter
and declares it via `idempotent: false` produces a Q-3-45 message that
names the extension. The user shouldn't feel blamed for choices made
on their behalf.
