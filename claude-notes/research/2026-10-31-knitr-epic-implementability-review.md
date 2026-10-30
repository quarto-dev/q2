---
title: "Knitr execution-records epic: implementability review"
---

Date: 2026-10-31. Branch `feature/knitr-execution-records-epic` at `cd3526550`.

Scope: could an engineer or agent start KNR-1 today, and KNR-3 once its spikes report,
without guessing? This is not a second correctness review of the knitr claims. Two Sonnet
reviewers worked independently (an executor walking KNR-1 and KNR-3 3A/3B, and an
integrator checking references and consistency). The lead re-checked the findings below
against the files; items marked *verified* were checked by grep or read, items marked
*reported* rest on a reviewer alone.

Settled and not reopened: epic decisions 1 to 20, the 2026-10-31 decisions on the early
start, document-scope layering, no pattern form, and the mixed-record hash. The format in
the record (KNR-3 decision 8) is pending on the parent session and is listed only under
what it blocks.

## Ranked findings

### A. Blocks starting KNR-1

1. **Nothing KNR-1 builds on exists in code, and the plan does not say what to do until it
   does.** *Verified.* EXR-1, 2, 4, 7, 10, 11 and 12 are all `status: draft`. In code,
   `render_cell`, `FigureWriter` and `CellVisibility` exist only as `pub(super)` items in
   `crates/quarto-core/src/engine/jupyter/text_execute.rs` (lines 425, 562, 623), visible
   only inside the Jupyter module. `scan_units`, `Unit` and a `presenter/` module do not
   exist. KNR-1 line 12 says it may start once EXR-1 and EXR-2 are "available" but gives no
   stub, wait or skip rule.
   - Can start with no parent code: S1 to S5 and S7 to S9 (R experiments), the classifier,
     the normalizer, the corpus generator, Phase 1B capture.
   - Needs parent code: the S2 mapper output types, the Phase 1C baseline, the test
     `capture_one_unit_per_chunk_in_order`, and the id hand-off.

2. **The plan names today\'s presenter API, not the one EXR-1 will produce.** *Reported,
   partly verified.* KNR-1 lines 83 to 85 call `render_cell(language, code, &result, &mut
   FigureWriter, CellVisibility)`. EXR-1 (lines 140 to 146) replaces `FigureWriter` with a
   `FigureSink` trait (`FsFigureSink`, `RuntimeFigureSink`) and renames the result struct.
   An executor cannot tell which names are meant, nor which sink Phase 1C passes, nor
   whether the integration-test crate can reach the module (`pub` versus `pub(crate)`).

3. **Two interfaces inside KNR-1 are named but not defined.** *Verified by grep.*
   - Phase 1B returns "per-chunk records `{id, options, pieces, files}`" and Phase 1C
     consumes `KnitrExecuteResult.pieces`, but the JSON key, the per-piece shape (class,
     text, plot path, message call, `knit_meta`, source position) and the figure-path
     carriage are not specified. S2 decides the nbformat mapping but is not told to also fix
     the wire shape R emits.
   - It is not said whether `pieces` extends EXR-11\'s `chunks: Vec<KnitrChunkRecord>`
     (`{id, markdown, options_digest, files}`) or is a separate top-level field.
   - Where the `id` comes from before EXR-4 and the shared unit list exist is unstated:
     the header (line 14) promises an id hand-off in Phase 1B, but no Phase 1B checkbox
     contains it, and `scan_units` returns `id: None` (EXR-2).

4. **The baseline presenter has an unanswered design question.** *Reported.* S2 says that
   under `collapse=FALSE` knitr alternates source and output groups, so a record carries
   source-piece positions. Phase 1C says to present each chunk with the unmodified
   `render_cell`, which takes one `code` string and one output list. How a chunk with
   several source groups is presented (one call per group with empty code, or one call per
   chunk as a known divergence) is not stated, and it is not one of the S spikes.

### B. Wrong references (cheap to fix)

5. *Verified.* The version-branch list `hooks.R:332, :793, :842` (epic lines 254 to 255,
   KNR-1 lines 75 to 77) is wrong: `hooks.R` contains no `packageVersion` call. The real
   branches are `patch.R:6`, `:152`, `:300`, `execute.R:774` and `rmd.R:8`. Also, `rmd.R:8`
   is a hard floor (stops below knitr 1.44) for `.R` input, so "not a compatibility floor"
   is imprecise.
6. *Verified.* `block.R:302` is cited as the `evaluate` hook (epic lines 299 to 300, KNR-1
   line 148); in the snapshot the assignment is line 303 (302 is `cache.exists`).
7. *Verified.* KNR-4 lines 89 to 90 cite `preprocess.rs:62-70` for "adjacent spans evaluated
   once"; that note starts at line 71.
