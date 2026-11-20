---
title: 'OPR-7d implementability review: could an unattended agent run the plan cold?'
date: 2026-11-10
description: 'Whether an implementer agent starting cold could run OPR-7d to completion and get a correct result: the gate state on the current tip, the harness changes the plan needs, the zip and figure-directory gaps, a flip experiment that failed three tests (one a real regression), the stale-entry contradiction after the flip, the missing stop conditions, and a proposed split into five pieces.'
---

**Plan:** [OPR-7d](../plans/2026-11-08-opr-7d-switch-plain-render.md) at the tip of `feature/one-presenter` (`3d50f2443`,
the 7j claim). **Prior reviews:** [OPR-7d plan review](2026-11-10-opr-7d-plan-review.md),
[OPR-7j plan review](2026-11-10-opr-7j-plan-review.md) and [OPR-7j implementability review](2026-11-10-opr-7j-implementability-review.md).

**Method.** One agent in the lane B worktree (`workspace-4`, branch `opr/7d-implementability-review`). I read the
protocol, the 7d and 7j plans, the epic\'s decisions, the 7g and 7c gate reports, the harness and the manifest, and wrote
my concerns to a scratch file **before** opening any earlier review. Two sonnet sub-agents then ran in parallel (the
user authorized exactly two, overriding the protocol\'s no-sub-agents rule for this task): the *harness agent* owned the
workspace build and ran the gate; the *flip agent* did static analysis only. I re-checked one claim from each (below),
ran the flip experiment myself after the harness agent finished, and only then marked each finding new or known.
Marks: (read), (ran), (relied) for a sub-agent claim I did not re-check. Review only: I did not implement 7d, flip a
status, sign an entry, edit the allowlist or manifest, or open a strand.

## Verdict

**Not runnable cold as written; runnable after the proposed edits, and it should be split into five pieces.** The
design is sound and the gate state matches the plan\'s claim (the four cases it allowlists are the four that differ
today). But six things would stop or mislead an unattended agent, in order of cost:

1. After the flip, the plan\'s own stale-entry check (trap 3) fails: both sides present from the record, so the
   normalizer and the pin fire zero times. That also contradicts "the interleaving entry is permanent" (finding 12).
2. The flip makes three existing tests fail, and one is a **real regression**: a user-written `#| quarto-unit-id:` option
   aborts a plain knitr render with "Duplicate map key" (finding 17).
3. `docx`, `pptx` and `epub` do not skip in the harness: they land under "both routes failed" with a UTF-8 error, so a
   gate that lists them would look clean while comparing nothing (finding 8).
4. The gfm difference is larger than the plan says (plain writes a figure file, the recorded route inlines base64), and
   the fix is a naming decision the plan leaves to the implementer (finding 9).
5. The plan says every box is `[unattended]` and also that Gordon signs four entries and decides the fate of any Julia
   target class that fails. Both need Gordon mid-run, and there are no stop conditions or handoff (findings 22 to 24).
6. It is far more than one session of work (finding 25).

I applied five mechanical edits (end of report). Everything that changes a design or a gate is proposed text, for Gordon.

## Findings

Each carries a mark and **new** or **known** (in an earlier review).

### A. The gate on the current tip

1. **The Julia gate matches the plan (ran by the harness agent; I read its report file; new as a measurement).**
   `OPR_PARITY_REPORT=<file> OPR_PARITY_ONLY=julia/ OPR_PARITY_KNITR=0 cargo nextest run -p quarto-core -E
   'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture`: 11 identical, 1 allowlisted
   (`julia/docs-index-chunk`, by the signed `julia-code-leading-newline`), 4 blockers, 1 both-failed (`julia/engine-error`,
   expected), 0 skipped. The blockers are exactly `julia/capture-facts`, `julia/fig-subcap`, `julia/presentation-attrs`
   and `julia/records-plot`. `julia/positron-cache` is identical and `docs-index-chunk` no longer differs by the script
   lines, so 7i closed the Plots-script cases. Test time 101.9 s (2 min 20 s with the compile).
