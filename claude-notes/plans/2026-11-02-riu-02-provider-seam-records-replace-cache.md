---
title: 'RIU-2: a provider seam; records replace the capture cache'
date: 2026-11-02
description: 'Factor one publish-a-capture-run function out of the preview and provide-hub copies (it refuses to publish an empty legacy list), split the cache wrapper so an ordered source list can consult the cache without running the engine, make a record-only entry valid everywhere the preview path assumes a legacy capture (six places and one test assertion, per spike S2 and the plan review), keep the cache where it is with no pointer, and read the .quarto copy as a record source outside the preview.engine off guard.'
status: ready  # nothing started; written from RIU-0 S1, S2 and S4 and checked against the tree on 2026-11-03; revised after the second plan review (gate, cache files, re-execute, source-list home) and the fourth (resolver trait and per-source outcomes, the execution guard moved here)
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Evidence:** [RIU-0 findings](../research/2026-11-03-riu-0-spikes-findings.md), S1 (B3, B7, B8, B11),
S2 (record-only entry, pointer), S3 (consent), S4 (engineless records);
[the second plan review](../research/2026-11-03-riu-plans-review-2.md); [RIU-0c](../research/2026-11-03-riu-0c-findings.md)
spike 4 (the signatures below were type-checked on a scratch branch).
**Depends on:** nothing in code; RIU-1 can overlap. The spike branch `spike/riu-s2-record-only`
(commits `861176227`, `ce3961571`) is scratch and never merged; use it as a reference for the
change, not as a base.
**Blocks:** RIU-3 (sources plug into the walk), RIU-3a (adds the minimal `--records` parse of the list this plan defines), RIU-4 (hardens it and implements the resolver for the provider),
and the release of the RIU-1 flip.
**Estimated sessions:** 4 to 5 (an estimate after the fifth review: the record-only part is small, S2, but the cross-crate publish split, the cache split, the `ReExecuteConfig` churn (about 40 call sites), the resolver with its outcomes and refusal predicate, the guard and the browser run all land here).

> **For implementers:** tests first. The record-only change is small and proven (S2): the
> survey predicted many failing tests and found one assertion.

## Overview

Preview and hub work from a `CaptureRun` that carries legacy captures and, when the gate is on
and the engine built one, a record. Several places treat "no legacy captures" as "nothing to
show", so a record alone is not enough. This plan makes the record sufficient, puts the two
copies of the publish code behind one function whose input can come from any record source,
defines the ordered source list and splits the cache wrapper so the list can consult the cache
without running the engine, and reads the `.quarto` copy as a source.

Settled (epic decisions 1, 3, 4, 8, 9): records substitute for the capture cache whether or not
`record: true` is set; reuse is existing behavior and this plan adds no reuse concept; sources
are an ordered list, first wins, no merging, one `CaptureRef` points at one record; the cache
stays in `<data_dir>/captures/` and there is **no path-keyed pointer** (S2b: it is needed only if
the cache moves; with D1 fixed the `CaptureRef` alone reuses a record across a restart); the
writer gate decides only the form of the hub payload, never what the cache holds.

## What a record-only entry requires

The six places, all assuming a legacy capture (S2 found the first five; the plan review added the
sixth):

1. `record_capture_run_cached` (`crates/quarto-preview/src/cache.rs:230`): **the cache stores
   what the run has, and the gate is never read in `cache.rs`.** A `<key>.bin` is written when the
   run has captures; the record file is written when the record describes something; a hit is
   either file readable (the capture list is empty when only the record is). An engine run always
   has captures, so it writes both; an imported record has none, so it writes only the record.
2. `record_one` (`capture_driver.rs:172`): the guard `!run.captures.is_empty()` (`:198`) becomes
   the new predicate; the `engines=` log line reads the record engine names when there are no
   captures.
3. `perform_re_execute` (`re_execute.rs:307`): same guard.
4. `run_and_store` (`quarto-hub-provider/src/execute.rs:338`): same guard.
5. The predicate itself, **defined here**: `Record::describes_something()` (engines **or** cells
   non-empty) in `crates/quarto-core/src/record/types.rs`. It does not exist in the tree today (the S4
   spike branch has one); `run_has_content` is true when captures are non-empty or the record
   describes something, so RIU-3 engineless records are not treated as "no record".
