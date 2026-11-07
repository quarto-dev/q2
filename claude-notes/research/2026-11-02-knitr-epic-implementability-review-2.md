---
title: "Knitr execution-records epic: implementability review 2 (KNR-3 from 3C to KNR-7)"
description: "Read-only review of whether KNR-3 (from 3C), KNR-4, KNR-5, KNR-6 and KNR-7 can each be started today without guessing, with a disposition of the 2026-10-31 findings."
---

# Scope and method

- Branch `review/knr-implementability`, **reviewed commit `05935dabaf327aaadb0def5f64e1d13600648194`**
  (the KNR-2 `2C` tip, branched from `feature/knitr-execution-records-epic` at `7ba4d2b9a`).
- KNR-1 and KNR-2 are a fixed contract. Items that wait on KNR-2 work not yet landed are marked
  "depends on KNR-2 (item)".
- Two sonnet sub-agents (an executor and an integrator) reported. The lead re-checked every
  "blocks start" finding and a sample of the rest. **Verified** = the lead opened the file or ran
  the command; **reported** = a sub-agent only.
- Not run: tests, clippy, R. Paths are relative to `claude-notes/plans/` unless they start
  with `crates/` or `docs/`.
- Plan files in this note are abbreviated: KNR-3 is `2026-10-30-knr-03-record-adapter-hash-fallback.md`,
  KNR-4 is `...-knr-04-inline-typed-values.md`, and so on.

# Ready to start

| Plan | Verdict |
|---|---|
| KNR-3 (from 3C) | Ready after A1 (document-level staleness), A2 (write the settled `known_options` name and fix the marker text into the plans) and rebasing onto the parent tip; its 3C shadow test also depends on KNR-2 (present-from-record path for knitr). |
| KNR-4 | Ready after the plan text follows the settled classic-span rule (A3) and drops the spelling key (A4), and after the wire type is named (B3). |
| KNR-5 | Ready after B4 (the settled mime marker is written into the plan) and EXR-16 landing a caller of the importer; 5C needs Gordon\'s Positron fork. |
| KNR-6 | Blocked on EXR-15 landing on this line (rebase), bd-js24dold (open), KNR-2 exit, KNR-3 and KNR-4; A5 and A6 are settled in direction but not yet written into KNR-6. |
| KNR-7 | Not startable by design (written last); needs code numbers and concrete docs paths first (B5, B6). |

# A. Blocks starting a plan

**A1. Document-level staleness ignores the structured profile (KNR-3 3A and 3B; blocks KNR-6).**
*Verified* at the parent tip: `git show feature/execution-records-epic:crates/quarto-core/src/record/staleness.rs`
lines 44-96. `record_is_stale` builds every unit hash through `UnitKey::from_author_text` and passes
`UnitHashRef { profile: None }` to `hash_document`, with no format argument. A structured knitr
record\'s `document_hash` is built from structured-profile unit hashes, so the recomputed hash never
matches and every structured record reads stale. KNR-3 decision 8 and 3A change only
`match_units` and `splice_units`. *KNR-3 must change:* add a profile- and format-aware path to
`record_is_stale` (the alt-hash approach already planned for `UnitKey`), with four tests: fresh for
the same format, stale for another format, fresh after a presentation-only edit, stale after an
execution-relevant edit. KNR-6 6A "per-unit staleness" should cite them. This file does not exist
on this branch (EXR-15 is `draft` here); rebase first.

**A2. The per-record unknown-options field and the `quarto_knitr` marker contradict the fixed contract (KNR-3, KNR-5, KNR-7).**
*Verified.* KNR-3 lines 107-111 record Gordon\'s 2026-11-01 decision to replace the per-cell
`quarto_unknown_options` with one per-record field "chosen in Phase 3B"; KNR-3 lines 339 and 390 and
KNR-5 line 152 still write the per-cell key. KNR-3 line 391 says the adapter writes "the
`quarto_knitr` marker", and KNR-5 line 152 and KNR-7 line 60 repeat it, but epic decision 22 dropped
`quarto_knitr.asis` and `quarto_knitr.raw` (commit `5f54c0416`); what survives is
`metadata.quarto_knitr.file`/`knit_meta` plus the flat `quarto_markdown_passthrough`. KNR-2 (fixed)
reads the per-cell key and its message-versus-warning items depend on KNR-3 3B item 1.
*Settled (Decision 1):* the field is `known_options`, an optional list on the `quarto_engines`
entry beside `hash_definition`. Being an engine-entry field, it never passes through
`CellMetadata::options()` (`crates/quarto-core/src/record/types.rs:186-189`), so it needs no
`quarto_` prefix. *Change:* KNR-3 names the field, states in one sentence that it replaces the
per-cell key KNR-2 reads (pointing at KNR-2), and says an absent field means "no unknown-option
filtering". Fix KNR-3 lines 339, 390 and 391 (the marker is `quarto_markdown_passthrough`),
KNR-5:152 and KNR-7:60, and add a pointer under epic decision 21. No `quarto_record_version`
bump: `RECORD_VERSION` is 1 on both epic tips and the reader rejects only future versions
(`record/reader.rs:59`); the KNR-3 knitr declaration bump covers the change. Depends on KNR-2 (the
reader of the key).

