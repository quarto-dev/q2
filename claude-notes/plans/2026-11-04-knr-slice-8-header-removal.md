---
title: 'KNR-3 slice 8: a header option removed from a knitr chunk'
date: 2026-11-04
description: 'Fix the staleness where a presentation-only option deleted from a knitr chunk header keeps its recorded value. The knitr adapter records the header keys of each structured cell from the author text; a unit that lost one is stale in both the splice and record_is_stale, so the document re-runs. Restoring recorded defaults was dropped (derived options).'
status: done  # implemented 2026-11-04; see "Slice 8 notes". Gordon chose design (c), the author-text key source and the shared predicate.
---

**Part of:** [KNR-3](2026-10-30-knr-03-record-adapter-hash-fallback.md), slice 8. Epic:
[knitr execution records](2026-10-30-knitr-execution-records-epic.md).
**Evidence:** [spike note](../research/2026-11-04-stale-presentation-options.md) (run with
knitr 1.50) and its [plan](2026-11-04-knr-slice-8-stale-options-spikes.md).
**Depends on:** KNR-3 slices 1 to 6 (landed on `knr-2-presenter`). Slice 8 does not touch
`layer_options`, so it does not depend on slice 7\'s step order; it flips slice 7\'s Phase 7.0
known-gap test, which is not yet written.
**Must land before:** [KNR-6](2026-10-30-knr-06-flip-and-consumers.md) flips the live path, because
the staleness is invisible while the record is shadow output and visible afterward.
**Estimated sessions:** 1.

## Goal

A presentation-only option deleted from a `#|` header must stop showing. Today the unit still
matches (the `structured` hash profile leaves presentation options out), nothing re-runs, and the
presenter shows the recorded value. Run results (spike S1): deleting `fig-cap` keeps the caption;
deleting `echo: false` keeps the code hidden; changing a value is fine.

## Scope

- **In:** a `#|` header option deleted from a knitr structured cell, for every option, presented or
  not (the check does not know which keys the presenter reads).
- **Not a gap, so not here:** document-level `execute:` and `knitr:` keys (they are in the knitr
  document hash, so a change re-runs; run), `format: html:` keys (q2 ignores them; the review ran
  `fig-width`, `fig-align`, `fig-pos`, `keep-hidden`, run), fence parameters (Q-2-36), and a
  `ref.label` chunk\'s options (knitr copies code, not options; run). Project-level `_quarto.yml` and
  `_metadata.yml` keys reach the document hash through merged metadata (read, `hash_document`).
- **KNR-6\'s, not here:** a setup-chunk `opts_chunk$set` edit. The setup chunk goes stale. What
  closes the neighbors is a whole-document re-run, and that depends on the engine policy:
  `record_splice.rs` runs no engine (read); the preview driver re-runs only under
  `EnginePolicy::Auto` (read, `capture_driver.rs`). Under `preview.engine: off` or a manual policy a
  stale setup chunk still shows stale neighbors. KNR-6 must say so; this slice does not change it.
- **Out of scope:** `knitr: opts_chunk` and `format: html: echo` having no effect in a real render (a
  side finding in the spike note); the `ref.label` code staleness (S8).
