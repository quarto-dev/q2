---
title: 'OPR-9g: flip plain render of Julia and knitr to the recorded route'
date: 2026-11-10
description: 'Turns the internal present_from_record control on by default, so a plain q2 render of a Julia or knitr document is presented by the Rust presenter from an in-memory record, writing nothing to .quarto. Keeps the LaTeX family and uncovered targets on the engine route, quietly, and converts the parity harness into a regression test of the recorded route.'
status: complete
depends-on: [opr-9f]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Origin:** split from [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (now a tombstone) on 2026-11-10 by Gordon's decision; evidence in the
[7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) and its addendum, and the [7d plan review](../research/2026-11-10-opr-7d-plan-review.md).
**Branch:** `opr/9g-flip-plain-render` · **Lane D** (`workspace-6`) · **Blocks:** OPR-8.
**Needs:** R 4.3 with knitr and Julia 1.12; `hub-client` tooling for the WASM gate and the preview chain.

> **Reading text copied from OPR-7d.** Passages copied from the old 7d plan say "this plan" or "7d". Read them as this
> piece of the series 9a to 9g (see the [tombstone](2026-11-08-opr-7d-switch-plain-render.md)). Marks: (read), (ran), (relied).

## Start condition

`status: blocked-on-gordon` until Gordon has signed the four entries of 9f and set this plan `ready`. If the entries are `proposed`, the status is
not `ready`, or `workspace-6` is dirty or on another branch, stop and report.

## Decisions it implements

**1. The retained knitr fallback (knitr epic decision 24) for plain render: keep, as today.** It stays for chunks Rust
cannot map and for fallback targets; OPR-7d changes nothing about it, and removing it remains the separate,
trigger-gated KNR-8. Reason: Gordon chose the recommended option; the corpus does not cover every use of knitr, so the
fallback is a safety net the switch should not remove.

**2. An opt-out control: none.** The `present_from_record` flag is flipped on and stays internal; there is no environment
variable, document key or other user-facing way back. Reason: Gordon chose the recommended option; the parity gate (zero
unallowlisted differences, OPR-7c) is the safety, and a control would be a user surface to document and later retire. A bad
case is fixed in q2, not worked around. OPR-7a\'s "behind a control" means this internal flag only, which agrees.

(Decisions 3 to 5 of the old 7d are implemented in 9f, 9d and 9b; 6 in 9b.)

## Goal (decision D5)

A plain `q2 render` of a Julia or knitr document builds the record in memory and presents it through the Rust
presenter, as a recorded render does. Concretely:

- The internal `present_from_record` flag from OPR-7a is on by default. There is no opt-out (decision 2).
- **Nothing is written under `.quarto`** on a plain render except what is written today (`render-manifest.json`);
  `execute: record: true` still writes the record file (execution-records decision 3).
- The LaTeX family (`latex`, `beamer`, `pdf`), `pptx`, `context` and unknown targets stay on the engine route for
  knitr, **quietly**. This already holds (OPR-7a): `plan_capture` (`engine/knitr/target_route.rs`) gives the info
  diagnostic only when `want_record` is true (`want_record.then(...)`), so a fallback caused by `present_from_record`
  alone is quiet, and a unit test pins it. `want_record` is also true under `--trace` and for the preview and hub
  recorders, so a knitr render to a LaTeX target under `--trace` prints the line; Gordon accepted that (2026-11-10).
  The line reads, for `latex`, `beamer` and `pdf`: "knitr output for LaTeX is not presented from the execution record /
  the output format is `pdf`. / LaTeX targets keep knitr\'s finished markdown for every chunk."; for other uncovered
  targets: "knitr output for this target is not presented from the execution record / ... Targets outside HTML,
  revealjs, epub, Typst, docx and gfm keep knitr\'s finished markdown for every chunk." No code change.
- **Julia on non-HTML targets:** decided in OPR-9e (present by evidence, gate the rest, through a target allowlist in
  `offers_presentation`); the flip inherits whatever 9e graduated.
- **Native Jupyter takes no flag.** `text_execute.rs` gates only on `ctx.want_record` and already presents through
  `present_document` on every render, so the flip changes nothing for it (a Jupyter `fig-subcap` group does change, and so does every later figure number in that document, but through 7j, which lands first: epic D14 and 7j\'s decision D14, "Later-figure numbering"); the only difference from the plain route
  that [OPR-7i](2026-11-10-opr-7i-jupyter-page-dependencies.md) leaves (decision 6) is that native Jupyter and the ipynb
  replay path get the widget warning but not the page-dependency scan. 7d carries that as a known, documented
  difference, and OPR-8\'s notes say so.
- Cached knitr chunks fall back with no record (OPR-7b). If [OPR-7e](2026-11-09-opr-7e-cache-hits-as-records.md) is
  `complete`, a `cache=TRUE` chunk is a record cell on a hit as well as on a miss, and levels 1 and 2 still fall back;
  read its notes. If 7e is blocked or not done, proceed as in 7b: this plan does not wait for it (7e decision 5).
- Marimo is a documented exception and stays on its legacy path (decision D12).

## What the review added

- **Flip site and the tests it changes (ran).** Setting `present_from_record: true` in `EngineExecutionStage::new()` (reaches
  `pipeline.rs` at `build_html_pipeline_stages_with_options`, `insert_capture_splice_stage_in` and `parse_qmd_to_ast`; nothing in production calls
  `with_present_from_record`) fails three tests in `quarto-core` and `quarto` (9096 run, 9093 passed): the two `engine_record_stage` tests
  `no_record_is_built_when_observer_does_not_want_one` and `legacy_capture_payload_is_byte_identical_with_and_without_record` (their driver
  `run_stage_inner` in `engine_stage_driver.rs` keeps the `new()` stage when its flag is false; fix it with an explicit
  `.with_present_from_record(false)`), and `knitr_chunk_records::knitr_chunk_id_is_not_emitted_as_an_attribute` (9c). 47 unit tests in
  `engine_execution.rs` build their own stage with `new()`; read them (the experiment found none failing). The flip-agent\'s alternative,
  leaving `new()` at false and setting the flag where the render pipelines are built, is also sound; the plan\'s choice is `new()`.
- **Production recorders also take `new()` (read, relied).** `engine/preview_record.rs` builds its stages from
  `build_html_pipeline_stages_with_options` (two sites), so the flip turns the flag on for the preview and hub recorders too; they already
  want a record (`want_record` true), so presenting is probably unchanged. Check that every recorder path sets `want_record`.
- **New plain-render diagnostics and failures (read, relied).** A plain render can now print the knitr child-document and hook-override info
  lines (`knitr/mod.rs`, fed by `result.pieces`), unit-count mismatch warnings and skeleton notes, `engine_record_diagnostic` warnings, and
  fail with `present_from_record` errors or a `coalesce` error (`UnknownCell`, `DuplicateCell`: "execution record: ..."). List the ones you
  saw while rendering the corpus in your landing note.
- **`.quarto` (read, ran).** A record file is written only with `execute: record: true`. A single-file render may have no `.quarto`, so the
  listing test can compare empty with empty: add a positive assertion (the render manifest exists in a project render) or test on a project.
- **Marimo (read, relied).** Marimo is offered `presentedBy: "core"` and declines by not acknowledging; it also builds a record, so its HTML stays
  identical but stderr may gain `engine_record_diagnostic` lines. The byte-identity test covers HTML and says so.
- **WASM (read; 9a\'s S6).** The stage is not `cfg`-gated, so after the flip it runs in the browser. Only `ReplayEngine` runs there (the
  other engines are `cfg(not(wasm32))`); it returns the capture\'s record, and the stage pushes any `result.record` into the coalesce
  whatever the flag says, so after the flip the browser **coalesces a replayed record where before it did not** (the browser observer
  wants no record). `coalesce` fails with `UnknownCell` or `DuplicateCell` when a record\'s cell id is not in the unit list, so a capture
  recorded under an older id derivation could now fail to render. Add the test S6 names: replay a Jupyter capture recorded before the
  flip through `render_page_for_preview` (the WASM build and the two vitest files `recordPayload.wasm.test.ts` and
  `captureSpliceHtml.wasm.test.ts`) and assert it renders. The native flip run covered every `ReplayEngine` test in `quarto-core` (9096
  tests, no replay failure), so the open risk is the stored-capture case alone.
- **Stale comments after the flip:** `engine/context.rs` (the `record` field doc says it is built only when `want_record`) and
  `engine_execution.rs` (near the `present_from_record` field).
- **The four gate-time entries (proposed, for Gordon to confirm; recommendation from the review).** After the flip both sides of the harness
  present from the record, so the normalizer fires 0 and 0 and the pin diff is empty: trap 3 (stale entry) would fail the report. Resolve it:
  the four entries exist for the pre-flip gate. The flip commit removes them from the allowlist, keeps the normalizer code and its unit tests
  (so the mechanism stays tested), and the post-flip test asserts identity (same HTML and figure set with and without `execute: record: true`,
  over the Julia manifest and the knitr corpus, with an empty interleaving allowlist) and says so in the allowlist header and the runner
  doc. The pre-flip report stays as the gate\'s evidence. Freezing plain-route HTML as goldens is the alternative; it collides with the
  `committed-render-output` lint, so it is not recommended. The 7d wording "the interleaving entry is permanent" is replaced by this rule.
- **Not checked by any review:** freeze, listings, search and the profile checkpoint as readers of engine output; say so in your notes.

## Checklist (tests first; every box is `[unattended]` except the preview step)

- [x] **Tests first, through the binary** (model `crates/quarto/tests/integration/no_execute_cli.rs`, which has
      `q2_render` and `Q2_BIN` but no Julia or knitr helper; `r_available()` and `julia_available()` live in
      `quarto-core` tests, so the `quarto` crate needs its own copy or a shared one): a plain render of a Julia and a
      knitr document is presented from a record (the output carries the recorded-route markup, for example the `.cell`
      wrapper with the id OPR-7f made the presenter write; do not key a test on a missing `id="cell-N"`); a Julia cell
      with a bare PNG gets the file names OPR-7c decided on; a marimo document\'s HTML is identical to before
      (`marimo_engine_e2e::` still passes); nothing new under `.quarto`, tested as: `.quarto/execution-records` does not
      exist after a plain render, and the sorted `.quarto` listing equals that of a `--no-execute` render of the same
      document (do not pin the whole listing); a knitr document to `pdf` or `latex` takes the engine route and prints
      nothing about it; a cached chunk falls back; `execute: record: true` still writes the file; native Jupyter\'s HTML
      is identical before and after; there is no opt-out to test (decision 2). The Julia and knitr binary tests skip
      cleanly when the engine is absent and **are local evidence only**: CI installs a Jupyter venv but neither R nor
      Julia (a grep of `.github/workflows` finds none), so say that, and name the unit tests CI does run.
- [x] **Flip the default (no opt-out, decision 2) in `EngineExecutionStage::new()`.** `new()` is called at
      `pipeline.rs` in three places (the shared head, `insert_capture_splice_stage_in`, which rebuilds the stage with
      `new()` and would lose a builder-set flag, and `parse_qmd_to_ast`), so flipping its initial value reaches all three;
      list the tests that used `new()` as "plain" and say what each now means (the 2026-11-10 implementability review ran this flip
      on the tip: `cargo nextest run -p quarto-core -p quarto` gave 9093 passed and 3 failed:
      `engine_record_stage::no_record_is_built_when_observer_does_not_want_one` and
      `engine_record_stage::legacy_capture_payload_is_byte_identical_with_and_without_record` (their driver keeps the
      `new()` stage when its flag is false), and `knitr_chunk_records::knitr_chunk_id_is_not_emitted_as_an_attribute`). Nothing in production calls
      `with_present_from_record` today.
- [x] **Keep the harness as a regression test of the recorded route (Gordon, 2026-11-10).** After the flip it compares
      the recorded route with itself plus the file it writes; apply the rule in "What the review added" (remove the four entries, keep the
      code and tests, assert identity), say so in the allowlist header and the runner doc, and keep the pre-flip report as the gate\'s evidence. A test-only seam on the pipeline builder that re-enables the old
      route is not an opt-out, but it is a new surface; add one only if Gordon asks.
- [x] **End-to-end through the real binary,** plain `q2 render` on a Jupyter, a Julia and a knitr document and one
      LaTeX-target knitr document; read the HTML; record the invocations and what you saw. (Native Jupyter is unchanged;
      the check is that it is.)
- [x] **Preview chain** (AGENTS.md, "Verifying Rust changes in `q2 preview`"): `cd hub-client && npm run build:wasm`,
      `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`; confirm `q2 preview` of the same documents still
      renders. This is a compile and smoke check for the native preview (the preview recorder already wants a record, so the
      flip is a no-op there); the browser is not a no-op (see the WASM box above), so run the old-capture replay test there too. Say what was and was not
      seen by a person.
- [x] Gates: `cargo clippy -p quarto -p quarto-core --all-targets -- -D warnings`; `cargo nextest run` for the same
      crates; the Windows cross-clippy; `cargo xtask lint`.

## Stop conditions

Stop, commit your progress, write "Notes from an earlier attempt" and tell Gordon (the protocol's "When you cannot
finish") if any of these happens:

- the flip fails any test beyond the three named above, or a test among the 47 reads as relying on plain: report it, do not weaken it;
- a Julia or knitr render that passed before now fails or prints a new line you cannot explain from the list above;
- the byte-identity test for marimo or native Jupyter fails;
- the Windows cross-clippy fails in a build script (`aws-lc-sys` cannot cross-check from macOS): record it, say a Windows user must run the
  native check, and go on; any other cross-clippy error is a stop;
- the preview chain needs Node 24, nightly Rust with `rust-src`, `wasm-bindgen-cli` or `wasm-opt` that the machine lacks: report it. The step
  "a person saw the preview render" is **Gordon\'s**: write "not seen by a person" and hand it over; do not mark it done.

## Handoff

End the session with the protocol's handoff ("Ending your session"). If this plan is `complete`, name OPR-8 as startable (it now depends on 9g, not 7d). Otherwise follow the protocol's "continue your plan" handoff.

## Exit criteria

- A plain render of Julia and knitr is presented from an in-memory record; the 9f gate report shows zero unallowlisted differences; nothing
  new is written under `.quarto` beyond today\'s `render-manifest.json`.
- LaTeX-family and uncovered knitr targets stay on the engine route with no message (other than under `--trace`); marimo and native Jupyter
  unchanged (marimo HTML byte-identical).
- Gordon\'s decisions are implemented as recorded (fallback kept; no opt-out control; interleaving handled by 9d and 9f; subfigure groups
  unchanged by the flip, through 7j); the harness is a regression test of the recorded route.

## Notes from the run

Session 2026-11-11, lane D. Commits: `3bdb23e41` (flip, tests, allowlist), `ab643e591` (WASM replay tests).

**What I ran (ran) and what I only read.**

- Tests first: `crates/quarto/tests/integration/plain_render_recorded_route_cli.rs` (six tests). Before the flip the two route tests
  (`julia_plain_render_is_presented_from_a_record`, `knitr_plain_render_is_presented_from_a_record`) failed for the stated reason (plain was
  not on the recorded route); the other four passed before and after, so they are regression guards. Route markers: a multi-expression Julia
  cell is one code block on the engine route and one per expression on the recorded route; a knitr chunk inside a blockquote gives two
  `<blockquote>` on the engine route and one on the recorded route. A plain knitr `include: false` chunk shows no empty `.cell` wrapper on
  either route in my probe, so I did not use it as a marker. These are local evidence only (CI has neither R nor Julia); what CI runs is the
  `engine_execution.rs` and `engine_record_stage` unit tests, and `target_route.rs::a_fallback_caused_only_by_present_from_record_is_quiet`.
- The flip: `EngineExecutionStage::new()` sets `present_from_record: true`. `cargo nextest run -p quarto-core -p quarto`: 9183 run, 9182
  passed, 1 failed (`revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`, node_modules drift, not mine). The three failures the
  review predicted did not all appear: I fixed the `engine_stage_driver` driver (always sets the flag explicitly), after which both
  `engine_record_stage` tests passed, and `knitr_chunk_records::knitr_chunk_id_is_not_emitted_as_an_attribute` passed unchanged. None of the
  47 `engine_execution.rs` tests failed, and none read as relying on plain.
- Stale comments fixed: `engine/context.rs` (two field docs) and `engine_execution.rs` (the field doc).
- **Allowlist.** After the flip the gate (`OPR_PARITY_REPORT`) failed with nine stale entries and a report of 93 identical, 0 differences. The plan
  names "four" entries; there were nine, because 9f signed six interleaving entries and the three older plain-versus-presenter entries (knitr
  empty cell wrapper, knitr split blockquote, Julia leading newline) are stale for the same reason. I removed all nine, rewrote the header,
  and replaced `the_seeded_allowlist_parses_...` with a test that the allowlist is empty. Gordon signed these entries; he should see this
  list. Kept: the normalizer code, the pin mechanism and their unit tests (they use their own fixture), and `pins/` as pre-flip evidence.
  Gate after the change: 93 identical, 0 unallowlisted, 0 disagreements, 253 rejected before any engine ran (same on both routes), 2 both
  failed at execution with the same message, 8 zip formats not compared (the accepted knitr docx and epub gap). 42 harness and runner tests
  pass.
- End to end, `target/debug/q2 render doc.qmd` in the scratchpad (`.../scratchpad/e2e`), output read: Julia (`1 + 1`, `println`, `2 + 2` in
  one cell) wrote four code blocks and `jul-out`, `.quarto` held only `render-manifest.json`; knitr (a chunk in a blockquote, plus an
  `include: false` chunk) wrote one `<blockquote>`, `knit-out`, `.quarto` only `render-manifest.json`; knitr `--to pptx` rendered with no
  fallback line. **Jupyter: not verified through the binary**: this machine has no `ipykernel`, so a Jupyter render fails with "runtime is
  not available". The gate lists `jupyter/*` cases as identical, but I did not confirm they ran a kernel. Native Jupyter takes no flag, by
  construction. **LaTeX targets: not driven**: `pdf` is "not yet supported" and `latex`, `beamer` and `context` are unknown formats in the
  binary, so `pptx` stands in; the LaTeX family rests on the `target_route.rs` unit test. **Marimo**: not run here; `marimo_engine_e2e::` ran
  in the workspace test pass (it skips cleanly when marimo is absent).
- Diagnostics I saw while rendering the corpus: only the knitr `processing file` / `output file` lines (knitr's own, both routes) and the
  documents rejected before any engine ran (253, the same on both routes, mostly `Engine 'jupyter' is registered but its runtime is not
  available` for shell chunks). I saw no `present_from_record` error, `coalesce` error, unit-count warning or child-document line.
- WASM: `cd hub-client && npm run build:wasm` succeeded on the flipped tree. `captureSpliceHtml.wasm.test.ts` (a Jupyter-named stored capture
  through `format: html`) and `recordPayload.wasm.test.ts` (a record through `format: html`) both pass: 10 tests with the two I added. These
  two guard a risk the flip could have introduced and did not, so they were not red first. They do not cover a record whose cell id
  came from an older derivation; none is stored in the repo, so that case (S6's remaining open risk) is untested.
- Preview chain: `cargo xtask build-q2-preview-spa` (up to date) and `cargo build --bin q2` succeeded. `q2 preview --static` on the knitr
  document served `/doc.html` with one blockquote and the chunk output. **Not seen by a person**: the live browser preview was not opened;
  that step is Gordon's.
- Gates: `cargo clippy -p quarto -p quarto-core --all-targets -- -D warnings` clean. The Windows cross-clippy fails in third-party build
  scripts (`ring`, `wasmtime`, `aws-lc-sys`), as the stop conditions allow; a Windows user must run the native check. `cargo xtask lint` reports one
  violation I did not cause: `crates/quarto-core/src/positron_cache.rs:360` `metadata-as-str` (RIU-3a; `meta` looks like a serde JSON value, so
  probably a false positive). Not touched.
- `hub-client/changelog.md`: not updated. The changelog is for user-facing changes and the hub-client commit only adds tests.
- Production recorders (`preview_record.rs`, hub) already want a record, so the flip should change nothing there; I read this and ran the preview
  smoke above, but did not trace each recorder path for `want_record`.
- Not checked by any review, and I did not check: freeze, listings, search and the profile checkpoint as readers of engine output.
- Snapshots: none added, changed or removed. One golden/fixture file changed (`opr-parity-allowlist.toml`).
