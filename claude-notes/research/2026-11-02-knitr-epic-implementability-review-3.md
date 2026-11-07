---
title: "Knitr execution-records epic: implementability review 3 (test design and runtime behavior)"
description: "Third read-only review of KNR-3 (from 3C) to KNR-7: whether the tests the plans ask for would prove the right thing, and how the finished system behaves on real knitr documents."
---

# Scope and method

- Branch `review/knr-implementability`, **reviewed commit `60ba0e077100a5a7dfef2f79cf5e0f8337722d27`**
  (worktree clean at the start).
- In scope: KNR-3 (from 3C), KNR-4, KNR-5, KNR-6, KNR-7, the epic and the interlude. KNR-1 and KNR-2
  are a fixed contract; items waiting on unlanded KNR-2 work are marked "depends on KNR-2 (item)".
- Four decisions settled 2026-11-02 (`known_options`, classic spans stay live, one `present_record`,
  `text/x-quarto-markdown` marker) are not reopened.
- Two sonnet sub-agents reported: A (test design) and B (runtime behavior). The lead re-read the
  cited lines for every "blocks" and "risk" finding. "Verified" means the lead or the sub-agent
  opened the cited line; "reported" means code reading without a line the lead re-opened. Nothing
  was run except one R experiment by B (finding D12). No tests were run.
- Paths: `P/` is `claude-notes/plans/2026-10-30-`; `T/` is `crates/quarto-core/tests/`.

# Update after discussion (2026-11-02)

Gordon settled all four decisions below after the review, and the plans were edited
(uncommitted until this commit): decision 1 as option (b) (epic decision 24, new deferred plan
[KNR-8](../plans/2026-10-30-knr-08-fallback-removal.md)); decision 2 follows from it (a missing
record falls back to the shipped markdown for the whole document); decision 4 as option (a), with
an engine-neutral key `quarto_file_path` (epic decision 25). Decision 3 (gate acceptance): option a, epic decision 26. `knit_meta` is
rebuilt in Rust (decision 27); a block-level span output keeps knitr\'s behavior for now (decision 28). Brace spans may return rich values (an htmlwidget, an image), which S9 missed; see D17.
A1, A2 and B5 below are resolved by those edits; the text is kept as written at review time.

# Ready to start

| Plan | Verdict | Change since review 2 |
|---|---|---|
| KNR-3 (from 3C) | Ready after A1 (shadow-gate definition, see A2 below), B1 (`known_options` content) and review 2 A1; 3C also depends on KNR-2 (present-from-record path) | A2 and the marker text are applied; new blockers are gate definition and the `known_options` union |
| KNR-4 | Ready after the wire type is moved above "Tests first" (review 2 B3, reconfirmed D5) and fixtures are named (D6) | A3 and A4 applied; the spelling key is gone |
| KNR-5 | Ready after the importer gets a caller (EXR-16) and the `outputs_for` fallback parameter is specified (D11); 5C needs Gordon\'s Positron fork | B4 (mime marker) applied |
| KNR-6 | (At review time:) blocked on decision A1 (who produces markdown when Rust cannot present a chunk) and A2 (what a missing record does after the flip), plus the gate and oracle definition (A3); also EXR-15, bd-js24dold, KNR-2, KNR-3, KNR-4 | A5 and A6 still unwritten |
| KNR-7 | Not startable by design; needs the code numbers (review 2 B2, plus C3 below) and named fixtures and observables (D13) | unchanged |

# A. Blocks starting a plan

**A1. (Resolved by epic decision 24.) KNR-6 6B: Rust cannot fall back after R has returned a marker. (verified)**
- KNR-6 lines 56-57 and 102 say structured chunks return a marker and fallback chunks return real
  markdown; KNR-3\'s table decides which. But the decision is also made in Rust, after R finished:
  `crates/quarto-core/src/engine/knitr/pieces_map.rs:96-99` (`Other` piece returns `None`),
  `:116-127` (`image()` returns `None` for gif, webp, tiff, pdf, or an unreadable file), and
  `crates/quarto-core/src/engine/capture_files.rs:40-55` (files outside the document directory are
  skipped, so `include_graphics("../assets/x.png")` is unreadable).