- **Guard for the document-level claim:** the knitr declaration\'s `presentation_execute_keys` is
  `Vec::new()`, asserted only by `the_declaration_keeps_the_id_and_stores_the_structured_profile`.
  If a later change fills that list (slice 7\'s D10 does it for Jupyter only), the document-level
  rows reopen. That test is the guard; name it in any such change.

## Why restoring recorded defaults was dropped

The first draft (a) recorded the header keys and `opts_chunk$get(key)`, and `layer_options` restored
that value for a deleted key. The adversary review found it wrong, and these were run:

- q2\'s `opts_hooks` (`hooks.R`) derive presentation options from other keys. In a chunk labeled
  `fig-x` with `echo: fenced`, `opts_current` has `results` and `fig.show` as `hold`, while
  `opts_chunk$get` has `markup` and `asis`. `collapse: true` sets `comment` to `##`; the default is
  `NA`.
- Deleting `echo: fenced` would restore only `echo`; `fenced.echo`, `results` and `fig.show` are not
  header keys and keep the recorded derived values. A kept key can also change the right restored
  value of a deleted one. A per-key round trip passes every single-key test and is wrong here.

A fresh knitr run gets every derived option right, so the design re-runs on deletion instead of
reconstructing.

## Design: a deletion makes the unit stale

**D1. The key list comes from the author text, written by the knitr adapter.** The adapter already
holds `ctx.author_texts` and `ctx.units`, and calls `RecordBuilder::push`; the record builder itself
partitions the author text with `partition_author_text` (`record/builder.rs` ~147, `exec_hash.rs`
~109) and strips presentation keys from the cell\'s flat options afterward. In the structured-cell
branch of `adapter.rs` (next to `RESOLVED_OPTIONS_KEY`, ~117), partition the unit\'s author text the
same way and insert `quarto_header_keys`: the sorted key names of the whole `#|` mapping, as written
(minus `quarto-unit-id`, which the test pins as absent or stripped). Nothing is read from R, so
`capture.R`, `KnitrCapturedChunk` and `chunk_opts` are untouched; a blank chunk, `eval: false` and
every knitr version behave the same. The key stays out of every hash (`CellMetadata::options()` drops
`quarto_` keys), so `KNITR_HASH_VERSION` stays 2 and `quarto_record_version` stays 1.

**D2. Only structured cells carry it, and only they are checked.** The field is written for cells
the adapter maps to a structured cell, the same branch that writes `quarto_resolved_options`. A
fallback cell (hook override, `child=`, cache hit), a Jupyter cell and a cell from an old record
have no field, and a cell without it is never stale on this account and presents as before. This is
what keeps Jupyter, which re-presents from the document on every run, from re-running on a deleted
key.

**D3. The check is one shared predicate, applied at both levels.** A matched unit/cell pair is
demoted to stale when the cell\'s recorded keys are not a subset of the unit\'s current header keys.
Keys are compared with `-` mapped to `.` and nothing else: no alias table, no `known_options`, so
`record/` does not depend on the presenter. A swapped alias spelling (one key deleted, another
added) re-runs needlessly, which is the safe direction. An `!expr` value counts as present (it is a
key). The predicate is a function in `record/` (for example `lost_header_keys(&Matching, &[UnitKey],
&[RecordCell]) -> Vec<usize>`), called from two places:

- **Per unit:** the splice (`record_splice.rs`, after `match_units`, ~169) demotes each pair it
  returns, so the unit presents as source and reports `Stale`. `match_units` itself is unchanged.
- **Whole document:** `record_is_stale` (`record/staleness.rs`), after its hash comparison says
  fresh, builds the same unit keys as the splice (a unit with no owning engine gets a
  content-hash-only key), runs `match_units` against `record.cells`, and reports stale if the
  predicate returns any unit. It does not treat other unmatched units as stale, so its existing
  semantics (prose edits leave a record fresh) do not change. This is what makes the preview driver
  (`capture_driver.rs` ~312) re-run under `EnginePolicy::Auto`.

`UnitKey` gains the unit\'s current header keys (`Option<BTreeSet<String>>`, `None` for a span or a
unit that failed to partition); `from_author_text` already partitions the options, so it fills the
set at no extra parse.

**D4. A missing field means unknown.** No version bump. An old record, a fallback cell and a cell
from another engine behave as today.

**D5. Cost, accepted by Gordon.** A deletion of a presentation key re-runs the whole knitr document
(the document hash covers every unit hash). A deletion of a key the hash already covers (`eval`,
`warning`) already re-runs. Say so in the user-facing docs if the preview page documents re-run
triggers. Additions and value changes still show at once.

## Phases

TDD: tests first, watch them fail. Each phase ends with
`cargo clippy -p quarto-core --all-targets -- -D warnings` and `cargo nextest run -p quarto-core`
(known failure: `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`). The
preview-driver test is in `quarto-preview`; run the same two commands with `-p quarto-preview`.
Nothing here needs R, so no test is gated on `Rscript`, except the derived-option fresh-run check
below.

### Phase 8.0: tests first (all red)

- [x] Adapter (`engine/knitr/adapter.rs` tests): a structured cell\'s `extra` has `quarto_header_keys`
  holding every `#|` key (presentation ones included, `quarto-unit-id` not); a chunk with no header
  has `[]`; a blank chunk with a header has its keys; a fallback cell has none; the unit hash is
  identical with and without the field; `CellMetadata::options()` does not list it.
- [x] Predicate (unit tests beside it): a recorded key missing from the current header demotes the
  pair; the same key spelled `fig.cap` versus `fig-cap` does not; an added key does not; a changed
  value does not; an `!expr` value does not; a cell without the field is never demoted; an
  unmatched unit is left alone.