8. *Verified.* "Mark knitr\'s row of EXR-17\'s trigger satisfied" (epic lines 333 and 351,
   KNR-6 lines 20 and 151) points at nothing: EXR-17\'s trigger has no knitr row.
9. *Reported.* KNR-1 line 73 says `valid_path` is "line not read"; it is `patch.R:285-292`.
10. *Reported.* The helpers `rscript_available`, `knitr_r_package_available` and `skip` in
    `knitr_label_reinject.rs` are private and copy-pasted into at least five other test
    files. New test modules cannot reuse them, and the plan does not say whether to copy or
    extract. The cited range `:37-58` also cuts the `skip()` body, which runs to line 64.
11. *Reported.* The three `engine_output_parity.rs` helpers KNR-1 says to "reuse" are all
    private `fn`s (lines 63, 90, 114). The plan needs "promote to `pub(crate)`". Also
    `post_engine_ast` goes through the legacy `record_capture` path, which EXR-5, 6 and 17
    change or delete.
12. *Verified.* `KnitrExecuteResult` derives only `Debug, Clone, Deserialize`
    (`types.rs:94-96`), so the test `pieces_field_round_trips` (and KNR-3 line 224) can only
    be a deserialize test unless `Serialize` is added; the plan does not say which.
13. *Verified.* KNR-3 line 16 calls EXR-11 "landed" and the epic (line 347) and KNR-1 line
    13 call EXR-10, 11 and 12 landed. All three are drafts. Effects: the test "EXR-11-era
    record reads as stale" has no producer, "replace EXR-11\'s recorder" has nothing to
    replace, and "the chunk hook that EXR-11 built stays" has no hook yet.

### C. Inconsistencies between plans

14. **KNR-2 contradicts itself.** *Verified.* Lines 125 to 132 and decision 2 say logical
    `warning` and `message` are execution-relevant, so the presenter does not filter them;
    Phase 2D (lines 225 to 230) says the presenter filters `warning` and `message` (logical
    and numeric). Numeric forms are presenter-side; the logical forms are not. The text
    should say so once.
15. **KNR-2 refers to a coordination decision that is already settled.** *Reported.* Lines
    217 to 220, the exit criteria and Risk 2 still talk about a pending "coordination
    decision" and a flag, while epic decision 12 and EXR-6 forbid a runtime switch.
16. **The chunk hook has three roles in three plans.** *Reported.* The epic, KNR-3 and
    KNR-6 say it stays "narrowed to fallback chunks". KNR-1 S3 says final options exist
    "only at the chunk hook or after" and must be captured there, so structured chunks need
    R-side chunk-hook work. KNR-6 S7 has it emit a marker for structured chunks. Whether
    EXR-11\'s `quarto-unit-id` hand-off survives is not listed in KNR-6 6C.
17. **Who decides a LaTeX or other target falls back.** *Reported.* Epic decision 13 and
    KNR-2 2E say KNR-2 "routes"; KNR-3 says the chunk hook writes it. R must know the
    target to skip capture; which side decides is not stated. Also KNR-3 line 185 lists
    `gfm` as an uncovered target while KNR-1 line 324 puts it in the corpus.
18. **Dependency table versus Depends-on lines.** *Reported.* KNR-4 lists EXR-4 and the
    epic table does not. KNR-5 says it blocks KNR-7 while the table makes it soft. KNR-3
    marks EXR-7 soft although EXR-11 hard-depends on it in the parent table. KNR-2 2F builds
    splice support for EXR-14, which is not in its Depends list. The KNR-4 table note says
    "blocked on S9" while the plan says S1 and S9.
19. **A cycle in a decision.** *Reported.* KNR-3 hard-depends on KNR-2 for the pass-through
    presenter; KNR-2 Phase 2F tests need the stored-format mismatch rule (KNR-3 decision 8,
    open), and a mixed record that only KNR-3 writes. So KNR-3 waits on KNR-2 which waits on
    KNR-3\'s open decision, which waits on the parent session.
20. **Imported units have no stated option rule.** *Reported.* KNR-5 lines 67 to 71 say an
    imported unit\'s presentation options come from the current document source. KNR-3
    decision 3 (no document-scope layering) is stated for records with resolved options;
    no plan says which rule covers imported units.
21. **Two presentation-only options can hide a dependency change.** *Reported.* `fig.cap`
    can be an R expression evaluated after the chunk; the structured profile ignores it for
    the hash, and nothing says what the header layer does with an expression it cannot
    evaluate. `results='hide'` drops a `knit_asis` (and its `knit_meta`) in the presenter
    while the hash ignores `results`; KNR-3 states no test for that.
