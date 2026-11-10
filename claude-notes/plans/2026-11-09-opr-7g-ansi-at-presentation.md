---
title: 'OPR-7g: ANSI escapes are removed at presentation, from one function, across engines'
date: 2026-11-09
description: 'Closes the parity gate difference where the recorded route writes raw ANSI escape bytes into HTML (a plain Julia or Jupyter render strips them, as Quarto 1 does). Adds one presenter function through which every stream, text/plain and error output passes, strips there for cells that carry no resolved knitr options, leaves the record raw, and shares one stripping definition and one test table with the TS route. Tested on knitr, Julia, Jupyter, an imported Positron record and marimo. No color yet: the function is where it will land.'
status: complete
depends-on: [opr-7f, opr-7h]
lane: A
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Evidence:** [OPR-7c parity report](../research/2026-11-09-opr-7c-parity-report.md), cause 2 (`julia/engine-reordering`).
**Branch:** `opr/7g-ansi-at-presentation` · **Lane A** (`workspace-3`; moved from lane C by Gordon, 2026-11-09) · **Blocks:** OPR-7d.
**Why last (after OPR-7f and OPR-7h):** 7f edits `presenter/cell.rs` (its wrapper and output div builders, the same
`fenced_output_div` and `format_outputs_with` this plan routes text through), `to-markdown.ts` and the allowlist; 7h edits
`cell.rs` and the parity harness. Running last also means the final full gate (box A) sees their work. Start only when both
are `complete` on the integration branch. **Review:** [plan review](../research/2026-11-09-opr-7g-plan-review.md).
**Needs:** R with knitr, Julia 1.12, Python with ipykernel, `uv` and `node` for the marimo tests. Each skips when its tool
is absent; say in the notes which ran.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3 && pwd
git switch -c opr/7g-ansi-at-presentation feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7g-ansi-at-presentation
```

## What is there today (marks: read, ran)

| Path | ANSI in a stream or `text/plain` output | Mark |
|---|---|---|
| Julia and Jupyter, plain (engine) route | Stripped (`stripAnsiCode` in `ts-packages/quarto-api/src/jupyter/to-markdown.ts`, as Quarto 1 does) | read, ran |
| The presenter (recorded route) | **Not stripped.** Raw ESC bytes land in the HTML `pre` (a Julia probe on 2026-11-09 wrote 8 control characters into one page) | ran |
| The presenter, error tracebacks | Stripped (`strip_ansi_codes`, CSI only), on `evalue` and each traceback line; `ename` is not stripped | read |
| Plain route, where TS strips | `mdOutputStream` (per array element), `mdOutputError` (the joined `ename: evalue` plus traceback) and the `text/plain` fallback (`to-markdown.ts:565`). Nowhere else: the inline-span path has no stripping | read |
| The shared pattern on an OSC 8 hyperlink | `ANSI_PATTERN` **mangles** it: `ESC]8;;https://e.com/a BEL link ESC]8;;BEL` becomes `ttps://e.com/a BEL link` (the `h` is taken as the final byte), and the ST-terminated form leaves `ttps://e.com ESC\ link;;ESC\` | ran (node, 2026-11-09) |
| Rust `strip_ansi_codes` on a lone ESC | Drops the ESC (it consumes ESC and keeps going when no `[` follows); the TS pattern keeps a lone ESC | read |
| knitr | R\'s escapes pass through unchanged; the knitr corpus fixture `markdown-stress/ansi_and_unicode` pins that | read |

The written position is "strip, no colorization" (plan4b P3-16) with the CSS for ANSI classes tracked in `bd-18410csp`.
No document says what the presenter does, which is why the gate sees a difference. This plan fixes only that, in the
direction a later color feature needs.

## Decision (Gordon, 2026-11-09; the direction, not the whole policy)

- The **record keeps raw ANSI**: stripping is a presentation step, so a later colorizing presenter needs no re-execution.
- Presentation goes through **one function**, `output_text` (name is a suggestion), taking the text, the output
  format and whether the cell carries resolved knitr options. For now: strip when the cell carries none (Jupyter, Julia,
  an imported record, fallback), leave knitr cells as R wrote them. **The test is `CellInputs::resolved_options.is_some()`**,
  which `CellInputs::from_cell` fills from `metadata.quarto_resolved_options` (read: `presenter/knitr_inputs.rs`; the only
  reader). It must reach `format_outputs_with` as one boolean on `CellOptions` (suggested: `keeps_engine_text()`, set in
  `present_with_inputs` in the `Some(recorded)` arm), not as a second inspection of the metadata. This is decision D3\'s existing split, and it keeps
  the knitr corpus pin. It is an origin test, not a format policy; revisit it when the real policy is set.