- [x] Splice (`record_splice.rs` tests): the header-deleted unit reports `Stale` and is left as
  source; its fresh neighbors still splice; the stage\'s `RecordStaleness` event shows it.
- [x] `record_is_stale` (`staleness.rs` tests): a deleted header key makes the record stale; an added
  key, a changed value and a prose edit leave it fresh; a Jupyter record with a deleted presentation
  key stays fresh (the D2 guard).
- [x] Preview (`quarto-preview`, driver level): a header deletion flips the sidecar staleness to true
  and triggers the re-run under `EnginePolicy::Auto`, and only sets the flag under the others.
- [x] Interacting keys (run, adversary finding): a `fig-` labeled chunk with `echo: fenced`, and one
  with `collapse: true`. An R-gated test captures the document, deletes the header key, captures
  again, and asserts the fresh capture differs from the old record in `results`, `fig.show`,
  `comment` and `fenced.echo`, and that the deletion is stale (so the old derived values are never
  presented).
- [ ] The slice 7 Phase 7.0 known-gap test, flipped once it exists: header deleted, the unit is
  stale, so the presenter is not asked to show the old caption. (Not written yet: slice 7 has not
  landed. The sweep deletion test and the real-record test pin the same property; flip slice 7\'s
  test when it lands.)

### Phase 8.1: adapter and predicate

- [x] Add `quarto_header_keys` in `adapter.rs` (D1, D2); make the adapter tests green.
- [x] Add the key set to `UnitKey` and write the predicate (D3) with a doc comment that states the
  `-` to `.` rule and why no alias table is used.

### Phase 8.2: the two call sites

- [x] Call the predicate from the splice (D3); make the splice tests green.
- [x] Call it from `record_is_stale` (D3); make the staleness and preview tests green. Check the
  unchanged-document fast path: the extra `match_units` runs only when the hashes already say fresh,
  so a stale document pays nothing; time a fresh one on the corpus and record it here.

### Phase 8.3: sweep and end to end

- [x] `record_splice/tests/option_sweep.rs` builds a synthetic record (`rich_record`, ~138), so it
  cannot compare against a fresh run. Add a deletion variant that asserts, for each presentation
  option the sweep varies, that deleting it from a header marks the unit stale.
- [x] End to end: `cargo run --bin q2 -- render` cannot show the record before KNR-6, so run the
  `corpus_report` shadow on a header-deleted variant of two fixtures and report the diff against the
  live output. Record the invocation and the observed output here.
- [x] `scripts/exr-record/nbformat_facts.py` is a validator with synthetic cases and no cell-key list;
  add one synthetic case with the new key, or note that none is needed. None is needed: it checks
  nbformat shape and `quarto_` keys it knows by name only (grep), and an unknown `quarto_*` key
  passes.
- [x] Update KNR-3\'s slice 8 item and KNR-6\'s layering test list to the D3 wording, and slice 7\'s
  Phase 7.0 known-gap test.
- [ ] `cargo xtask verify --skip-hub-build` is the recommended pre-push check (recommended to Gordon, not run).

## Alternatives

- **(a) restore recorded defaults:** dropped, see above. Fixing it would need per-option derivation
  rules mirrored from `hooks.R`.
- **(b) hash the set of presentation keys present:** re-runs on every *add*, the common edit, and
  moves the hashes of 80 of the corpus\'s 334 cells (run, spike note); needs `KNITR_HASH_VERSION` 3.
  Simpler than this design, but this one keeps adds instant and needs no hash bump.
- **Reading the keys from R (`chunk_opts`):** dropped. A blank chunk has no `chunk_opts` (run, plain
  knitr gives `NULL`), knitr before 1.44 has none, and it relies on knitr internals; the author text
  is what q2\'s own parser saw.

## Risks

- The `-` to `.` comparison is coarser than the presenter\'s alias rename: a spelling swap re-runs
  without need. Accepted; the other direction (a false "fresh") cannot happen, because two spellings
  of one knitr key differ only in separators.
- `record_is_stale` now needs the record\'s cells and one `match_units` call after a fresh hash
  check; Phase 8.2 measures it.
- The new key is written only by the knitr adapter. A second engine that gains a structured profile
  must add its own (D2); the predicate does not look at engine names.

## Decisions (Gordon, 2026-11-04)

1. Design: a deletion makes the unit stale; defaults are not restored. Designs (a) and (b) are
   dropped.