- R has already returned the marker, and the plans say the finished markdown is not shipped for
  structured chunks. KNR-6 6C line 174-176 keeps the hook "narrowed to fallback chunks".
- KNR-3\'s table (lines 306-315) has no "unmappable output" row.
- Plan change: KNR-6 6B must say either that R decides every fallback (it can check extension and
  directory, and must be told which), or that R also ships the finished markdown for every chunk
  (then 6C line 174-176 must not narrow the hook), or that a Rust-side fallback triggers a second R
  run. KNR-3 adds the "unmappable output" row.

**A2. (Resolved by epic decision 24.) KNR-6: a missing record leaves a marker-only document. (verified)**
- KNR-3 decision 4 (line 129) and S5 say a count mismatch gives no record, a diagnostic, and a
  successful render. That is true only while the live markdown is untouched.
- `crates/quarto-core/src/engine/diagnostics.rs:111-115` says whether a problem may fail the render
  after the flip "is decided in that engine\'s flip task". KNR-6 never decides it.
- Plan change: KNR-6 must decide: fail the render, or re-run R with markers off.

**A3. KNR-3 3C and KNR-6 6A: the shadow gate is undefined and disappears at the flip. (verified)**
- The classifier (`T/integration/knitr_corpus_classify.rs:79-93`) renders to HTML only, with a fixed
  host document, while fixtures run on `html`, `typst`, `docx`, `gfm` (`manifest.toml:524-568`).
  A typst-only or docx-only difference vanishes in HTML and is classed Identical or Semantic.
  KNR-3 line 415 and KNR-6 line 141 say only "modulo the classified differences".
- The normalizer (`T/integration/knitr_corpus_normalizer.rs:1-35`) has two rules, but
  `normalize_figure_paths` replaces every `figure-<fmt>/<name>` with `FIG<n>`; reused for
  record-versus-live it hides exactly the figure placement bugs the tests target.
- 6C deletes the recorders and the live path (lines 174-179), so after the flip nothing is left to
  compare against; `run_fixture(.., capture: bool)` (`knitr_corpus_run.rs:61`) loses its "off" arm.
- Plan change: KNR-3 3C and KNR-6 6A state the accepted classes (Identical and SyntaxLevel, or the
  signed-off allowlist), require per-format comparison for non-HTML formats, restrict the normalizer
  to widget ids plus a position-and-extension check for figures, say whether the 3C test is promoted
  to the 6A gate (one harness, not two), and commit frozen oracle markdown per fixture (an
  old-trace fixture too) before 6C. Those files are snapshots; the AGENTS.md duty applies (D4).

**A4. KNR-3 3B: `known_options` is under-specified and would forward internal options. (verified)**
- KNR-3 lines 103-105 define the list as `names(knitr::opts_chunk$get())`. The live R path removes a
  larger set before forwarding unknown options: `crates/quarto-core/src/engine/knitr/resources/rmd/hooks.R:470-505`
  has `knitr:::opts_chunk_attr`, `quarto_knitr_opts`, `quarto_opts` (`label`, `panel`, `layout*`,
  `code-fold`, `output`, ...), `other_opts` (`quarto-unit-id`, `class.*`, `attr.*`, `indent`,
  `fenced.echo`, ...) and names starting with `.`.
- A presenter that subtracts only `opts_chunk$get()` forwards all of these as cell attributes.
- Plan change: KNR-3 3B defines `known_options` as that union (R supplies the knitr part, Rust adds
  the Quarto constants and the dot rule), and says whether `quarto-unit-id` is dropped at capture.
  KNR-2 (fixed contract) must treat the list in this sense: "KNR-3 must write what KNR-2 reads".

# B. Wrong references

- **B1. KNR-3:405-407 "next free numbers after Q-18-3" is wrong.** The catalog holds Q-18-1 to 5 and
  Q-18-10 (`crates/quarto-error-catalog/error_catalog.json:1920-1955`); Q-18-6 to 9 are claimed by
  other plans. The first free code is Q-18-11. The existing record-dropped diagnostic has no code
  (`diagnostics.rs:140`). KNR-3 3B lists the codes in one table. Adds to review 2 B2. (verified)