6. `encode_capture_payload` (`capture_files.rs:303`): `!r.metadata.quarto_engines.is_empty()` becomes
   `r.describes_something()`. Without this an engineless record goes out as an empty legacy list even
   with the gate on. The rule has exactly two inline copies in the tree, this one and the write filter
   at `cache.rs:285` (item 1); no other site tests `quarto_engines.is_empty()`. The existing test
   `gate_on_with_an_engineless_record_falls_back_to_legacy` (`capture_files.rs:442`) stays as it is: it
   clears both the engines and the cells, so the record describes nothing and still falls back to the
   legacy array under the new predicate. It is not inverted. The new case is the opposite one, an
   engineless record **with cells** (what RIU-3 imports), added as a separate test below. This extends EXR-15's
   rule (a record that describes nothing never replaces the legacy captures) and does not reverse it
   (epic, decisions of 2026-11-06). **The same function carries a second rule, owned by RIU-1:** a record with
   engines is stored as a record only if every capture's engine is among them; otherwise the whole document is
   stored in the legacy format and the names of the engines without records are returned for a diagnostic. The
   `describes_something()` change must keep it. An imported record has no captures, so it is never affected.
   `encode_for_publish` applies the same helper through the publish seam, so the two cannot drift.

Plus the one test assertion `cached_wrapper_stores_and_returns_the_record` (`cache.rs:740`,
`second.captures.len() == first.captures.len()`). A hit that finds the `.bin` still returns its
captures, so this may not need to change; confirm when building it. A hit on a record file alone has no
captures by construction.

**The gate and a record with no legacy form (epic decision 9).** The gate decides only the form of
the hub payload. When the gate is off and a run has a record that describes something but **no
captures**, the legacy form is an empty list and the page would show nothing, silently. The publish
function (Phase B) therefore **refuses** in that case, with an error that names
`QUARTO_ENGINE_RECORD_WRITE` and says the source needs the record writer on. Gate-off engine runs are
unaffected: they have captures, so the legacy form is real. For local preview the RIU-1 flip makes the
gate on by default; for `provide-hub` the flip waits for the hosted hub, so a Positron source there
needs the environment variable until then.

Nothing in `recompute_staleness` changes for an engine-run record: it takes the Record arm
(`capture_driver.rs:324`), and its Legacy arm is reached only if the hub doc itself is legacy.
(An **imported** record in that arm is RIU-3's guard.) Nothing in the WASM or hub-client read path
changes (`parse_capture_payload` dispatches on payload shape).

### What does not change in the cache file code

The record file keeps its name and form (`<key>.record.bin`, gzipped; epic decision 8), so
`record_file_path` (`cache.rs:108`), `read_cached_record` (`:144`), `write_gzipped_atomic` (`:165`)
and their tests (`record_round_trips_through_the_cache`, `a_missing_or_corrupt_record_file_reads_as_no_record`,
`concurrent_record_writers_leave_a_readable_entry`) are **untouched**. What changes is the wrapper
around them: its hit condition, its store condition, and the split below.

## Decisions that shape this plan

From the epic's "Decisions recorded 2026-11-05", with the answers to the fourth and fifth reviews
([review 4](../research/2026-11-05-riu-plans-review-4.md), [review 5](../research/2026-11-05-riu-plans-review-5.md)).

- [x] **The resolver and the list walk.** `quarto-core` owns, in `record/sources.rs`, the list type, the
      first-wins walk, the refusal predicate and a per-source outcome record. A two-method trait covers only
      what depends on who is asking; the preview (this plan) and the provider (RIU-4) each implement it. Both
      methods take `&SourceInputs`, so the pre-engine pipeline runs once per document and the cache key derives
      from `inputs.input_qmd`. `Positron` and `Dir` need no trait method, because neither depends on the
      caller; RIU-3a supplies their arms inside the walk, in its native-only module. `SourceInputs` gains an
      `ownership` field in RIU-3 Phase B, filled only when the caller asks for it (additive; this plan never
      reads it).
- [x] **What `run_engine` returns, and why.** The preview publishes a run itself, through the seam below; the
      provider cannot, because its engine step is `run_and_store` (`execute.rs:272`), which runs consent, the
      engine, the capture document and the sidecar in one body and errors on an empty capture list (`:338`).
      So `run_engine` returns an `EngineResult`: `Run(CaptureRun)` for a caller that publishes (the preview),
      or `Published` for one that already did (the provider). The walk reports which in `Chosen`. The trait
      stays small and the provider's consent and temp-project flow are untouched.
- [x] **The walk's inputs and result.** The walk takes a small context (the list, the engine policy, the writer
      gate), the `&SourceInputs` and `&dyn RecordLookup`; it needs the gate and the policy for the refusal check
      and for the `Unavailable` outcome. Its error types (`LookupError`, `EngineError`) are string-backed
      enums defined beside it. Every name in the code block below is illustrative; the shape is settled.