2. Key source: the author text, written by the knitr adapter; R `chunk_opts` is not used. The
   knitr 1.44 question is moot.
3. Placement: one shared predicate, applied in the splice and in `record_is_stale`.

## Exit criteria

The Phase 8.0 tests pass, including the interacting-key cases and the Jupyter guard;
`KNITR_HASH_VERSION` is still 2; the corpus shadow shows no new blocker; and the end-to-end evidence
and the Phase 8.2 timing are recorded here.

## Slice 8 notes (2026-11-04)

What landed, and where it differs from the plan above.

- **Code.** `HEADER_KEYS_KEY` (`quarto_header_keys`), `UnitKey::header_keys`, `Matching::demote` and
  `lost_header_keys` are in `record/matching.rs`. The adapter writes the key in `engine/knitr/adapter.rs`
  (`header_keys`, which drops the join id). The splice demotes after `match_units`; `record_is_stale`
  runs `match_units` and the predicate only after the hash check says fresh. `KNITR_HASH_VERSION`
  is still 2.
- **`record_is_stale` and the format.** A cell that names a hash profile matches by a hash that counts
  the output format, which `record_is_stale` is not given. It reads the format from the record\'s own
  structured cells (`quarto-output-format`), so the signature is unchanged. With no such cell it builds
  plain keys.
- **Finding, fixed by [KNR-3 slice 9](2026-10-30-knr-03-record-adapter-hash-fallback.md) (2026-11-04):** the whole-document hash check in `record_is_stale` hashes every unit
  under the default profile, while a knitr cell is hashed under `structured`. A real knitr record
  therefore reads as stale today even when unchanged (run:
  `unchanged stale=true` for `options/echo/yaml/00_true` and `options/fig.cap/yaml/00_a_caption`
  from the real engine). So the new whole-document leg is unreachable for knitr until KNR-6 hashes
  profiled cells the way the writer did; the per-unit leg in the splice is live. The Jupyter and
  test-engine cases in `staleness.rs` and the preview driver exercise the whole-document leg.
  Slice 9 hashes a profiled cell under its profile, so the whole-document leg now fires for knitr.
- **`fig.show` deviation.** The plan expected deleting `echo: fenced` from a `fig-` chunk to change
  `fig.show`. It does not (run): the `fig-` label sets `hold` on its own. `fenced.echo` and `results`
  do change (`hold` to `markup`), and `collapse: true` changes `comment`. The R-gated test
  (`capture_tests::deleting_a_header_key_changes_the_options_derived_from_it`) asserts those.
- **Red first.** The tests were written before the code (compile-red), and the behavior was then
  checked by disabling the two call sites: the splice test, the staleness test and both driver tests
  failed, the rest stayed green.
- **Sweep.** `deleting_any_swept_option_from_the_header_makes_the_unit_stale` deletes each of the
  `SWEEP` options (all 100 rows) from a header that had it, over a record whose cells carry their keys,
  and asserts the unit is stale.
- **Timing (Phase 8.2).** A 1000-cell Jupyter-style document, 20 calls of `record_is_stale` on a fresh
  record, release-like test profile: 5.4 ms per call with the new `match_units` and predicate, 5.0 ms
  with them skipped, so about 0.35 ms per 1000 cells. A stale document pays nothing.
- **End to end.** Invocation: `cargo nextest run -p quarto-core knitr_header_deletion --no-capture`
  (real `KnitrEngine`, real R). Observed, and inspected:
  `options/echo/yaml/00_true: recorded header keys [Array [String("echo")]]`, then
  `after deleting the header, matched [Some(0)], demoted units [0]`; the same for
  `options/fig.cap/yaml/00_a_caption` (`fig.cap`). The unit still matched by hash (the key is not in
  the structured hash) and the predicate demoted it. `corpus_report` presents a fresh record of
  whatever document it is given, so it cannot show a stale record; I ran it only to check the adapter
  change on the same two fixtures
  (`KNR_REPORT=<file> KNR_ONLY=options/echo/yaml/00 cargo nextest run -p quarto-core corpus_report`):
  gate PASS, 1 accepted, frozen oracle 1 match, for each. No header-deleted fixture was added to the
  corpus, because the real-record test above covers the deletion directly.
- **User docs.** `docs/guides/authoring/execution-records.qmd` already says a `#|` option line edit makes the
  record stale, which now holds for a deletion on knitr too; knitr records are not live until KNR-6, so
  it needs no change here.