22. **Parent text edits.** *Reported.* KNR-6 6B says to update EXR-6 and EXR-17
    cross-references; the epic (line 52) says the parent text is not edited again.
23. **KNR-3 decision 2 hides a second parent question.** *Reported.* EXR-4 says flat cell
    keys are the options minus the engine\'s `presentation_cell_options`, while the unit hash
    uses the unit\'s profile. KNR-3 chooses the unit\'s own profile and writes "confirm with
    the parent session", but the epic open-questions list omits it.

### D. Ordering and testability inside KNR-1

24. **The plan order contradicts the real dependency order.** *Reported, consistent with
    the text.* The real order is: corpus manifest and generator; control run (R, capture
    off, existing live path; needs no Phase 1B); normalizer rules written from the control
    run; normalizer and control-run tests; Phase 1B capture and
    `capture_does_not_change_live_output`; Phase 1C mapper (needs S2) and baseline; runner;
    report; 1E. The classifier tests are independent. The plan puts the generator,
    normalizer and control-run tests in 1A ("tests first") while the generator is in 1D, the
    normalizer list is only known after the control run, and the control run has no
    checkbox of its own. `control_run_is_identical_after_normalizer` sits under "No R
    needed" but runs two real R renders. S7 needs the baseline presenter (Phase 1C), so
    "spikes first" cannot finish S7 first. The plan does not say whether S1 fixtures seed
    the corpus or are throwaway.
25. **Tests that cannot be written first.** *Reported.* `pieces_to_nbformat_mapping` needs
    the S2 table; `capture_include_false_and_cache_hit_shapes` pins S1/S4 findings;
    `normalizer_rewrites_preserve_ids_and_temp_paths` has no pass condition until the
    control run; `corpus_generator_is_deterministic` has no manifest format and no meaning
    for "the same seed". The other classifier tests are writable now.
26. **Phase 1D leaves unknowns.** *Reported.* "How Quarto 1 output is gathered is a Phase
    1D decision" (KNR-1 lines 388 to 389) has no checkbox, and the Q1-gap step depends on
    it; where each harness piece lives (R side, `src/`, or a test helper) is unstated.

### E. S1 feasibility

27. *Reported.* S1 has roughly 18 fixtures (several with sub-variants), three candidate
    mechanisms and three knitr versions. Setup that is not a checkbox: a script building the
    scratch library, a harness recording what each candidate sees and misses (a
    fixture-by-mechanism-by-version table is implied but not said), and the fixture
    directory. The decision form is "record all three here" with no named subsection for
    KNR-3 to cite. The 4 to 6 session estimate was made before any spike ran (the plan says
    so); three to four sessions for S1 alone would not be surprising.
28. *Could not verify.* Installed knitr is 1.50 with evaluate 1.0.4. knitr 1.44 predates
    evaluate 1.0, whose piece classes changed, so the scratch library probably must pin
    `evaluate` (and possibly `xfun`) too, or the matrix measures a combination no user has.
    It also needs network access for the CRAN archive.

### F. KNR-3 Phases 3A and 3B

29. *Verified true.* The type names fit EXR-4: `HashDefinition` with `profiles`,
    `HashProfile`, `quarto_hash_rule`, the stale rule for a missing profile, and the
    document hash dropping only keys excluded by every profile used. An empty
    `presentation_execute_keys` on either profile is consistent with that rule (nothing is
    dropped). `Unit`, `CellUnit` and `scan_units` exist as EXR-2 defines them.
30. **Codable from the plan text once S1 to S5 report:** the declaration shell, the
    stale-on-missing-profile and mixed-record hash tests, the alignment test (count mismatch
    gives no record plus a diagnostic), decision 4 on stage-supplied hashes, the content
    hash vectors, the nbformat validator, the chunk-hook fallback marker, and the
    format-independent fallback rows.