2. **`records-plot` and `presentation-attrs` are interleaving only (ran).** The whole diff is two plain lines and four
   recorded lines: a code block split in two, each with a copy-button scaffold. Nothing else. Known in kind (7d plan
   review, item 11); new as a measurement on this tip.
3. **`capture-facts` has four kinds of difference, not three (ran, 51 diff lines).** `data-prompt="true"` on the plain
   cell div (absent recorded), the `&gt; ` prompt prefix, the numeric `echo` (plain shows `a = 1` and `b = 2`), and the
   `cell-output-stderr` blocks (`@info` and `println(stderr, ...)` kept, `@warn` dropped). The plan\'s decision 5 names
   "the prompt marker and the prefix", which can be read to cover `data-prompt`; the 7d plan review (item 12) lists it.
   **Known**; the pin must still contain all 51 lines.
4. **`fig-subcap` differs by 7j plus interleaving (ran).** The harness report truncates the diff, so the harness agent
   re-rendered both routes and diffed in full: missing panel ids, float classes and the `aria-describedby` wrapper, panel
   captions without "Figure 2:", the lost group caption, and `?fig-two-1?` references. All are 7j\'s. The one remaining
   difference is the later code block between panels, which the normalizer must handle **on the page after 7j lands**:
   I could not prove that (finding 24). Known (7j reviews); the interleaving residue is new.
5. **The report test asserts nothing (read, ran).** `opr_parity_report` passed with four blockers. The plan\'s "zero
   unallowlisted differences" and its trap 3 ("fails the report test") need a new assertion, gated on
   `OPR_PARITY_REPORT`. **New.**

### B. The harness changes

6. **Schema (read; new in detail).** `scope` is a `starts_with` prefix and the signed `scope = "knitr"` relies on that to
   cover `knitr-corpus/...`, so exact-name scoping needs a **new optional field** (`document`), mutually exclusive with
   `scope`; the pin is a second optional field, a TOML `'''` literal string, valid only with `document`. `RULES` is a
   `&[&str]` and `apply_rule` returns no counts, so normalizers need a second table (a function from HTML to HTML plus a
   fire count) run after the regex rules (trap 6). `Verdict::Allowlisted { rules: Vec<String> }` derives `Eq` and one
   test compares it by value, so the shape change touches that test.
7. **The pin needs an unclipped, ordered diff (read; new).** `describe_difference` is a multiset diff clipped to 8 lines
   per side and 240 characters, so a pin compared against it would hide everything past line 8. A changed pin is a
   `Blocker` printing both texts; a pin on a document whose diff is empty is a stale entry.
   **`scraper` has no source spans (read, relied),** so trap 1\'s "splice touched subtrees back into the original text"
   is not possible with it; the plan\'s other option (round-trip both sides first, report a blocker when raw texts differ
   but round-tripped texts do not) works. `lol_html`, a normal dependency, preserves untouched bytes; nobody tried it.
   `scraper` itself is usable: a normal dependency under `cfg(not(wasm32))` (`crates/quarto-core/Cargo.toml:152`, read)
   and already imported by two integration tests. `cargo clippy -p quarto-core --all-targets -- -D warnings` is clean on
   the tip (ran).
8. **Zip formats misclassify silently (read; ran by the harness agent with a scratch entry, reverted; new).**
   `COMPARABLE` (`opr_parity_runner.rs`, about line 79) is consulted only in the knitr-corpus loop of `plan_jobs`; the
   manifest loop pushes every listed format. `render_prepared` does `read_to_string` on the output, and a docx, pptx or
   epub is not UTF-8, so both sides fail with the same message and the case lands under "both routes failed at execution",
   flagged "NOT expected", not under "not compared". `Side.text` is a `String`, so zip support is a new extraction step
   (`word/document.xml`, the pptx slide XML, the epub xhtml, relationship ids stripped). `zip` is a dev-dependency already
   used by three integration tests (`Cargo.toml:250`, read; no new dependency). The 7d plan review (item 5) knew the
   knitr-only use of `COMPARABLE`; the silent misclassification is new.