- **B2. KNR-4 line 107, "live output identical with recording on and off", is dead after the flip.**
  The existing test (`knitr_corpus_control.rs:75-118`) passes by construction while capture is
  additive and has no "off" arm once recording is unconditional (review 2 C1, D6). It cannot fail
  first. Replace it with record-present-versus-live equality on the inline fixtures. (verified)
- **B3. KNR-6 6C line 174-179 deletion list is incomplete.** Missing: `engine/knitr/chunk_record_tests.rs`
  (`mod.rs:103`), `chunk_records.rs` and `span_records.rs` (which also own `insert_unit_ids` and
  `SpanPlan::new`), the in-crate assertions on `result.chunks` (`types.rs:470,542`), and the R side
  (`quarto_chunk_records`, `quarto_span_records`, `quarto_flush_span_records` at `execute.R:216,275-276`,
  `hooks.R:35-45,163,626`). The only reader of `KnitrExecuteResult.spans` is `mod.rs:323`. Stored-record
  consumers (trace `stage/trace.rs:278`, hub `execute.rs:511-538`, `encode_capture_payload`) go through the
  generic record, so they are unaffected. (verified)
- **B4. KNR-6 6B and the legacy capture path.** `stage/stages/capture_splice.rs:332-345` splices from
  `result.markdown`; hub and preview use that path whenever the record gate is off
  (`capture_files.rs:302-318`). After the flip a skeleton in `result.markdown` splices garbage. 6B must
  require final text while the gate exists, or flip the gate in the same commit. (verified)