**A3. KNR-4 premise contradicts the unit scanner (classic spans).**
*Verified.* `crates/quarto-core/src/engine/units.rs:1-5` scans "brace-spelled inline spans" only.
KNR-4 says the shared span regex is the only definition of a span and requires a test "with both
spellings", and the S9 decision aligns ids by position across all 22 `evaluate.inline` calls, which
include classic `` `r expr` `` spans. A classic span has no unit and no id.
*Settled (Decision 2):* classic spans stay on a live R path and only brace spans get records.
*Change:* KNR-4 drops the "both spellings" test and the claim that the regex is the only span
definition; it says R-side entries carry the spelling (the `.QuartoInlineRender` wrapper present or
absent) and only wrapped entries are matched to the unit list. KNR-6 6B says how the live path
substitutes classic spans after the flip (A5).

**A4. KNR-4 still describes the pre-S9 design (spelling key and escaping).**
*Verified* by grep: KNR-4 lines 60, 82-88, 108, 111-112 say the presenter applies
"spelling-specific escaping" and add a `quarto_inline_spelling` key. The S9 decision (KNR-1 around
lines 882-926) is that the record holds the text knitr inserted, after R-side escaping, and both
`NULL` rules run in R. *KNR-4 must change:* rewrite decisions 2 and 3 and the Presenter phase, drop
the spelling key and its test; retitle "typed values". Also, `presenter/inline.rs` (EXR-3;
`classify_inline_result`, `InlineTarget`) already exists, so the Presenter phase should call it, not
add new code (reported).

**A5. Nobody owns inline substitution after the flip (KNR-4 decision 2, KNR-6 line 102-105).**
*Reported.* Each defers to the other; the S7 verdict covers chunk markers only. *KNR-6 6B names the
owner.* Given Decision 2, it states that classic spans are substituted by R on the live path, and
whether brace spans are substituted at present time from the record or carried by markers. KNR-4
then builds only the record side and calls `presenter/inline.rs`.

**A6. KNR-6 6B "one line" replay dispatch is not one line.**
*Verified in part.* `crates/quarto-core/src/engine/replay.rs:228` gates `execute` on
`engine_name == JUPYTER`, and `present_record` (113-157) is Jupyter-specific (error text names a
"jupyter capture"). The "one line, prototyped in I2" claim rests on `spike/knr-i2`, which predates
KNR-2\'s `CellOptions`. *Settled (Decision 3):* generalize `present_record` into one function that
live knitr rendering and replay both call. Engine differences (where options come from, the error
text) are small inputs, not a branch on engine name. *KNR-6 must change:* replace the "one line"
claim with this design; add a test that replaying a knitr record equals the live output; keep the
existing Jupyter replay tests green; say what `capture.result.markdown` holds after the flip (the
skeleton or the final text). Reported for the unverified parts.

**A7. KNR-6 dependencies not met on this line.** *Verified.* bd-js24dold (ReplayEngine does not claim
`{r}` chunks) is `open` (`braid show`), while KNR-3 lines 25-26 and KNR-6 lines 18-19 say it lands
first; the epic table (lines 405-420) does not list it. EXR-15 is `complete` only on the parent tip.
Add it to the epic table as a hard dependency of KNR-3 and KNR-6.

# B. Wrong references

**B1. Stale parent status.** *Verified.* EXR-10, 11, 12, 13, 14 are `status: complete`
(front matter, `2026-10-28-exr-*`). Still called drafts or "needs merging": epic lines 5, 81, 409,
417-420; KNR-3 lines 16-18, 369-372, 403-404; KNR-4 line 10; KNR-1/epic table row for KNR-1. At the
parent tip EXR-8, 9, 15 are also complete; only EXR-16, 17, 18 are drafts. The "test needs EXR-11,
hand-build one" text can use the real recorder (`engine/knitr/chunk_records.rs`).