9. **gfm is more than a directory (ran by the harness agent on `julia/records-plot`; new).** Plain writes
   `doc_files/figure-gfm/cell-4-output-1.svg` as a markdown image. The recorded route writes **no figure file** and inlines
   `<img src="data:image/png;base64,...">`, besides the split code fence. typst: plain writes `figure-typst/...svg`,
   recorded writes `figure-html/...svg`, plus the interleaving split. So the plan\'s "normalize the name for gfm in the
   presenter or in the harness" does not cover the Plots case (a different format and a different delivery), and which
   one to build is a decision. Known: the directory (7j reviews); new: the inlining.
10. **The figure-directory fix (read; ran by the harness agent only as a grep; partly new).** One production hardcode:
    `presenter/figures.rs:228` (`format!("{}/figure-html/{file_name}", ...)`, re-read by me). `FsFigureSink::new` and
    `FigureNaming::new` take no format. Production callers, all with a context at hand: `engine/ts_record.rs:541`,
    `engine/ipynb_replay.rs:234`, `engine/replay.rs:136`, `engine/jupyter/text_execute.rs:657` and `:2150`, about 15 test
    callers. A second hardcode, `transforms/hephaestus.rs:107` (`ARTIFACT_DIR = "figure-html"`), was not examined.
    A grep finds **zero** tests or goldens that name `figure-html` under a non-HTML format; the hits are html or
    format-less tests (a heuristic count, not a census: only a failing run settles it). Names (read): Quarto 1 writes
    `figure-` plus `pandoc.to` with the `+` or `-` suffix stripped (`render.ts:20-26`), so gfm is `figure-commonmark`; q2\'s
    knitr passes `pandoc.to = ctx.format.as_str()`, so it writes `figure-gfm`, `figure-typst` and `figure-revealjs` (read,
    not ran). The decider is R plus `build_format_config`, not Rust.
11. **Manifest and fixtures (read; new).** Five non-HTML Julia classes need a fixture that is **not in the tree**: the
    "review\'s hand-run document" (a `println`, a labeled figure, a labeled table, a bare expression) lives only in an earlier
    session\'s scratch. Nothing under `opr-parity/julia/` or `claude-notes` has it. The existing `julia/fig-subcap` table
    renders as literal pipe text on both routes, so it does not exercise a table. Manifest entries need `name`, `dir`,
    `file`, `requires = ["julia", "plots"]` as needed, `formats` and a non-empty `note` (asserted by
    `the_plans_own_manifest_parses_and_names_real_documents`).