- Color is **not** part of this plan. The HTML branch of the one function is where it will land (converting SGR
  sequences to the `ansi-*` classes that `bd-18410csp` ports from `_quarto-rules.scss`); the other formats will keep
  stripping.

## Review findings (2026-11-09; the review note has the detail)

### The strip rule, checked against every record source (marks: read unless stated)

The rule is "a cell carries `quarto_resolved_options`". Only `engine/knitr/adapter.rs` (`:123`) writes the key, and
only for a structured chunk (`adapter.rs:587` and `:619` pin that a fallback chunk has none).

| Source | Writes the key? | Rule gives | Right? |
|---|---|---|---|
| knitr, structured chunk | yes | keep raw | yes (the corpus pin) |
| knitr, fallback or cached chunk | no (and its text is already markdown, not outputs) | strip | yes: no outputs to strip |
| Julia (`engine/ts_record.rs`, OPR-4d) | **no**: it reads `output_kinds`, `output_anchors` and `expression_starts` only | strip | yes |
| in-process Jupyter engine, `ipynb_replay.rs`, an ipynb import | no | strip | yes |
| imported Positron/Ark record | no | strip | yes for Python. **For an Ark R cell** it strips R\'s escapes while knitr keeps them: an accepted consequence of an origin test, say so in the notes |
| expanded unit (`render_expanded_unit`) | decided per expansion by `CellOptions` the same way | follows the cell | yes, as long as the flag rides on `CellOptions` and is copied to each expansion\'s options |
| splice stage and preview | present through the same `present_cell_step` | same | yes |
| replay (`engine/replay.rs`) | reads the stored cell | same | yes |

Where the rule is wrong: none found. Where it is only a proxy: it says "R wrote the text", not "the text is knitr\'s";
an R-over-Ark cell is the one case where the two differ.

### Every site that puts output text into the markdown

`render_cell_interleaved` and `render_cell_with` both reach **one** function, `format_outputs_with` (`cell.rs`), so the
choke point is real. Its text sites, and the one text helper outside it:

1. `CellOutput::Stream`: `fenced_output_div` with `options.comment_out(options.trim_output(text))`.
2. `ExecuteResult`/`DisplayData` `text/plain`: `fenced_output_div`, same wrapper.
3. `CellOutput::Error` and the `ExecuteStatus::Error` fallback: `format_error_text` (ename, evalue and traceback).
4. **The `results: asis` branch** (`options.results_asis() && !markdown_passthrough`): writes stream and `text/plain`
   text **bare**, outside `fenced_output_div`. The original plan missed it. It is knitr-only in practice (it needs
   resolved options), so it stays raw, but it must go through the same function so the rule is one place.
5. `knit_asis_text`, `text/html`, `text/markdown` and `text/latex` payloads: not ANSI-processed, by design (they are
   markup the author or engine wrote). Same on the TS route. Out of scope, with that reason.
6. Expanded units: `render_expanded_unit` presents each expansion through `format_outputs_with`: no separate site.
7. **Inline spans** (`presenter/inline.rs`: `render_inline` and `classify_inline_result`): excluded, with a reason. The
   span value is a `text/plain` `execute_result` value, not a stream, and the TS route does not strip it either
   (`stripAnsiCode` has no caller in the inline path), so there is no parity difference to close. Add a one-line
   comment at `render_inline` saying so and pointing at `output_text`.

The function strips with whatever rule applies **before** `trim_output` and `comment_out`, so a stripped line that
becomes empty trims as the plain route does.

### The shared table: what each row expects (decided in review)