**B2. Error-code range and ownership.** *Verified.* KNR-3 line 401 says `docs/errors/engine/` holds
Q-18-1 to Q-18-3; the directory holds Q-18-1, 2, 3, 4, 5 and 10, so "next free" is ambiguous (6 or
11). KNR-3 3B allocates codes, KNR-7 lines 63-66 allocates again and adds "unsupported knitr version"
and "unrecognized capture shape" that no earlier plan creates. Neither plan names the
`docs/_quarto.yml` sidebar entry (lint `error-docs-sidebar-unlisted`) or says the page and entry ship
in the same commit as the catalog entry (`error-docs-page-missing`). `EngineRecordProblem::CountMismatch`
already exists (`crates/quarto-core/src/engine/diagnostics.rs`, reported) and should be reused.
*Change:* KNR-3 gets one table (code, trigger, page, sidebar line); KNR-7 only documents them.

**B3. Wire fields undefined (KNR-4, KNR-3 3B).** *Reported.* `KnitrExecuteResult`
(`engine/knitr/types.rs:102-161`) has `chunks`, `spans`, `pieces`. S9\'s per-span `{ordinal, code,
text}` has no Rust type or JSON key. The first test KNR-4 can write is a deserialize test once the
field is named. KNR-3 line 166-176 leaves the resolved-options metadata field "decided here after
S3". `quarto_output_anchors`, `quarto_output_kinds` and `quarto_unknown_options` appear in no `.rs`
file yet: depends on KNR-2.

**B4. KNR-5 importer seam.** *Verified.* `PositronImport::outputs_for(content_hash, occurrence,
owner, lang)` (`crates/quarto-core/src/record/positron_import.rs:140-152`) refuses unless the owner
is Jupyter. KNR-5 wants `knitr` plus `r`, and to refuse fallback units, but the function has no
fallback input, and no key marks a fallback unit (only the `text/x-quarto-markdown` mime does; epic
decision 8 says "flagged in its metadata" without a key). Nothing outside tests calls the importer.
*Settled (Decision 4):* the `text/x-quarto-markdown` mime is the fallback marker; no new key.
*KNR-5 must* state the new signature or the caller check that refuses a unit whose output carries
that mime, and KNR-3 and epic decision 8 must say "the mime is the marker". EXR-16 is still `draft`; the I4 fixture lives on `spike/knr-i4`.
KNR-5 lines 22-26 ("nothing was run against a real Positron") contradict lines 28-46.

**B5. KNR-7 placement.** *Reported.* "Linked from the places EXR-18 links to" (lines 11-12, 89-90)
depends on a draft. Name the `docs/` pages and sidebar entries. Exit criteria give no command for the
docs build: `cargo run --bin q2 -- render docs/`, and `cargo xtask build-agents-docs` plus
`cargo build --bin q2` if the embedded docs must refresh.

**B6. `replay-engine.md` field name.** *Verified.* `claude-notes/instructions/replay-engine.md`
lines 16 and 24 say `engine_capture.record`; the trace field is `engine_captures` (lines 59-72, 144).
KNR-7 line 54-59 and bd-js24dold\'s comment both claim the fix; assign it to one.

**B7. Dangling or stale links (reported).** KNR-2 line 325 and KNR-3 lines 336-341 cite
`2026-11-01-exr-19-widget-outputs.md`, which exists only on `feature/execution-records-epic`.

# C. Inconsistencies

**C1. Cache hits.** *Reported, KNR-1 S4 line 637-653.* Knitr\'s cache is forced off for every
recording run, so a level-3 hit never occurs, yet KNR-3 line 308 (table row), epic decision 8,
KNR-6 6A line 150 and KNR-7 line 39 still list it (*verified* by grep). KNR-3 has no 3A test for
the unconditional cache-off hook. After the flip recording is always on, so a `cache: true`
document reruns every chunk on every render: KNR-6 6B, its exit criteria and KNR-7 user docs must
say so. KNR-6 Risk 5 (pre-flip caches) is obsolete.

**C2. KNR-2 and KNR-3 wait on each other (earlier item 19, still present).** *Reported.* KNR-3
hard-depends on KNR-2 whole; KNR-2\'s zero-blocker bar (message vs warning, unknown options, hidden
widget mark "decide it with KNR-3") needs KNR-3 3B item 1. Fix in the epic table and KNR-3: KNR-3
depends on KNR-2 Phase 2F only; KNR-2 exit depends on KNR-3 3B item 1; KNR-6 depends on both. The
hidden-widget mark has no owner in KNR-3.

**C3. Header merge has no owner.** *Reported.* `CellOptions::from_knitr` takes one options value
(`crates/quarto-core/src/presenter/options.rs:78-82`). Epic decision 11, KNR-3 decision 3 and KNR-6
assume "record options, current header wins", but nothing names who parses `#|` lines and fence
attributes and merges. Name the function in KNR-3 3B or KNR-6 and test it. Depends on KNR-2
(record-read interface, "to be designed" at KNR-2 line 206).