- [x] **Outcomes.** The walk returns the chosen result, if any, and one outcome per source tried: supplied,
      nothing, refused (with the reason) or unavailable (`engine` absent from the list, or the policy is
      `off`). The callers report them; this plan only produces them (RIU-4 builds the "no source supplies"
      diagnostic from them).
- [x] **One refusal predicate.** A small pure function in `quarto-core` takes the gate, the captures and the
      record, and says whether the run has no form the gate-off writer can publish: a record that describes
      something, no captures, and the gate off. Both `encode_for_publish` (Phase B) and the walk call it and
      nothing else encodes the rule, so the two cannot drift. It replaces the inline filter at
      `capture_files.rs:303` as the single statement of the rule. Where the refusal surfaces: the walk records a
      "refused" outcome and falls through; if the refused source was the last that could supply, the caller
      maps that outcome to `RecordError::Refused` (the preview) or `ProviderError::Protocol` (the provider),
      with the same text naming `QUARTO_ENGINE_RECORD_WRITE`. `PublishError::RecordWriterOff` is what
      `encode_for_publish` returns on the same predicate when a caller publishes without having walked (the
      export and tests).
- [x] **Fall-through.** A refused source is an outcome, and the walk tries the next one. In the preview,
      `record_one` emits a page warning through `diagnostics::current_sink()` when it fell through to a later
      source that did supply a record; in `provide-hub`, RIU-4 writes the same text to stderr. Test both the
      fall-through and the standing error.
- [x] **Imported records are never written to the cache** (the cache holds engine-run results only).
      Test: "Re-execute on a document with only an imported record runs the engine".
- [x] **Staleness at publish.** `record_one` stops hard-coding `staleness: Some(false)`
      (`capture_driver.rs:206`). The staleness comes from the source that supplied the record: fresh by
      construction for `Cache` and `Engine`, checked when chosen for `Q2` (RIU-4), and the one function of
      RIU-3 Phase B for an imported record.
- [x] **The execution guard and the `axum::Extension` move here** (Phase B3), not to RIU-4, so the guard
      exists before any `--records` flag does.

**Implementation notes (2026-11-06, `record/sources.rs`).** Two details differ from the illustrative
code above, both forced by crate boundaries and neither a change of decision. `WalkContext` carries
`engine_enabled: bool` in place of `policy: EnginePolicy`, because `EnginePolicy` lives in
`quarto-preview` and `quarto-core` cannot name it; the caller passes `policy != Off`. `OutcomeKind`
gains a fifth kind, `Failed(String)`, for an unreadable source or an engine error, so a failure is
reported and stepped past instead of being mistaken for "nothing"; an `engine` missing from the list
adds an `Unavailable` entry at the end of the outcomes. `Resolution` has `first_failure()` and
`refusal()` for the caller to map to its error type. `CaptureRun::has_content()` (in
`preview_record.rs`) is the "no captures is not nothing" predicate the three caller guards use.

The trait and the walk, with illustrative names:

```rust
pub enum EngineResult {
    Run(CaptureRun),   // the caller publishes it through the seam (the preview)
    Published,         // the implementation already published it (the provider)
}

#[async_trait(?Send)]
pub trait RecordLookup {
    /// The `Cache` and `Q2` sources.
    async fn lookup(&self, source: &RecordSource, inputs: &SourceInputs)
        -> Result<Option<CaptureRun>, LookupError>;
    /// The `Engine` source: run it now.
    async fn run_engine(&self, inputs: &SourceInputs) -> Result<EngineResult, EngineError>;
}

pub struct WalkContext<'a> {
    pub list: &'a RecordSources,
    pub policy: EnginePolicy,
    pub record_gate: bool,
}

pub enum Chosen {
    Run { source: RecordSource, run: CaptureRun },
    EnginePublished,
}
pub struct Resolution {
    pub chosen: Option<Chosen>,
    pub outcomes: Vec<SourceOutcome>,
}

pub async fn resolve_records(
    ctx: &WalkContext<'_>,
    inputs: &SourceInputs,
    lookup: &dyn RecordLookup,
) -> Resolution;
```

## Phase A: tests first