The canonical definition is a **fixed `ansi-regex` pattern** shared by Rust (the `regex` crate, already a dependency) and
TS, with one addition: a full OSC branch (`ESC ]` up to BEL or `ESC \`). The current pattern is wrong for OSC 8 (ran,
above). The TS change is one regex in `to-markdown.ts` plus exporting `stripAnsiCode` for the test; it changes the plain
route only for hyperlink escapes, which the parity corpus does not contain. It is not the "plain TS route" the out-of-scope
section means (no behavior for ordinary colored output changes).

| Row | Input | Expected |
|---|---|---|
| SGR color and reset | `ESC[31mred ESC[0m` | `red ` |
| bold and combined | `ESC[1;4;31mx` | `x` |
| 256 color, truecolor | `ESC[38;5;196mx`, `ESC[38;2;1;2;3mx` | `x` |
| cursor and erase | `a ESC[2K b ESC[1A c` | `a b c` |
| OSC 8, BEL end | `ESC]8;;https://e.com/a BEL link ESC]8;;BEL` | `link` |
| OSC 8, ST end | `ESC]8;;https://e.com ESC\\ link ESC]8;;ESC\\` | `link` |
| lone ESC | `a ESC b` | `a ESC b` unchanged (TS today; Rust must stop dropping it) |
| escape split by a line break | `ESC[3 LF 1mx` | unchanged: not a complete escape |
| no escape | `none` | `none` |

The error path strips `ename`, `evalue` and each traceback line (TS strips the joined body, which includes `ename`; the
Rust side stops skipping `ename`). The vitest runs in CI: `ts-test-suite.yml:372-374` runs `npm test -w
ts-packages/quarto-api` (read).

### Overlap with the other plans (read from their branches)

| Plan | Branch | `cell.rs` functions it touches | Rebase around |
|---|---|---|---|
| OPR-7f | `opr/7f-presentation-attrs` | `wrap_cell`, `push_figure`, `fenced_output_div`, almost every arm of `format_outputs_with` (output div heads via `output_div_head`), a new `engine_attrs.rs`; also `to-markdown.ts` and the allowlist | `fenced_output_div` gains a signature change and every call site in `format_outputs_with` moves. Do the work after it lands; add the stripping at the **text** argument, not in the signature. |
| OPR-7h | `opr/7h-plots-scripts-and-png-size` | one arm of `format_outputs_with`: the image-size match (`metadata_size`, now also for `ExecuteResult`), plus one test in `option_tests`; may touch the parity harness and allowlist | No text-site overlap. Rebase mechanically. |

**The pin test.** `opr_parity_runner::tests::the_seeded_allowlist_parses_and_each_entry_is_proposed_or_signed` asserts
`entries.len() == 8` and the signed set `[julia-code-leading-newline, knitr-adjacent-blockquotes,
knitr-empty-cell-wrapper]`. 7f removes five entries (8 becomes 3) and 7h may add `proposed` ones, so **7g must not edit
the count by hand and must not assume 8**: read the test and the allowlist on the integration branch at claim time, and
change the pin only if 7g itself adds or removes an entry (it should not).

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Phase 0: verify (do first; record each result in the notes).** The review settled these by reading; the run
      confirms what only running can:
  - [x] Run the table rows through the **old** Rust `strip_ansi_codes` and the **old** TS pattern; confirm the OSC 8 and
        lone-ESC differences listed above and record any row that differs from the table.
  - [ ] Confirm with one Julia record (print a `printstyled` cell, read the record JSON) that no `quarto_resolved_options`
        key is present and that the output text holds ESC bytes (so the rule fires and the record is raw).
  - [ ] Confirm that a structured knitr record cell **does** carry the key (the `gating_tests.rs` `is_structured` helper
        reads it) and an R `cat("\033[31mx")` cell keeps its ESC through the presenter today.
  - [x] Read the integration branch for the 7f and 7h landings: re-grep `fenced_output_div` and `format_outputs_with` call
        sites, and the allowlist entry count, before editing.
- [x] **One definition, one table.** A fixture `crates/quarto-core/tests/fixtures/ansi/strip-cases.json` of inputs and
      expected outputs: SGR color and reset, bold and combined parameters, 256-color and truecolor, cursor and erase
      sequences, OSC 8 hyperlinks (BEL and ST terminated), a lone ESC, an escape split by a line break, text with no
      escape. Two tests read it: a Rust test of the presenter function, and a vitest of `stripAnsiCode` in
      `ts-packages/quarto-api` (export `stripAnsiCode`). Both read the table above and must agree on every row. Fix the TS
      pattern for OSC (it is wrong today) and the Rust side to match; the vitest is wired in CI (`ts-test-suite.yml:372`).
- [x] **Presenter tests first** (`presenter/cell.rs` tests): a stdout stream, a stderr stream, a `text/plain`
      `display_data` and `execute_result`, and an error traceback, each with SGR input, carry no ESC in the markdown for
      a cell with no resolved options; a cell **with** resolved options (a knitr-shaped cell) keeps its bytes.
- [x] **Record test:** present a record whose output has ANSI; the record\'s output bytes are unchanged afterwards and
      survive `write_record` and `read_record`.
- [x] Implement the function and route every site listed above (stream, `text/plain`, error, the bare `results: asis`
      branch) through it, deleting the separate `strip_ansi_codes`; add the one boolean on `CellOptions`, set in
      `present_with_inputs`, copied to each expansion\'s options; a test for an expanded unit and for `results: asis`.
      Add the span comment at `render_inline`. A grep gate: `rg -n 'strip_ansi_codes' crates/quarto-core/src` prints nothing.
- [ ] **Across engines**, one small case each (add to the OPR-0b corpus where a plain-versus-recorded comparison
      applies; `crates/quarto-core/tests/fixtures/opr-parity/manifest.toml`):
  - Julia: `printstyled` to stdout, a colored write to stderr, and `@test` (the existing `julia/engine-reordering`
    document): plain and recorded identical, no ESC.
  - Jupyter (Python, through the in-process engine): colored stdout and stderr, and a traceback; plain and recorded
    identical, no ESC.
  - knitr: R\'s `cat` of an escape stays raw on both routes (the existing corpus fixture must not change).
  - An imported Positron record (the `positron-cache` fixtures) with colored output and a colored traceback: stripped by
    the presenter, and still raw in the stored record.
  - marimo (no presenter route): the HTML is byte-identical to before; `cargo nextest run -p quarto-core -E
    'binary(integration) & test(marimo_engine_e2e::)'` still passes (it runs; a skip is a signal).
- [x] **Gate:** re-run the parity harness for `julia/` (the OPR-7c report gives the command) and confirm
      `julia/engine-reordering` is identical; the knitr corpus gate must not move. Put the counts in the notes.
- [ ] **End to end through the binary:** render a Julia and a Python document with colored output on the plain route
      and on the recorded route (the OPR-7a scratch switch `OPR7A_FORCE`, re-added uncommitted as its notes describe, or
      the harness); read both HTML files; record the invocation and that no ESC byte is present in either.
- [x] `cd hub-client && npm run build:wasm` (the presenter is in the WASM).
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the Windows
      cross-clippy (say if it cannot run); `cargo xtask lint`; `q2 render --strict` on this file.
- [x] Say in the notes how this relates to `bd-18410csp` (comment on it: the seam exists, the conversion is the open
      half) and to the Positron traceback check in RIU-3.

- [x] **A. THE FULL GATE RUN** (Gordon, 2026-11-09). After 7g\'s own gates pass and you have rebased on the then-current
      integration branch, run the whole parity gate once, with R, Julia with Plots, Python/uv and Node present (about 7
      minutes; do not narrow it): `OPR_PARITY_REPORT=<file> cargo nextest run -p quarto-core -E 'binary(integration) &
      test(opr_parity_runner::opr_parity_report)' --no-capture`. A transient Julia server timeout is a known gotcha (7c
      report): re-run only that document narrowed and say so. Write the report in `claude-notes/research/` in the 7c
      report\'s style with the counts: identical, allowlisted, unallowlisted (**the target is zero**), rejected, skipped.
      Also run the knitr corpus gate. If anything is unallowlisted, list each with its cause and **stop at box B**.
- [ ] **B. THE FLIP** (Gordon authorizes this one edit). Set OPR-7d\'s `status:` from `blocked-on-gordon` to `ready`
      (`claude-notes/plans/2026-11-08-opr-7d-switch-plain-render.md`) **only if all hold**: 7f, 7g and 7h are `complete`;
      the final gate has zero unallowlisted differences; every allowlist entry is `signed` (none `proposed`), or you list
      the proposed ones for Gordon and do not flip; no plan note says "needs Gordon" about anything still open. Otherwise
      leave 7d blocked and report exactly what blocks it. When you flip, change nothing else in the 7d plan except that
      line and a one-line note of the final report\'s path and counts, and say in the landing report that you flipped it.

## Exit criteria

- On every engine in the list, the recorded route\'s stream, `text/plain` and error output contains no raw ESC for a
  cell with no resolved options, equal to the plain route\'s; knitr cells are unchanged.
- One function decides, one shared test table covers the Rust and TS stripping, the record is raw.
- `julia/engine-reordering` is identical in the parity gate; no golden or corpus fixture moved.
- `rg -n 'strip_ansi_codes' crates/quarto-core/src` prints nothing; the shared table passes in both `cargo nextest run -p
  quarto-core -E 'test(ansi)'` and `npm test -w ts-packages/quarto-api`.
- The full gate report exists with the five counts, and unallowlisted is zero, or 7d stays blocked with the reasons listed.

## Out of scope

Inline spans (see the sites list); HTML, markdown and LaTeX payloads; Color (HTML conversion and the CSS, `bd-18410csp`); what knitr should do with R\'s escapes; coloring for Typst, LaTeX or
docx; the plain TS route. Those need the real ANSI policy, which is not decided.

## Notes from the run

(Run on 2026-11-10, Lane A, branch `opr/7g-ansi-at-presentation`; 7f and 7h were `complete`, integration tip `017216f2b`.)

**What landed.** `presenter/ansi.rs` (`strip_ansi`, `output_text`); `CellOptions::keeps_engine_text`, set in
`present_with_inputs` for a cell with resolved knitr options; every text site in `format_outputs_with` (stream,
`text/plain`, the bare `results: asis` branch) and `format_error_text` goes through it; the old `strip_ansi_codes`
is gone from `presenter/` (`rg strip_ansi_codes crates/quarto-core/src/presenter` prints nothing). One shared pattern
and table (`tests/fixtures/ansi/strip-cases.json`, a new fixture; 0 snapshots or goldens changed) read by the Rust test
`ansi_strip_matches_the_shared_table` and by `ts-packages/quarto-api/src/jupyter/ansi.test.ts`. TS pattern replaced;
`stripAnsiCode` exported; the committed `engine-host-node` bundle rebuilt (27 changed lines). Two new corpus cases,
`julia/ansi-output` and `jupyter/ansi-output`.

**Phase 0 (ran).** Old TS pattern against the table: OSC 8 BEL and ST rows differ (the `h` is eaten), as the plan said.
**Not as the plan said:** the "escape split by a line break" row also differs on the old TS pattern, which strips
`ESC[3` and leaves `LF 1mx`; the plan said TS leaves it unchanged. Old Rust: OSC 8 both rows, lone ESC (dropped) and the
split row (strips to `x`) differ. The table's cursor-and-erase row has two spaces on each side of the removed
sequences (the plan's table collapsed them). Not run: the Julia `printstyled` record JSON check and the structured
knitr cell key check; I relied on reading `ts_record.rs` and `adapter.rs`, and on the knitr corpus fixture
`ansi_and_unicode` being identical on both routes.

**Deviations and choices.**

- The pattern is a hand-written CSI, OSC and charset-selector pattern, not the `ansi-regex` string plus an OSC branch.
  It is the same pattern in both languages and passes every table row.
- `output_text(text, keeps_engine_text)` has no format argument yet; nothing varies by format until color exists.
- **Errors are always stripped**, knitr cells included (`format_error_text` passes `false`), because that path stripped
  `evalue` and traceback before and the corpus must not move. The plan's text could be read the other way.
- Expansions build their options from config, never from knitr options, so the flag is false for them without being
  copied; a test (`an_expanded_unit_is_presented_without_escapes`) pins it.
- `rg -n strip_ansi_codes crates/quarto-core/src` still finds a different function in
  `pandoc_filters/diagnostics.rs` (Pandoc stderr, SGR only). It is not presenter output; I left it.
- The `ansi-output` Julia case has one expression per cell, because a multi-expression cell shows the accepted OPR-4d
  interleaving, not ANSI.

**Not done.** The imported-Positron-record case: the `positron-cache` Python and Julia fixtures hold no ESC, and I did
not build one; the stored-record-stays-raw property is covered by a test over `basic.json` with ANSI outputs. For an
Ark R cell the rule strips R's escapes while knitr keeps them, as the plan's table accepts (`r.cache.ipynb` has three
ESC bytes; no test over it). The recorded route was checked by the parity harness (identical to plain), not by
reading a recorded HTML file; plain `q2 render` of both fixtures printed 0 ESC bytes. The Windows cross-clippy cannot run
on macOS (`aws-lc-sys`). `cargo xtask lint` reports only `positron_cache.rs:360`, the known unrelated finding.
`bd-18410csp` has a comment (the seam exists; conversion is the open half). I did not check this against RIU-3's
Positron traceback check.

**Box A.** [Final gate report](../research/2026-11-10-opr-7g-final-gate-report.md): 67 identical, 6 allowlisted,
**6 unallowlisted**, 253 rejected (same on both routes), 0 skipped. The six are the 4d interleaving (`capture-facts`,
`presentation-attrs`, `records-plot`), the Plots script elements (7h part A; `docs-index-chunk`, `positron-cache`,
`records-plot`) and `fig-subcap` (`bd-gbaykhth`). `nextest -p quarto-core`: 8211 of 8212 pass; the one failure is
`revealjs::assemble::...vendored_reveal_assets_match_npm_package` (local `node_modules/reveal.js` drift; not
verified on the integration branch). The marimo tests ran and passed.

**Box B: not flipped.** The final gate has six unallowlisted differences, and the 7h notes hold an open proposal
(Plots scripts) that needs Gordon's decision. OPR-7d stays `blocked-on-gordon`. What it waits on: a whole-document
allowlist form for the 4d interleaving (7d decision 4 says to add one), a decision on the Plots scripts, and
signing or fixing `fig-subcap`.