- **B5. (Resolved by epic decision 25.) Figure names and directories change at the flip with no sign-off.** The shared sink hardcodes
  `<stem>_files/figure-html/cell-<n>-output-<m>.<ext>` and writes jpeg as `.jpeg`
  (`crates/quarto-core/src/presenter/figures.rs:111-117,139-161`); knitr names by label, honors `fig.path`
  and `fig.ext`, and uses `figure-<format>`. The corpus normalizes names away
  (`claude-notes/research/2026-11-01-knr-01-corpus-report.md:17`). Add to KNR-6 6A\'s divergence list
  (epic decision 23\'s list lacks it) or make the sink honor `quarto_knitr.file`. Also say whether
  knitr\'s own copies are deleted after Rust reads them, else `_files` holds duplicates. (verified)
- **B6. KNR-7 replay test: `QUARTO_R` and the process-wide R cache.** `find_rscript` honors `QUARTO_R`
  first and caches (`engine/knitr/subprocess.rs:17-18,56-68`); "hidden R" must unset it, not only edit PATH.
  Replay also writes figures next to the document via `FsFigureSink::new(&ctx.source_path)`, so run
  it in a temp directory. (verified for QUARTO_R; reported for the sink)

# C. Inconsistencies

- **C1. Decision text checks out, two leftovers.** `claude-notes/plans/2026-11-01-knr-interlude-spikes.md:259-260,273`
  still describe `quarto_knitr.asis`, `quarto_knitr.raw` and `quarto_unknown_options`; lines 298 and the epic
  (273, 289) supersede them, but the 259-260 passage has no "amended" note. Code has the same stale marker:
  `crates/quarto-core/src/presenter/cell.rs:1062` uses `{"quarto_knitr": {"asis": true}}` (KNR-2 cleanup; KNR-3 3A
  "no `raw` marker" must not collide). (verified)
- **C2. KNR-3 3C and KNR-6 6A are the same test written twice** with different corpora (see A3).
- **C3. KNR-4 lines 56 and 110 say "both spellings"** for the `NULL` rule and the alignment test. After
  decision 2, classic spans are not units; the wording means "the pre-pass handles both" and should say so.
  (verified)
- **C4. Epic and KNR-6 risk 5 (stale caches) are obsolete.** `capture_install` forces `opts_hooks$cache` to
  FALSE (`crates/quarto-core/src/engine/knitr/resources/rmd/capture.R:303`), so every post-flip render runs knitr uncached (review 2 C1).
  KNR-6 exit criteria and KNR-7 user docs need the sentence. (reported)
- **C5. KNR-3 lines 373-376** ("needs EXR-11 merged; until then hand-build one") is stale (review 2 B1).

# D. Risks and gaps

- **D1. Capture-side failures are unguarded (risk, verified).** `capture.R` has no `tryCatch` around
  `capture_options` or `capture_piece` (`jsonlite::serializeJSON`, `deparse`, `conditionCall`) or the wrapper
  bodies (lines 164-175, 195-249). An error there aborts `render()`, where today it succeeds. The one
  `tryCatch` (`capture_select_plots`, 150-159) falls back to unfiltered plots when the non-exported
  `knitr:::rearrange_figs` fails, silently recording plots the live markdown hides (`fig.keep`). KNR-3 3B:
  each wrapper degrades its chunk to a fallback unit with a diagnostic; test a throwing wrapper.
- **D2. Always-on recording cost (risk, verified).** After the flip `want_record` is unconditional.
  `engine_execution.rs:741-757,842-856` coalesces twice and calls `serde_json::to_value(&record)` per render;
  base64 figures are copied about three to four times in a plain `q2 render`. Preview and hub only warn at 10 MB
  gzipped (`capture_files.rs:201-206`). KNR-6 should state whether serialization is skipped when no observer
  consumes the record; KNR-7 docs mention the size warning.
- **D3. No test ties capture to `want_record` (gap, verified).** Production sends `capture: false`
  (`engine/knitr/mod.rs:275`); `insert_unit_ids` and `SpanPlan` must follow the same flag. KNR-3 3B needs one test.
- **D4. Snapshot policy absent from KNR-3 to KNR-7 (gap, verified by grep).** No item says which `.snap` or
  golden files change or that the AGENTS.md duties apply (count, summary, surprises, list). Candidates:
  Jupyter goldens when `panel`, `layout-ncol`, `code-fold` render (KNR-7:42-43), the 6A divergence list,
  the frozen oracles of A3. One sentence in each of KNR-3, KNR-6, KNR-7.
- **D5. KNR-4 wire field defined after its test (gap, verified).** Lines 104-110 require a deserialize test
  for the field that the Mechanism item (last) defines. Reconfirms review 2 B3.
- **D6. Fixture gaps (verified).**

  | Behavior | Fixture today | Plan names one? |
  |---|---|---|
  | `{r}` brace spelling, classic-only doc, another engine\'s spans between knitr\'s | none (`s9/01_inline.Rmd` uses wrapped and classic forms only) | no |
  | Mixed R and Python document | none (`manifest.toml:597-611` has shell and handled languages only) | no |
  | `cache: true` hit | none (every run is a fresh tempdir, `knitr_corpus_run.rs`) | no |
  | `results='hide'` with `knit_asis` | only `s9/01_inline.Rmd:13` | no (review 2 item 21) |

  KNR-3 line 311 ("level-3 hit is a fallback row") and C1\'s forced cache-off have no hit-path coverage; KNR-6
  needs a two-render cached fixture.
- **D7. Hash tests can pass vacuously (gap, verified).** KNR-3:348-351 and 377-379: "an execution-relevant
  option changed has a different hash" holds for every unlisted name (closed-list rule), so it cannot fail first.
  The sweep needs a base value different from knitr\'s default and a statement of which arm can fail; reuse the
  manifest `[[sweep]]` mechanism (`manifest.toml:105+`).
- **D8. No runnable exit commands; silent skips count as green (gap, verified).** The harness has seed 1
  (`knitr_corpus_runner.rs:228`), `KNR_SAMPLE`, `KNR_ONLY`, `KNR_R_LIBS`, `KNR_REPORT`; the plans use none.
  Tests that find no R print `SKIP` and return, so nextest reports PASS. "1.44 and the latest" (KNR-3:345,
  KNR-6:142) needs a scratch-library command and an offline rule. Give the command, expected counts, and report
  skips as skips. Extends review 2 D8.
- **D9. Non-determinism (gap, reported).** State that figure bytes compare by presence and extension; Windows `\`
  in `metadata.quarto_knitr.file` is a wire path (D1 of review 2); `include_graphics("img\\a.png")` is stored as
  typed while the `files` map is forward-slash keyed (`capture_files.rs:48-58`), so lookup misses and the chunk
  silently falls back: normalize with `quarto_util::to_forward_slashes` in the adapter.
- **D10. Replay equality (risk, verified/reported).** If `replay_knitr_record_runs_presenter` calls the same
  `present_record` on both sides, a bug in it is invisible: compare against stored oracle markdown. Include a
  replay with empty `ctx.units` (`engine/replay.rs:113-157` re-scans). The old-markdown-trace fixture must be
  committed before 6C deletes the recorder that produces it.
- **D11. KNR-5 (gap, verified).** The importer has no caller and the gate test "does nothing for a fallback
  unit" needs a new `outputs_for` parameter that no plan states. K-S1 and K-S2 need Gordon\'s Positron fork and
  `q2 preview` (WASM chain). The findings note says data frame, warning and real binary were not observed.
- **D12. Error chunks and spin scripts (gap).** `pieces_to_cell` always sets `ExecuteStatus::Ok`
  (`pieces_map.rs` `CellResult`), even with an error piece; add `error: true` and `error: false` adapter tests
  (verified). knitr also claims `.r` through `ProcessorSpec::Spin` (`engine/knitr/mod.rs:187-192`); no plan says
  what the record\'s source hash means for a spun file; add a test or scope it out (reported). Figure paths
  under `execute-dir` were checked with R 1.50 and are not a problem.
- **D13. KNR-7 e2e (gap, verified).** No fixture or observable is named for figure, table, htmlwidget,
  `error: true` and inline; "record stays fresh, no re-execution" names no counter; hub guest and preview need a
  running hub and the WASM chain: mark manual unless a harness exists. The "corpus shadow gate already proves
  the markdown" premise disappears at 6C (A3).
- **D14. Labels in markers (risk, reported).** The unit id embeds the label (KNR-3:277); a label containing `--`
  or `-->` ends an HTML-comment marker early. KNR-6 risk 1 should require the marker to carry only the index or
  hash.
- **D15. WASM reach (risk, verified by cfg).** `engine/knitr` is `cfg(not(wasm32))`; `presenter` and `record`
  are not, so the generalized `present_record` and `FsFigureSink` reach WASM. It needs a sink parameter, and KNR-3
  must say which side owns the adapter. Extends review 2 D3.
- **D17. Brace spans can return rich values (gap, verified in knitr source).** KNR-4 and S9 record a
  span as `text/plain`, but knitr sends a `knit_asis` inline value through `sew(res, inline = TRUE)`
  (`external-sources/knitr/R/block.R:652`, `output.R:575,679`), so an inline htmlwidget or
  `include_graphics()` works in Quarto 1; the record would lose the mime, `knit_meta` and the file.
  The widget epic (workspace-1, WGT-2) never mentions inline spans. Applied: KNR-4 decision 6 and
  fixtures, KNR-6 decision 6 (span fragments in the retained fallback). Open: where `knit_meta` is
  rebuilt at present time (WGT-2 owns the question), and block-level output inside a paragraph.
- **D16. Exit criteria needing a person (verified).** KNR-3:430 (rate set by Gordon; "reported" is uncheckable),
  KNR-6:129-130 and 190 (signed-off divergence list with no file path), KNR-5 K-S1 and K-S2, KNR-7:91-93 (no
  command text, and it links to the draft EXR-18). Split each exit into command-checkable and needs-Gordon, and
  measure the live baseline first (KNR-4:119-120 does not).

# Conflicts with the units-only draft (conflicts only)

- The draft says core is the only qmd parser and an opted-in engine never sees markdown or a skeleton; KNR-6\'s
  skeleton design is the opposite shape.
- The draft\'s "no fallback to markdown where possible" conflicts with KNR-3 and KNR-6 keeping the fallback hook.
- The draft\'s S3 (core owns `<<name>>` and `ref.label`) and S4 (which options need R) would change KNR-3\'s
  resolved-options decision 11. KNR-6 should not delete `quarto-unit-id` or the hook before the draft settles
  (KNR-6:183-185 is already conditional).

# Check of commit 60ba0e077

The four decisions landed consistently in the plans. Grep over `claude-notes/plans` outside KNR-2 and notes:

- `quarto_unknown_options`: only in the epic (273) and KNR-3 (109), both as "this replaces", and the interlude
  (273, 298; 298 is the supersession note, 273 is the old decision left unamended).
- `quarto_knitr` as a marker: none in plans except `metadata.quarto_knitr.file`, `.knit_meta` (legitimate, KNR-1
  table) and the interlude lines 259-260 (C1). Not cleaned in code: `presenter/cell.rs:1062`.
- `quarto_inline_spelling`, "one line" dispatch: none in the KNR plans (the `one line` hit is an unrelated plan).
- "both spellings": KNR-4:56 and 110 (C3), KNR-1:873 (fixed contract); the other hits are unrelated plans.
- Not applied by that commit, by design: review 2 A1, A5, A6, A7, B1 to B3, B5 to B7, C1 to C5, D1 to D11.

# Disposition of review 2 findings

| Finding | Disposition | Evidence |
|---|---|---|
| A1 (staleness ignores the structured profile) | Still open | KNR-3 has no document-level staleness item; 60ba0e077 touched none |
| A2 (`known_options` name, marker text) | Applied; content now A4 above | KNR-3:103-105, epic:273 |
| A3 (classic spans live) | Applied | KNR-6:98-100 |
| A4 (spelling key) | Applied | grep: no `quarto_inline_spelling` |
| A5, A6 (KNR-6 settled directions not written) | Still open | KNR-6 6B/6C still provisional (A1, A2, B4 above) |
| A7 (dependency table) | Still open | epic table unchanged |
| B1 (EXR-11 "landed") | Still open | KNR-3:373-376 (C5) |
| B2 (Q-codes) | Still open; new evidence | B1 above |
| B3 (wire type named late) | Still open | KNR-4:104-110 (D5) |
| B4 (mime marker in KNR-5) | Applied | KNR-5 edited in 60ba0e077 |
| B5 to B7 | Still open | not touched by the commit |
| C1 (cache-off hook) | Still open; new evidence | C4, D6 |
| C2 to C5 | Still open | not touched |
| C6 (child/hook rows vs units-only draft) | Still open | draft conflicts above |
| D1 to D11 | Still open | D9, D15 and D8 extend D1, D3, D8 |
| 2026-10-31 item 21 (`results=hide` vs `knit_asis`) | Still open | D6 |

# Decisions for Gordon (status after discussion)

*Settled: 1 (option b), 2 (follows from 1), 3 (option a), 4 (option a, key `quarto_file_path`).*

1. **Who produces markdown when Rust cannot present a chunk (A1, A2).**
   - (a) R decides every fallback and ships markdown only for those; Rust-only failures (unreadable or
     out-of-directory figure) become hard diagnostics.
   - (b) R always ships finished markdown for every chunk next to the record; Rust falls back per chunk.
     Costs the deletion of the recorder, keeps the old path alive.
   - (c) A Rust fallback triggers a second R run with markers off.
   - **Recommend (b)** for the flip release, narrowing to (a) once the corpus shows no Rust-only fallback.
2. **Missing record after the flip (A2):** (a) fail the render with a Q-code; (b) re-run with markers off.
   **Recommend (b)**, with (a) under `--strict`.
3. **Shadow gate acceptance (A3):** (a) Identical and SyntaxLevel only; (b) any class on the signed-off
   allowlist. **Recommend (a)**, per format, with frozen oracles committed before 6C.
4. **Figure naming (B5):** (a) the sink honors `quarto_knitr.file`; (b) accept new names and add the divergence.
   **Recommend (a)**.