The writer gate is read from the environment in exactly two places: `write_capture_doc` in
`capture_driver.rs` and `run_and_store` in the provider (`execute.rs:361`). `write_capture_doc_gated`
already takes the gate as an argument, and it is the test seam: a test of the encoder, the refusal or
the publish function passes `true` or `false` and needs nothing new. A test that goes through
`record_eager_captures` or `perform_re_execute` (the restart and record-only tests below) cannot set an
environment variable safely (`set_var` is `unsafe` in edition 2024 and process-wide), so those paths take
the gate from a **`ReExecuteConfig`** that this plan introduces (spike 4: it compiles):
`{ policy, sources, record_gate, cache_dir }`, built once in `lib.rs` where `engine_policy` and `cache_dir`
are built today, with `record_gate` read from the environment there and nowhere else. It replaces the
`(policy, cache_dir)` parameters of `record_eager_captures` and `recompute_staleness`, this plan adds the
re-execute guard to it (Phase B3), RIU-3a adds the minimal `--records` parsing and RIU-4 hardens it. The provider stores the gate it reads at construction
(RIU-4 gives `Provider::new` and `new_with_overlay` the parameter).
`PreviewConfig` gains a `record_sources` field. The churn is mechanical but large: 20 call sites of
`record_eager_captures` (2 production, 18 in tests), about 17 of `recompute_staleness`, and every
`PreviewConfig { .. }` literal in the integration tests (18); give `PreviewConfig` a constructor or
`Default` for the test literals rather than editing each.

- [x] A cache test in `cache.rs` (`mod tests`; helpers `write_fixture`, `counting_recording_registry`):
      a hand-built `CaptureRun` with a record and **no captures** goes through `store_run`, a
      second `lookup_cached_run` hits, and the engine does not run. (An engine run cannot produce
      this: it always has captures. Only an import can.) A second test: an engine run through
      `record_capture_run_cached` writes both the `.bin` and the record file.
- [x] Confirm or update `cached_wrapper_stores_and_returns_the_record` (see above).
- [x] A test that an unreadable record with no legacy file (corrupt bytes, and
      `quarto_record_version` 99) is a **miss with a warning that re-runs the engine**, not a
      panic, and that with the legacy `.bin` present it still falls back to the captures. S1 B11
      survived those cases only because `.bin` still existed.
- [x] Confirm the tests that assert empty-captures behavior (`cached_wrapper_returns_empty_for_prose_only_doc`,
      `prose_only_doc_records_no_capture`, `md_doc_with_engine_spec_records_no_capture`,
      `recompute_staleness_noop_when_no_capture`, `prose_only_doc_leaves_sidecar_empty`) still
      pass. S2 found they use the passthrough engine and are unaffected; a prose-only document
      still records nothing under the new predicate. Change a predicate only if one fails.
- [x] Tests for `describes_something()` in `record/types.rs`: true with engines, true with cells and
      no engines, false for an empty record (build it by struct literal, or from a checked-in record
      fixture).
- [x] Encoder tests for `encode_capture_payload`: an engineless record with cells and the gate on
      encodes a **record** document; and the publish function with the gate off, a record that
      describes something and no captures **refuses**, with the environment-variable message, while a
      gate-off run that has captures publishes the legacy form.
- [x] Tests for the split cache wrapper: `lookup_cached_run(cache_dir, key)` returns the run on a hit
      and `None` on a miss (it takes no registry, so it cannot load an engine; the check is that it
      compiles without one and that the resolution test below never calls the engine fake);
      `store_run` writes what the old wrapper wrote; the composition behaves exactly as
      `record_capture_run_cached` did (the existing wrapper tests, unchanged, are the check).
- [x] A test of the walk with a fake `RecordLookup`: first wins, a source that supplies nothing falls
      through, no merging, and an absent `engine` means `run_engine` is never called. A source
      **supplies** a record only if it yields one that describes something and, for a source that
      matches by unit (Positron, a directory), at least one unit matches; a cache file with no
      matching unit falls through to the next source. The outcomes list one entry per source tried, with
      the right kind for supplied, nothing, refused and unavailable. A fake `run_engine` that returns
      `Published` makes the walk report `EnginePublished` and publish nothing itself. (RIU-3a adds the minimal parser and RIU-4 the validation;
      this plan owns the type and the walk.)
- [x] A unit test of the refusal predicate itself (gate on or off, with and without captures, with and
      without a record that describes something), and a test that `encode_for_publish` and the walk give the
      same answer for each combination.
- [x] A fall-through test: with the gate off, a record with no captures is refused at the publish check
      and the walk takes the next source (the outcome is "refused", naming `QUARTO_ENGINE_RECORD_WRITE`);
      with the refused record the last source that could supply, the error stands.
- [x] A test that `record_one` publishes a staleness computed from the supplying source, not a constant:
      a fake source that reports stale produces `staleness: Some(true)` in the sidecar.