12. **After the flip the stale-entry check fails (read, by the plan\'s own design; new).** Trap 3 says an entry needed by no
    document in its scope fails the report test. After the flip both sides present from the record: the interleaving
    normalizer fires 0 and 0, and the `capture-facts` pin\'s diff is empty. The plan also says the interleaving entry is
    "permanent" and that the harness stays as a regression test. These three statements cannot all hold.
    **Recommendation:** the four entries are *gate-time artifacts*. The flip commit removes them from the allowlist,
    keeps the normalizer code and its unit tests, and the post-flip test is the identity check (the same HTML and figure
    set with and without `execute: record: true`, over the Julia manifest, Julia-gated) with an empty allowlist; the
    pre-flip report stays as the evidence. The alternative, freezing the plain-route HTML as goldens, collides with the
    `committed-render-output` lint (only `claude-notes/plans/<name>-investigation/` may hold rendered HTML). The 7d plan
    review (item 1) knew the harness changes role; the stale-entry failure is new.
13. **The seeded-allowlist test (read; known, with a wrong name in the plan).** The test is
    `the_seeded_allowlist_parses_and_each_entry_is_proposed_or_signed` (`opr_parity_runner.rs`), not `..._every_entry_is_proposed`.
    It asserts exactly three entries, the signed rule names and the "Signed: Gordon, 2026-11-09" text, and it passes today:
    7e\'s "8 versus 6" note is stale (fixed in `9ef13b0cd`, count set to 3 by 7f). Edited in the plan. Which tests change:
    the test above, the one that compares `Verdict::Allowlisted { rules: vec![...] }` by value, the `allowlist_parse_rejects_*`
    pair (extend for `document` and `pin`), `the_report_names_every_class` (per-entry columns) and the manifest test.

### C. The flip, the routing and the binary tests

14. **Callers of `EngineExecutionStage::new()` (read; ran by me in the experiment).** Three production calls
    (`pipeline.rs:398`, `:986` in `insert_capture_splice_stage_in`, `:1921` in `parse_qmd_to_ast`); none of
    `with_present_from_record` in production; one test sets the flag (`engine_stage_driver.rs:150`); 47 unit tests in
    `engine_execution.rs` build their own stage with `new()`. `insert_capture_splice_stage_in` does rebuild with
    `new().with_spliced_engines(...)` and would lose a builder-set flag: the plan is right. **The flip-agent\'s alternative:**
    leave `new()` at `false` and set the flag where the render pipelines are built. The plan\'s flip in `new()` works too,
    but only with the driver fix (finding 17). Known (7d plan review, item 3); the 47-test census is new.
15. **Production recorders also take their stage from `new()` (read, relied; new).** `engine/preview_record.rs:169` and
    `:252` build their stages from `build_html_pipeline_stages_with_options`, so a flipped `new()` turns the flag on for the
    preview and hub recorders, including the legacy `record_capture` path. `want_record` is true for them (`preview_record.rs:151`,
    `trace.rs:310`, `observer.rs:243`), so presenting is probably unchanged; **not checked** whether every recorder path sets
    `want_record`. The plan says only "the flip is a no-op there".
16. **Julia\'s target gate (read, relied; new in detail).** The list belongs in `offers_presentation`
    (`ts_record.rs:660`, today `ctx.builds_record() && !ctx.units.is_empty()`), whose only production caller is
    `ts_engine.rs:1087`. The format is `ctx.format` (a `String`, `context.rs:90`), set from `FormatIdentifier::as_str()`:
    lowercase and canonical, no `+extensions`, no `html5` or `epub3` spellings. About 60 identifiers exist (`html`, `pdf`,
    `docx`, `pptx`, `epub`, `typst`, `revealjs`, `dashboard`, `gfm`, `commonmark`, `odt`, `rst`, `markdown`, ...). `latex` and
    `beamer` never reach the engine ("Unknown format", `format.rs:445`). Knitr\'s `route_for_target` strips `+ext` and matches
    `STRUCTURED_TARGETS` (`html`, `html4`, `html5`, `revealjs`, `epub`, `typst`, `docx`, `gfm`), so `pptx` is uncovered: the
    plan is right not to reuse it. **The plan\'s "a class graduates by removing it from the list" implies a deny list, which
    would present every unlisted identifier (`odt`, `rst`, `dashboard`, `commonmark`) with no parity case**, the opposite of
    "no case means stays on the engine route". It must be an **allowlist** (graduation adds a line). Also, the gate in
    `offers_presentation` changes `--trace`, `record: true` and preview behavior for gated Julia targets (the record is still
    built and then discarded): state the intent or apply the gate only when `!want_record`. **New.** I did not trace what a
    `pdf` render does.
17. **The flip experiment (ran by me; new).** In `EngineExecutionStage::new()` I changed `present_from_record: false` to
    `true` and nothing else, ran `cargo nextest run -p quarto-core -p quarto --no-fail-fast` (270 s), then
    `git checkout -- .` and confirmed a clean tree. **9096 tests run: 9093 passed, 3 failed, 38 skipped.**
    - *Expected meaning change (2):* `engine_record_stage::no_record_is_built_when_observer_does_not_want_one` (asserts
      `builds == 0`) and `engine_record_stage::legacy_capture_payload_is_byte_identical_with_and_without_record` (asserts
      `builds == usize::from(wants)`). Their driver `run_stage_inner` only replaces the stage when its flag is true, so
      otherwise it keeps the `new()` stage, which now presents and builds. Fix: the driver sets
      `.with_present_from_record(false)` explicitly. The plan does not name them.
    - *Real regression (1):* `knitr_chunk_records::knitr_chunk_id_is_not_emitted_as_an_attribute`. The document has a chunk
      option `#| quarto-unit-id: abc123`; on the flipped plain route knitr aborts: `Error in yaml::yaml.load(...):
      Duplicate map key: 'quarto-unit-id'`, so the render fails. The record path injects the same key. The test passes
      before the flip (7g\'s clean `quarto-core` run, relied). The recorded route (preview, `record: true`) has the same
      defect today; the flip exposes it to plain render. The test name says the intent: a user-written reserved key must
      not break a render and must not leak. A fix needs either rejection with a clear diagnostic or dropping the user
      key; either is a decision, so stop and report (proposed text below).
    - *Unrelated:* none in this run. The known `vendored_reveal_assets_match_npm_package` failure did not appear.
    The `quarto` crate had no failure.
18. **New plain-render diagnostics and failures (read, relied; new).** The knitr info line prints only when
    `ctx.want_record` (`target_route.rs:95`, pinned by `a_fallback_caused_only_by_present_from_record_is_quiet`), so the
    plan is right that the fallback is quiet. Newly reachable on a plain render and not in the plan: the knitr child and
    hook-override info lines (`knitr/mod.rs:352-372`, fed by `result.pieces`, non-empty when capture is on); unit-count
    mismatch warnings and skeleton notes (`chunk_records.rs:221-251`, `knitr/mod.rs:431-435`); `engine_record_diagnostic`
    warnings on a record-build failure (`ts_record.rs:645-651`); `present_from_record` returning an error when core asked
    to present and no record exists (`ts_record.rs:690-705`); and `coalesce` errors becoming `PipelineError` "execution
    record: ..." (`engine_execution.rs:896-898`), which can now fail a render that passed. Nothing is dropped.
19. **`.quarto` and marimo (read, relied; new in part).** A record file is written only when the document sets
    `execute: record: true` (`engine_execution.rs:920`); `want_record` or `present_from_record` alone writes nothing. Engines
    write nothing under `.quarto`; the presenter\'s figures go to `<stem>_files/`. For a **single-file render `.quarto` may
    not exist**, so "the sorted `.quarto` listing equals the `--no-execute` listing" can be empty equals empty: add a
    positive assertion, or use a project render. Marimo is a TypeScript engine: `offers_presentation` has no engine-name test
    (D8), so it is offered `presentedBy: "core"` and avoids presenting only because it does not acknowledge. It also builds a
    record (EXR-9), so after the flip every plain marimo render builds one and may print `engine_record_diagnostic` warnings.
    HTML stays identical; stderr might not. The plan says "byte-identical" and means HTML.
20. **WASM (read, relied; new).** `EngineExecutionStage` is not `cfg`-gated, so it runs in the browser. `ReplayEngine` returns the
    recorded record and the stage then runs `coalesce`, which was previously only on `want_record` and can fail
    (`record/coalesce.rs:55-66`, `UnknownCell` and `DuplicateCell`). So the WASM leg is a new runtime path for replay
    renders, not only a compile check. I did not verify it in a browser.
21. **The binary tests (read, relied; known in part).** `no_execute_cli.rs` has private `Q2_BIN` and `q2_render`; there
    is no Julia or knitr helper. `r_available` (`knitr_corpus_run.rs:19`) and `julia_available` (`julia_engine_e2e.rs:59`) live
    in `quarto-core`\'s test binary, unreachable from `crates/quarto`; copy them (`quarto-util` is already a dependency of
    `quarto`, `Cargo.toml:41`). CI (`test-suite.yml`: ubuntu and macos, Node 24, uv, a Jupyter venv with `python3`) has no R
    and no Julia, so **no CI-runnable test fails before the flip**. The CI-reachable guards are marimo (`marimo_engine_e2e`,
    gated on Node and uv) and native Jupyter or stored `.ipynb`, which do not discriminate the flip. The first local test: a
    Julia document with `println("a")` then `x + 1` has one `.cell-code` on the plain route and two inside one `div.cell` on
    the recorded route (inferred from code and finding 2; nothing ran for this); it fails before the flip, passes after.

### D. Chains, lint and Windows

22. **Preview chain (read, relied; known in part).** `hub-client/package.json` has `build:wasm` (`node scripts/build-wasm.js`);
    `cargo xtask build-q2-preview-spa` exists (`crates/xtask/src/main.rs`). Needs Node 24 (`.nvmrc`), `npm install` from the
    repo root, nightly Rust with `rust-src`, `wasm-bindgen-cli`, `wasm-opt` and Homebrew LLVM on macOS. The `wasm32` and
    `x86_64-pc-windows-msvc` targets are installed here. "What a person saw" cannot happen unattended: the box should say
    "not seen by a person" and hand that to Gordon.
23. **Windows cross-clippy and lint (read, relied; known).** `aws-lc-sys` (`Cargo.lock:377`) cannot cross-check from macOS
    (7g report, line 63), so an unattended agent would treat the box as blocking. `cross-platform.md` allows saying so in the
    PR; the plan should say so, as the 7j implementability review proposed. `cargo xtask lint` checks
    `add-file-with-id`, `config-path-base`, `external-sources` and `metadata-as-str` per file, and the flip trips none of
    them. New fixtures risk `committed-render-output` (no `X.html` beside a tracked `X.qmd`).

### E. Process

24. **Collision and dependency (read, ran; new).** The plan names lane D, `workspace-6`. On 2026-11-10 that tree was on
    branch `knr/knitr-pieces-hook-memo` with an untracked file (ran). 7j was `in-progress` in lane C. 7j changes
    `options.rs`, `cell.rs`, the figure arm, the figure count and panel ids, so every fact in findings 4 and 24 about the
    `fig-subcap` page **depends on 7j**. Edited in the plan: the start condition and a clean-tree check.
25. **Size (judgment).** The plan holds a normalizer with seven negative controls and six traps, a pinned entry kind, zip
    extraction, five new Julia classes plus a fixture, a figure-directory fix across about 20 call sites, a target list, a
    flip with 47 tests to read, a harness conversion, about eight binary tests, the end-to-end run and the preview chain. That
    is not one session. Proposed split below.
26. **Gordon mid-run (read; new).** Every box says `[unattended]`. Needed from Gordon: signing four entries
    (`proposed` to `signed`); the figure-directory decision (finding 9); the decision for any Julia class that fails
    (graduate, gate, or fix); the decision for the `quarto-unit-id` regression; and the "seen by a person" step of the
    preview box. Without a stop-and-hand-off rule, an agent either guesses or marks them done.
27. **No stop conditions (read; known for 7j, new here).** The 7i plan has them; 7d has none. Proposed text below.

## The earlier reviews: new, known, and disagreements

**Already in earlier reviews:** the gate must run before the flip and the harness becomes a regression test (7d review,
items 1 and 4); the flip site and the three `new()` calls; the quiet knitr fallback; the `capture-facts` pin and its kinds;
the normalizer form, the traps and the negative controls; `scraper`; native Jupyter takes no flag; the preview and WASM
no-op; the figure directory being presenter-wide and the manifest formats (7j reviews); the Windows `aws-lc-sys` limit (7j
implementability review); the CI limits.
**New here:** findings 3 (the pin must hold all four kinds, the plan names three, partly known), 5, 6, 7 (clipped diff,
no source spans), 8, 9 (inlined PNG), 11 (no fixture in the tree), 12, 14 (the 47-test census), 15, 16 (deny list versus
allowlist, `--trace` side effect), 17 (three failures, one real), 18, 19 (empty equals empty, marimo stderr), 20, 24, 26, 27.
**Disagreements:** (1) the 7d plan review calls the WASM and preview effect "a no-op"; the flip agent found a new `coalesce`
runtime path in the browser (finding 20), which I have not verified, so treat the disagreement as open. (2) The 7d plan
review (item 12) says `capture-facts` has three kinds of difference; I count four with `data-prompt` as separate, a
wording question more than a disagreement. No disagreement on the direction of the verdict.

## Proposed plan text (not applied; each changes a design or a gate)

**1. Split OPR-7d into five pieces, in order.** Each piece is its own branch and landing, and ends with a short handoff.
- *7d-1, harness machinery (no production code).* Normalizer table, `document` and `pin` entry fields, per-side fire
  counts, the unclipped ordered pin diff, the report assertion, all tests and negative controls. Gate: harness unit tests
  and clippy. Does not need 7j. Entries are written `proposed`.
- *7d-2, non-HTML measurement.* Zip extraction in `render_prepared`, the fixture, five manifest classes, the figure
  directory decision and fix. **Needs Gordon first** for the directory and gfm-inlining decision. Gate: the per-class
  table.
- *7d-3, Julia target allowlist.* The data-driven list in `offers_presentation`, graduating the classes 7d-2 proved.
  Depends on 7d-2.
- *7d-4, the gate.* Needs 7j and 7i landed and 7d-1 to 7d-3 landed. Runs the harness with the new entries; reports.
  **Needs Gordon to sign the four entries.** Ends with the report.
- *7d-5, the flip.* The one-line flip with the driver fix, the 47-test read, the three known failures resolved, binary tests,
  end-to-end, the preview chain, harness conversion, `status: complete`. Needs 7d-4 signed.

**2. Stale entries after the flip (finding 12).** Replace "the interleaving entry is permanent" and trap 3\'s
post-flip behavior with: "The four interleaving entries exist for the pre-flip gate. The flip commit removes them from
the allowlist (the normalizer code and its tests stay). The post-flip harness test asserts that a render with
`execute: record: true` equals a plain render over the Julia manifest and the knitr corpus, with an empty
interleaving allowlist, and says so in the allowlist header and the runner doc."

**3. Target list (finding 16).** "`offers_presentation` consults one data list of presented Julia targets (an allowlist).
A target not in it stays on the engine route, quietly. A class graduates by adding its identifier and its parity case.
State whether the gate applies under `want_record`."

**4. The `quarto-unit-id` regression (finding 17).** "Stop before the flip if
`knitr_chunk_records::knitr_chunk_id_is_not_emitted_as_an_attribute` fails: report it to Gordon. Do not weaken the test. A
fix is a separate decision (reject the user key with a diagnostic, or drop it)."

**5. Zip formats (finding 8).** "A manifest format with no extractor fails the harness loudly (`not compared` or an error),
never as 'both failed'. First red test: a `.docx` output becomes a `Side`."

**6. Stop conditions.** Stop and report (commit progress, a "Notes from an earlier attempt" section, status `ready`, a
handoff) when: a dependency is not `complete`; `workspace-6` is dirty or on another branch; R, Julia or Plots is absent
(the gate and Julia tests cannot run: do not skip them silently); the gate shows an unallowlisted difference other than
the four; a Julia class fails parity or its figure names differ in a way the plan does not decide; the flip fails any test
beyond the three named and the 47 reviewed; the Windows cross-clippy fails in a build script (record and go on, as the 7j
review proposed); an entry is `proposed` and the next box needs it signed; disk full.

**7. Handoff and Gordon steps.** "At each of 7d-1 to 7d-4, end with a handoff (protocol, 'Ending your session') naming
the next piece. Gordon must sign the four entries, decide the figure directory and gfm inlining, and decide any failing
Julia class. These boxes are `[Gordon]`, not `[unattended]`."

**8. The `.quarto` test (finding 19).** Add a positive assertion, or test on a project render, so the comparison is not
empty equals empty. Say the marimo check covers HTML and stderr.

## What I did not check

- The implementation of any piece, clippy beyond `-p quarto-core --all-targets` on the tip, and `cargo xtask lint`.
- The knitr part of the gate (the run used `OPR_PARITY_KNITR=0`), the WASM gate and the WASM build, the Windows cross-clippy,
  and the preview chain.
- How 7j changes the `julia/fig-subcap` diff once it lands, and whether the normalizer handles that page.
- Whether every preview and hub recorder path sets `want_record` (finding 15), what a `pdf` render does, the render manifest,
  freeze, listings, search and the profile checkpoint as readers of engine output, and the bodies of the 47 unit tests.
- Whether `transforms/hephaestus.rs:107` matters for non-HTML formats, and whether `lol_html` works for the normalizer.
- The `figure-html` census is a grep heuristic. The (relied) claims in findings 14 to 16, 18 to 21 and 23 come from the flip
  agent\'s static reading; I re-checked the `engine_stage_driver.rs` behavior (by reading and by the experiment) and
  `figures.rs:228`, and the harness agent\'s `zip` dependency and the `COMPARABLE` use, but not the rest.

## Plan edits applied (mechanical only)

1. The "Start condition" opening: it said `blocked-on-gordon` until signed; the status is `ready` and only `depends-on`
   holds the plan back (stale status text). It now says to read statuses from the integration branch and stop on a missing
   dependency.
2. "Branch and land": added a `git status` and branch check, with the `workspace-6` collision (a missing command).
3. The "Tests first" box: the seeded-allowlist test now has its real name, says what it asserts, drops the stale "8 versus 6"
   note and names the two files (a wrong name and a stale fact).
4. The "Gate before the flip" box: added the harness command, the tools it needs, its runtime, and that the test asserts
   nothing (a missing command).
5. The "Flip the default" box: added the three failing tests found by the flip experiment (a missing count and list).

## Addendum: the gfm and pptx figure difference is a presenter bug (investigated after the first landing)

Finding 9 said the gfm difference was larger than a directory name and its cause unknown. I traced it (ran, on the
`julia/records-plot` fixture, with a binary built from the clean tip; an earlier attempt used a binary left flipped by the
flip experiment, which made both routes look identical, so I rebuilt).

- **The engine output is the same on both routes.** The recorded cell holds one output with `application/pdf`,
  `image/png`, `image/svg+xml` and `text/html` on gfm, epub, pptx, html and revealjs; typst and docx get `image/png` and
  `image/svg+xml` only (the Julia runner chooses per target).
- **The presenter picks by one fixed order.** `format_outputs_with` (`presenter/cell.rs`) tries `text/html` before the image
  branch whatever the target. Quarto 1 (`display-data.ts:45`, `displayDataMimeType`) uses the target: markdown targets put
  images first and `text/html` last; other non-HTML targets drop `text/html` unless it holds a table. A faithful Rust port
  already exists (`display_data_mime_type` in `presenter/inline.rs`, with a shared-fixture test), but only inline spans call it.
- **Effect by target.** gfm: plain writes `figure-gfm/cell-4-output-1.svg`; recorded inlines a base64 PNG in an `<img>`.
  **pptx: plain has `ppt/media/image1.svg`; recorded has no media, the figure is missing** (the raw html block is dropped by
  pandoc), so the flip would regress Julia pptx. docx: media identical on both routes (svg and png). typst: the same
  figure file on both routes, only the directory differs (`figure-typst` against `figure-html`). epub: one PNG on both routes
  (html-compatible, so `text/html` is right). Not run: odt and the other long-tail formats.
- **The fix** (recorded as decision 6 and a checklist box in the 7d plan): thread the `InlineTarget` the presenter
  already carries into `CellOptions`, default to the HTML target so every existing result is unchanged, and take the
  `text/html` branch only when the target allows it. Native Jupyter changes on non-HTML targets for bundles with both
  `text/html` and an image; Gordon decided that is a separate bugfix, not an exception to the epic. The figure directory
  becomes `figure-` plus the format identifier (Gordon agreed, 2026-11-10).
- **Not checked:** the `hephaestus.rs` `figure-html` constant; whether LaTeX-flag targets exist in this tree; the
  unflipped behavior for odt and other formats.

Plan edits applied after the first landing: decision 6 and the "Presenter: display priority and figure directory" box were
added to the 7d plan, and item (3) of the non-HTML box now points to it (a decision recorded at Gordon\'s instruction; the
other proposed text above, including the split, the stale-entry rule and the `quarto-unit-id` diagnostic, is still not applied).