**C4. Imported-unit options (KNR-5 lines 89-94).** *Reported.* KNR-2 reads `fig.cap` and friends
under knitr\'s dotted names only; KNR-5 says imported units take them "as for Jupyter cells". State
which constructor and spelling. Depends on KNR-2.

**C5. Status paragraphs.** KNR-6 line 44 "design, untested" vs a settled S7; KNR-4 lines 54-60
"not verified" vs S9; KNR-6 lines 49-56 superseded by S4. Session totals: epic line 382 still
says 2-3 for KNR-5, which now estimates about 1.

**C6. Units-only draft (conflict only).** The draft says it overrides KNR-3 decision 6 (`child=`
fallback) and KNR-4 decision 1, and sequences after KNR-6. Flag in KNR-3 and KNR-4 so nobody builds
more of the `child=` fallback than its table requires. It did not read KNR-5 or KNR-6.

# D. Gaps and repo rules

- **D1. Windows cross-check.** *Verified* by grep: no KNR-3..7 plan mentions `windows-msvc`;
  KNR-1 line 1232 does. Add `cargo clippy -p quarto-core --all-targets --target
  x86_64-pc-windows-msvc -- -D warnings` to each exit. Hazards: `metadata.quarto_knitr.file` and the
  files map are wire format (forward slashes via `quarto_util::to_forward_slashes`), R-dependent
  tests, figure directory handling.
- **D2. Test layout.** *Verified* by grep: none of KNR-3..7 mentions `tests/integration/` or
  `main.rs`. Add one sentence each; R-calling modules need `#![cfg(not(target_arch = "wasm32"))]`;
  reuse `knitr_corpus_run::r_available` for skips instead of copying `skip` helpers.
- **D3. WASM leg.** `matching.rs`, `record::staleness`, the presenter and `place.rs` reach the WASM
  build. KNR-3, 5, 6 exits should add `cargo xtask verify` (not only `--skip-hub-build`).
- **D4. KNR-3 3C exit criteria** name no command, seed or sample size, and no skip count when R is
  absent. The harness exists (`crates/quarto-core/tests/integration/knitr_corpus_*.rs`, `sample(seed,
  n)`); give the filter and expected counts.
- **D5. KNR-3 3A items 4-6** (parent edit for decision 8): *verified* that `UnitKey`
  (`record/matching.rs:23-29`) has no alt hashes yet; the item includes a report-to-Gordon gate, so
  say that this edit comes first and gates the splice-key tests. Tests that only need existing
  APIs (`HashDefinition`, `quarto_hash_rule`, `hash_unit`) can be written now.
- **D6. KNR-6.** Add a checklist item to count existing knitr traces (epic line 226-227 defers to
  it); decide `trace: true` overwriting instead of "either/or" (line 161-162); a sign-off file for
  the signed-off divergences list (path not named); define "latest supported knitr"; list the
  test-file disposition for the deleted `chunk_records.rs`, `span_records.rs` code (tests in
  `tests/integration/knitr_chunk_records.rs`, `knitr_inline_records.rs`,
  `knitr_canonical_input.rs`, `capture_splice_knitr_label.rs`); say `want_record`
  (`engine/context.rs:232-237`) and the wire `capture` flag become unconditional; KNR-7 "plain
  render writes none" should read "writes no file".
- **D7. KNR-2 2F text is stale (note for KNR-6).** `record/place.rs` already places a lone
  `text/x-quarto-markdown` unit verbatim; `present_document` does not. KNR-6 should test both
  paths.
- **D8. Needs of R, network, Positron.** KNR-3 and KNR-6 need a scratch library with older knitr
  and evaluate (network; KNR-6 line 136 says "minimum and latest supported knitr" without saying
  so). KNR-5 5C needs Gordon\'s Positron fork; the done spikes did not observe a data frame, a
  warning or the real binary, yet the exit criteria require them through `q2 render` and
  `q2 preview` (which needs the WASM chain in AGENTS.md).