- [x] A restart test pinning the reuse behavior S1 and S2 observed: with a persisted store, a
      prose edit while stopped reuses the record through the `CaptureRef` (no engine run, no
      stale flag), and a code edit while stopped shows stale (the D1 tests already cover the
      stale flag; add the prose half). Also a test that with **no** ref (a cache copied into a
      fresh `--data-dir`) a prose edit misses the content-keyed cache and runs the engine once.
      That second test documents S1 B3 so a later change to it is deliberate. It goes in
      `crates/quarto-preview/src/capture_driver.rs` tests (a deviation: the shared recording engine is `#[cfg(test)]`, in `src/test_support.rs`, so it cannot reach an integration binary) through
      `record_eager_captures(ctx, runtime, Some(registry), &cfg)` (a `ReExecuteConfig`) and a
      `HubContext` over a `StorageManager`; drop the first context before starting the second (the
      `hub.lock`). It needs a recording engine, and the one in `cache.rs` tests is private: share it
      from a test-support module rather than copying it.
- [x] A test for the `q2` source: a file at `.quarto/execution-records/<doc>.ipynb` supplies the
      record when the cache has none, with no engine run; and an unreadable file is skipped with a
      warning and the next source is tried. The path comes from the **same function the writer uses**
      (see Phase E), so the reader cannot drift.
- [x] A test that with `preview.engine: off`, a cached or `.quarto` record is shown and the
      engine is never run (see Phase E).
- [x] A test that `perform_re_execute` runs the engine and does **not** consult the other sources: a
      list with a source that would return a stale record still ends with the fresh engine record in
      the `CaptureRef` (see Phase B2).

## Phase B: the seam

- [x] (done: `record_has_no_legacy_form`, `PublishError`, `encode_for_publish` in `capture_files.rs`; `quarto_hub::resource::publish_payload_doc`; both `write_capture_doc` copies now call them, `RecordError::Refused` and the provider's `Protocol` carry the refusal) One publish path for a `CaptureRun` (encode, write the payload document), replacing the two
      copies of `write_capture_doc` (`quarto-preview/src/capture_driver.rs:396-441` and
      `quarto-hub-provider/src/execute.rs:530`). `quarto-hub` cannot depend on `quarto-core`, and both
      callers depend on both, so split it (spike 4: it compiles with no new dependency edge).
      `quarto-hub` gets `publish_payload_doc(repo, is_record, gzipped, meta)`, which builds the payload
      document and calls `repo.create`; that is the whole duplicated part, about ten lines.
      **`set_capture` is not part of it**: each caller updates its own sidecar (`record_one` in the
      preview). `quarto-core` gets `encode_for_publish(captures, record, gate)` beside
      `encode_capture_payload`, returning a `PublishError` (`Io`, or `RecordWriterOff` for the refusal
      above). It calls the one refusal predicate of the decisions above, extracted from the inline filter at
      `capture_files.rs:303`, and `encode_capture_payload` uses the predicate too, with
      `r.describes_something()` in place of the engines test. `RecordError` (in `quarto-preview`) gains a
      `Refused` variant and the provider maps the refusal to its `ProviderError::Protocol`; the mapping from
      the walk's "refused" outcome to those two is written once per caller, with the text naming
      `QUARTO_ENGINE_RECORD_WRITE`.
- [x] The input is a source-neutral value (a `CaptureRun`, or a record plus provenance), so
      RIU-3 adds a source without touching the publish code.
- [x] Preserve the intent of the three caller guards with the new predicate.

## Phase B2: the source list and the split cache wrapper

`record_capture_run_cached` fuses three steps: compute the content key (`compute_input_qmd`,
which runs the pre-engine pipeline), look the key up, and on a miss run the engine and store. An
ordered list that puts the cache before `q2` and the engine before nothing needs the steps apart.

- [x] Split it into `lookup_cached_run(cache_dir, key) -> Option<CaptureRun>` (the `cache` source),
      `store_run(cache_dir, key, &CaptureRun)` (what the miss arm writes) and the key computation,
      computed **once** per document and passed to both. Keep `record_capture_run_cached` as the
      composition (lookup, else `record_capture_run`, then store) so existing callers and tests
      are unchanged. Its production callers are `record_one` (`capture_driver.rs:188`) and
      `perform_re_execute` (`re_execute.rs:304`).
- [x] Define the list type, the walk and the trait in **`quarto-core`**, the one crate both
      `quarto-preview` and `quarto-hub-provider` depend on that can hold them (`quarto-hub` does not depend
      on `quarto-core`): an ordered `Vec` of `Cache`, `Q2`, `Positron`, `Dir(PathBuf)` and `Engine`, with its
      two defaults (`cache,q2,engine` for preview, `engine` for `provide-hub`) in one place, as a
      target-neutral module `record/sources.rs` (it compiled natively on the spike; the WASM build was not
      run). The trait is `#[async_trait(?Send)]` and must compile for wasm32, so run `npm run build:wasm`;
      `CaptureRun` and `SourceInputs` hold no non-wasm type (read in the tree, 2026-11-05) and the modules
      they live in are not cfg-gated. `quarto-core` already has `async-trait`; `quarto-preview` (this plan)
      and `quarto-hub-provider` (RIU-4) do not, so each gains `async-trait.workspace = true` where it
      implements the trait. The walk (signature in the decisions above) returns the first record a source
      supplies, or the engine's `EngineResult`, plus the outcomes, and runs the engine only when `Engine` is
      in the list and the policy allows it. `Positron` and `Dir` resolve to "nothing" until RIU-3a supplies
      their arms. The preview's implementation of the trait lives in `quarto-preview` (it reads the cache
      and the `.quarto` copy). RIU-3a parses the CLI option into this type and RIU-4 hardens it; neither
      adds a second type.