31. **Still needs a decision or spike result:** the cache policy (S4), `child=` and hook
    policies (S5; whether the scanner can identify `child=` chunks has no stated owner), the
    resolved-options field name (S3/S8), whether `fig.show` joins the structured list (S1),
    and a closed, one-spelling option list for the structured profile (KNR-3 lines 97 to
    102, deferred to S3; the option-sweep test names no source for "every documented knitr
    chunk option"). Phase 3B says "each with a diagnostic code" but allocates none.
    "Both recorders write behind the same `ExecuteResult` field" needs a field that
    `engine::context::ExecuteResult` does not have (it is added by EXR-7, listed only as
    soft).

### G. What the pending format question blocks (KNR-3 decision 8)

32. Cannot be coded until it is answered: the 3A hash tests (lines 200 to 204; vectors
    depend on whether the format is a hash input), the 3A declaration tests (205 to 209) and
    the 3B declaration (228 to 229), the 3A version test (210), the 3A flat-key adapter tests
    (215 to 217) and the 3B adapter (230 to 231; the `quarto_format` tag, or one record per
    format, which would change the unit set), and the target-dependent rows of the fallback
    table (176 to 185). Not blocked: alignment, serde, the nbformat validator,
    `child=`/cache/hook rows, content hash vectors, mixed-record document-hash tests. KNR-2
    Phase 2F is also blocked on it (item 19).

### H. Repo rules

33. *Reported, partly verified.* The corpus directory, the `tests/integration/main.rs`
    registration and the xtask location fit the rules. Not mentioned by the plans:
    - Every existing R-calling test file starts with `#![cfg(not(target_arch = "wasm32"))]`;
      `main.rs` has no such gate, so new R-calling modules need it.
    - The scratch R library setup script has no language or Windows gate; a shell script or
      a `:` path literal would break the cross-platform rules. A Rust helper using
      `std::env::join_paths`, or a runtime `if !cfg!(unix)` guard, fits. KNR-7 lines 71 to
      73 ("R hidden from the replay engine") needs `join_paths` for the same reason.
    - Allowlist keys and expected strings containing paths must be built with `Path::join`
      (local diagnostics) or forward slashes (wire format).
    - No KNR exit criterion includes the `cargo clippy --target x86_64-pc-windows-msvc`
      cross-check for touched crates.

### I. Claims and estimates

34. The session estimates are called guesses and sum to 20 to 31, matching the table. The
    mixed labeling is the weak spot: the epic says facts marked *verified* were checked on
    2026-10-30, but the 2026-10-31 additions say "reported by review" or "measured in
    review" with no script or log location. KNR-1 lines 48 to 52 (plot alt text, `log.echo`,
    1.44-1.46 cache hits through `fix_evaluate`) and "about 190 lines" for a replacement
    `eng_r` have no cited source.

### J. Layout

35. *Verified.* KNR-6 lines 24 and 25 are the same line twice. (The reviewer also flagged
    KNR-4 lines 41 to 42 and a missing exit-criteria heading in KNR-5; the first is not a
    duplicate, the second was not checked.)

## Unresolved markers, by unblocker

- **Parent session:** the format in the record (KNR-3 decision 8; epic, KNR-2, KNR-6);
  which list produces flat cell keys (item 23, not in the epic list).
- **KNR-1 spikes:** S1 capture mechanism, minimum knitr version, decision 10, `fig.show`;
  S2 `knit_asis` mapping, source order, figure paths; S3 option serialization and field
  names; S4 cache policy; S5 `child=`, hooks, odd chunks; S6 rendered-output seam; S7
  skeleton versus assembly; S8 option state and scope layering; S9 inline mechanism.
- **Gordon:** the acceptable fallback rate (KNR-3 exit); KNR-6 corpus bar and sign-off
  list; how Quarto 1 fixtures are gathered (KNR-1 1D, no named decider).
- **bd-sr73 (open measurement owned by EXR-4):** the figures decision. Who decides once the
  measurement exists, and whether it blocks S2, is not stated.
- **No owner or unblocker stated:** whether the scanner can identify `child=` chunks;
  the spelling-key name for inline units (KNR-4 decision 3); KNR-5 decision 4
  (`execution_count`, "check against S2" although that is KNR-1\'s S2); KNR-5 spike time in
  Gordon\'s fork; the count of existing knitr traces (epic decision 14); KNR-6 decision 4
  ("confirm no consumer hard-fails").

## Verified true

`mod.rs:157` (`execute`) and `:225`; `types.rs:97`; `collect_capture_files` exists
(`engine/capture_files.rs:62`); `knitr_label_reinject.rs:37` starts the helpers;
`engine_output_parity.rs` defines the three helpers; the `patch.R`, `hooks.R` and
`execute.R` line references added on 2026-10-31 check out apart from the `hooks.R` version
list; the knitr spot checks (`block.R:259`, `:313`, `:317-319`, `output.R:295`,
`utils.R:1130`) hold; the mechanism order agrees across the epic and KNR-1; the `execute:`
key rule is consistent across the epic, KNR-2, KNR-3, KNR-6 and KNR-7 (the only gap is
item 20); KNR-4 matches EXR-2, EXR-4 and EXR-12; parent decisions 38 to 40 match.

## Could not verify

Runtime of S1 or the harness; whether knitr 1.44 loads with evaluate 1.0.4; the 1.44 to
1.46 cache-hit path and the `eng_r` size; the "measured in review" claims about htmlwidget
id randomness and `include=FALSE` with `error=TRUE`; the KNR-5 exit-criteria heading.