- **D9. Claims without evidence.** KNR-3 "349 record, splice, replay and presenter tests pass" and
  "about 65 lines" come from `spike/knr-i3` (commit `9a0ff2d23` exists; reported, and it predates
  `record::staleness`); KNR-6\'s I2 findings come from `spike/knr-i2` (reported); epic line 326
  (htmlwidget ids) and 348 (`evaluate()` enforces logical warning/message) are "reported, not re-run";
  session estimates are labeled guesses. Re-read `exec_hash.rs` and `matching.rs` at the rebased
  tip: the parent changed `exec_hash.rs`.
- **D10. Rebase conflict.** *Reported.* KNR-2 and the parent both edit
  `crates/quarto-core/src/presenter/document.rs`; expect a conflict when rebasing.
- **D11. Deliberate exception.** `quarto-output-format` is hyphenated to escape the `quarto_`
  filter (KNR-3 lines 232-237); say plainly that it is an exception and the `options()` lookup is
  the only guard.

# Disposition of the 2026-10-31 findings

Only findings about KNR-3 to KNR-7 or the epic above them. KNR-1 and KNR-2 findings skipped (1-4,
9-12, 14-16, 24-28 are KNR-1/KNR-2 scope or about them).

| Item | Disposition | Evidence |
|---|---|---|
| 5 (hooks.R version list) | Applied | spot-checked as applied (lead brief) |
| 8 (EXR-17 trigger wording) | Applied | spot-checked as applied |
| 13 (KNR-3 calls EXR-11 "landed") | Obsolete, superseded | EXR-11 is now complete; the line now reads "a draft" instead: B1 |
| 15, 35 | Applied | spot-checked as applied |
| 17 (who decides a non-HTML target falls back) | Unclear; still open | KNR-3 line 312 still lists `gfm` "once the corpus covers it, which KNR-1 does" |
| 18 (dependency table vs Depends-on) | Still open | KNR-5 line 17 "Blocks nothing hard" now matches the table; but bd-js24dold and EXR-4/EXR-7 rows still missing, epic table line 405-420: A7, C2 |
| 19 (cycle KNR-2/KNR-3) | Still open | C2 |
| 20 (imported-unit option rule) | Still open | KNR-5 lines 89-94: C4 |
| 21 (results hide vs knit_asis) | Still open | no `results=hide` test in KNR-3 (grep: only lines 86, 137, 342 mention `knit_asis`) |
| 22 (parent text edits, KNR-6 6B) | Unclear | KNR-6 lines 163-164, 173-174 now say the parent text is not edited; the 6C item recording EXR-17 trigger item 1 remains; wording only |
| 23 (flat cell keys: second parent question) | Unclear | KNR-3 line 52 refers to the profile\'s `presentation_cell_options`; not in the epic open-questions list |
| 29, 30 (type names fit EXR-4) | Applied (still true) | `HashDefinition` and `HashProfile` at `record/hash_definition.rs:25,41`, `quarto_hash_rule` at `record/types.rs:180` |
| 31 (needs a decision or spike result) | Partly applied | S4 settled (C1); child/hook rows follow the units-only draft (C6) |
| 32 (decision 8 blocks) | Applied | KNR-3 decision 8 settled 2026-11-01 (I3), but the parent edit and A1 remain |
| 33 (repo rules) | Partly applied | wasm gate and `join_paths` applied; Windows clippy cross-check not found (D1) |
| 34 (claims and estimates) | Still open | D9 |
| A1 to A4, B13 | Obsolete | EXR-1, 2, 4, 7, 10, 11, 12 landed |

# Decisions (settled by Gordon, 2026-11-02)

The epic and KNR-3 to KNR-7 were rewritten for these four decisions (and the spelling-key removal that
S9 already settled). The other findings are not yet applied to the plans.

1. **Unknown-options field (A2): `known_options`.** An optional list on the `quarto_engines` entry,
   beside `hash_definition`; absent means no filtering. No `quarto_` prefix and no new record
   version.
2. **Classic `` `r expr` `` spans (A3, A5): stay on a live R path.** Only brace spans get records.
3. **Replay of a knitr record (A6): generalize `present_record`.** One shared function for live
   rendering and replay; engine differences are inputs, not a branch on engine name.
4. **Fallback-unit marker (B4): the `text/x-quarto-markdown` mime.** No new metadata key.