- [x] A run that the engine produces is stored in the cache whenever a cache directory exists
      (the preview), whether or not `Cache` is in the list, because storing is what makes the next
      lookup cheap; `provide-hub` has no cache directory and stores nothing (its Decision #4 is a
      fresh run, `execute.rs:332-335`). `Cache` in a `provide-hub` list is a start-up error that says so
      (RIU-4 raises it).
- [x] `record_one` calls the walk instead of `record_capture_run_cached`, publishes the run it returns through
      the seam, emits the fall-through warning, and publishes the supplying source's staleness (see the
      decisions above). Its `EngineResult` is always `Run` in the preview.
- [x] **`perform_re_execute` does not call the resolution.** Re-execute means "run the engine now": it
      runs the engine, stores the result in the cache and publishes it, so the document's
      `CaptureRef` points at the fresh engine record. Later staleness checks read the ref's record and
      do not re-resolve the list (the list is read once at start, epic decision 4), so a
      `positron,engine` list cannot bring the Positron record back over the fresh result.

## Phase B3: the execution guard and the `Extension`

Every path that starts a kernel for a preview goes through `claim_and_spawn` (`re_execute.rs:203`),
which has exactly two callers: the HTTP handler (`:144`) and `trigger_auto_re_execute` (`:180`). So
one guard there covers a share guest, who reaches the handler through the tunnel, and the automatic
re-execute from both of its entry points. Today the handler is open under `preview.engine: off` (S1 B8);
closing it is the epic's decision 5 and changes behavior, so say so in the commit message.

- [x] Replace the process-wide `CACHE_DIR` static (`re_execute.rs:53`) and `set_cache_dir` (`lib.rs:361`)
      with the `ReExecuteConfig { policy, sources, record_gate, cache_dir }` of Phase A, carried as an
      axum `Extension` layer beside the handler's `State`: `Some(move |r|
      extend_with_preview(r).layer(axum::Extension(cfg)))` satisfies the `FnOnce + Send` bound of
      `run_server_with` (spike 4). The seam `re_execute_with_registry_and_cache` (`:120`) takes the config
      too, and so do its four in-module tests (`:412`, `:437`, `:545`, `:607`), which a list without `engine` would otherwise refuse. The two test callers of the no-argument form (`render_download.rs:264`, `route_tests.rs:396`)
      keep it; it installs no config, so a test that reaches re-execute through it is refused.
- [x] **The handler fails closed:** with no `Extension<ReExecuteConfig>` it refuses; today it falls back to
      a temp directory when `CACHE_DIR` is unset (`:101-109`), which must not survive.
- [x] The guard in `claim_and_spawn`, placed **before** its in-flight insert (`re_execute.rs:211`; a refusal after it would leak the slot and every later POST for that path would answer 409): refuse unless the policy is not `off` **and** `engine` is in
      `sources`, and never start a kernel. It adds `ClaimOutcome::Refused`, which both callers match (the
      handler answers 403 with a message naming `preview.engine` or `--records`;
      `trigger_auto_re_execute` logs and returns). The file-watcher callback is an `Fn` closure, so it
      clones the `Arc` inside rather than moving it.
- [x] Tests first. `ReExecuteConfig` and the `Extension` do not exist today, so a test written against them
      cannot compile, let alone fail on behavior. Land the config type and the `Extension` as a compile stub
      first (no guard yet: it carries the policy, the list and the cache directory the code reads today), so
      that the tests below fail on behavior. Then, each watched failing against that stub: the POST under
      `off` is refused; under the default `manual` policy with a list that lacks `engine` the POST is
      refused; `auto` with a list that lacks `engine` starts no kernel after a staleness flip (through
      `trigger_auto_re_execute`); with `engine` in the list, `manual` and `auto` run as before; two handlers
      built with different configs in one process each follow their own (the old static made the first
      writer win); a request with no config is refused (this one fails on behavior even before the stub,
      because the handler falls back to a temp directory, `re_execute.rs:106`). The HTTP handler passes no
      registry (`re_execute.rs:110` calls the seam with `None`), so the handler tests assert the 403 and an
      unchanged sidecar, and the proof that no engine runs is a recording engine at the `claim_and_spawn`
      seam.
- [x] Update the doc comment at `diagnostics.rs:121`, which names the `CACHE_DIR` static this phase removes.
- [x] The share and CLI-level tests (a guest-triggered Re-execute on a `--share` host whose `--records`
      has no `engine` is refused) are RIU-4's, because they need the flag.

## Phase C: record-only entries

- [x] Make the six changes listed above, in that order, each with its test from Phase A.
- [x] The record cache file keeps its name and form (see above); the legacy `<key>.bin` is
      untouched (EXR-17 deletes it). Anyone who wants a Jupyter-openable record uses
      `.quarto/execution-records/`.

## Phase D: no pointer, and where reuse comes from

- [x] Do not add `by-path/` or any path-keyed pointer. Add a comment in `cache.rs` saying reuse
      across a restart comes from the hub `CaptureRef` in the persisted store, and that a pointer
      is the fallback if the cache is ever moved out of `data_dir` (S2b: about 85 lines, plus
      garbage collection that the prototype did not have).

## Phase E: the `q2` source and `preview.engine: off`

- [x] Read `.quarto/execution-records/<doc>.ipynb` as the `Q2` source, with `read_record`. A file
      that fails to read is skipped with a warning. It is **not** checked for staleness in this plan (RIU-4
      checks it when chosen, epic decision 2 of 2026-11-05); the splice marks each unit that no longer matches. The writer derives the path with the private
      `project_relative_source` (`engine_execution.rs:1108`) plus the `.ipynb` extension under
      `<project>/.quarto/execution-records`; expose **one** path function there and have the writer
      and this reader both call it.
- [x] **`off` (S1 B8, settled):** the eager driver returns immediately
      (`capture_driver.rs:67`), so with `off` and no ref nothing shows, while with an existing ref
      records show. Move the record-source lookup outside **that** guard: with `off` the cache and
      `q2` sources are consulted and the engine is never run. The watcher hook's guard (`:262`) stays:
      there is no source lookup in `recompute_staleness`, only the staleness compare, and
      `config.rs:30-32` documents `off` as making that hook a no-op. The re-execute guard is Phase B3.
- [x] Do not widen the WASM or SPA surface in this plan.

## Phase F: verify

Use `target/debug/q2`, never the `q2` on PATH (an old nightly that cannot read records). A Jupyter
document needs a venv (epic, "Verification environment"; set `SCRATCH` to a scratch directory first):
`uv venv "$SCRATCH/venv" && uv pip install --python "$SCRATCH/venv/bin/python" jupyter-core ipykernel matplotlib pandas`,
then `export PATH="$SCRATCH/venv/bin:$PATH"`.

- [x] `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub -p quarto-hub-provider --all-targets -- -D warnings` (`quarto` builds the production `PreviewConfig` literal, `commands/preview.rs:354`)
- [x] The Windows check moved to the epic close-out: the "Windows testing" bullet in the epic's Exit criteria
      already lists RIU-2's part (the cache tests with temp directories and file names). It needs a native
      Windows run, because `--target x86_64-pc-windows-msvc` cannot build from macOS (`aws-lc-sys`).
- [x] `cargo nextest run -p quarto -p quarto-core -p quarto-preview -p quarto-hub -p quarto-hub-provider`
- [x] Because this touches `quarto-core`, which the WASM client depends on, recommend
      `cargo xtask verify` to Gordon; he decides. Also run `npm run build:wasm`, since
      `Record::describes_something()` is in a module the WASM compiles.
- [x] Real binary, **needs a browser (Playwright on the preview iframe) and a Jupyter venv**, with
      the preview rebuild chain from `AGENTS.md` (`npm run build:wasm`,
      `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`): `q2 preview` on the S1
      `basic.qmd` fixture (`crates/quarto-preview/tests/fixtures/records-in-use/basic.qmd`) with
      the gate on, as S2 did (first open: one kernel, `captures/` holds the record file and the legacy
      file; restart on the same `--data-dir`: no kernel; cache copied into a fresh `--data-dir`: no
      kernel, every output shown; code edit: banner and Re-execute). State what a person did not see.

**Phase F observations (2026-11-06, merged tip `82908eaba`, macOS).** Run with `target/debug/q2` rebuilt through
the full chain (`npm run build:wasm`, `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`), a uv
venv with ipykernel, matplotlib and pandas first on `PATH`, and a `sitecustomize.py` on `PYTHONPATH` that
appends a line to a log whenever a process was started as `ipykernel_launcher` (read from
`sys.orig_argv`). Every server was started as
`QUARTO_ENGINE_RECORD_WRITE=1 q2 preview --no-browser --port N --data-dir D proj/basic.qmd`, with
`basic.qmd` copied from the S1 fixture, and driven by Playwright Chromium reading the page text in every frame.

- First open, fresh data dir: the page showed the text output, the DataFrame, the `ValueError`
  traceback and the inline value 42. The kernel log had 1 line. `captures/` held one `.bin` and one
  `.record.bin`.
- Restart on the same data dir: the same text, and the kernel log stayed at 1 line.
- Fresh data dir holding only a copy of `captures/`: the same text (7 expected fragments found), and the
  kernel log stayed at 1 line.
- Edit of a code cell while the server ran: the page showed "Code has changed since the last capture."
  and a Re-execute button, and the kernel log stayed at 1. Clicking Re-execute showed the new output and
  removed the banner; the kernel log went to 2 and `captures/` gained a second pair of files.

Not seen: the plot image itself (the screenshots were taken but only the text was compared), a share
guest, `preview.engine: off` or `manual` configured in `_quarto.yml` through the real binary (those are
covered by handler and driver tests only), Windows. The Windows cross-clippy cannot build from macOS
(`aws-lc-sys` needs a C toolchain for the target), so a Windows user should run the native check.

Merged-tip gate: clippy for the five crates passed; nextest for the five crates ran 9598 tests, 9597
passed and 1 failed. The failure, `julia_engine_e2e::j7_failed_run_does_not_leak_worker`, also fails alone:
the shared `~/Library/Caches/quarto/julia/julia_transport.txt` that it guards was absent after the run,
while two Julia servers started days earlier were still running on that cache. It passed in an earlier run
and nothing in this plan touches the Julia server, so it is environmental and not attributed to RIU-2.
`dashboard_goldens` also failed in the earlier core-only run and is tracked by another session.

## Exit criteria

- With the default on, preview and hub run with no legacy capture **required**: a record alone is
  enough everywhere, and every code location above has changed or has a stated reason it need not.
- A record with no legacy form is never published as an empty legacy list: it goes out as a record
  with the gate on, and is refused with a clear message with the gate off, by one predicate that
  `encode_for_publish` and the walk both call.
- A prose edit does not re-execute within a session or across a restart with a persisted
  `--data-dir`; a cold cache with no ref re-runs once, and that is pinned by a test.
- `preview.engine: off` shows a cached or `.quarto` record and never runs the engine.
- The source list type, the walk with its outcomes and `EngineResult`, and the `RecordLookup` trait exist
  (the trait compiles for wasm32), with the cache wrapper split, and re-execute runs the engine only.
- No kernel starts through the re-execute POST or the automatic path under `off` or with `engine` absent
  from the list, tested through the handler and `claim_and_spawn`.
- This is also the step toward EXR-17 trigger (deleting the legacy path); note in EXR-17 plan
  what remains.

## Coordination

KNR-6 moves knitr consumers onto structured records and uses the record-only entry from this
plan. If both touch `capture_driver.rs`, the integration branches rebase; there is no coupling
in the plans (epic, "Coordination with the knitr epic").

## Risks

- **The seam grows.** Keep it to publish, the walk and the guard; sources beyond `cache` and `q2`, the
  provider's trait implementation and the CLI belong to RIU-3 and RIU-4.
- **Nothing here is a permanent contract for provenance.** The outcome record and the per-source
  staleness rules exist because engines do not yet return provenance data; they should be revisited when
  engines do.
- **The `ReExecuteConfig` churn.** Replacing `(policy, cache_dir)` with one config touches about 40 call
  sites and 18 `PreviewConfig` literals in tests (spike 4's compiler counts: 36 errors in the lib tests, 19
  in the integration tests). It is mechanical, and it is the price of tests that never touch the
  environment.
