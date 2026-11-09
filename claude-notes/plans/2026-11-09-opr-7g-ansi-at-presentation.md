---
title: 'OPR-7g: ANSI escapes are removed at presentation, from one function, across engines'
date: 2026-11-09
description: 'Closes the parity gate difference where the recorded route writes raw ANSI escape bytes into HTML (a plain Julia or Jupyter render strips them, as Quarto 1 does). Adds one presenter function through which every stream, text/plain and error output passes, strips there for cells that carry no resolved knitr options, leaves the record raw, and shares one stripping definition and one test table with the TS route. Tested on knitr, Julia, Jupyter, an imported Positron record and marimo. No color yet: the function is where it will land.'
status: ready
depends-on: [opr-7f]
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Evidence:** [OPR-7c parity report](../research/2026-11-09-opr-7c-parity-report.md), cause 2 (`julia/engine-reordering`).
**Branch:** `opr/7g-ansi-at-presentation` · **Lane C** (`workspace-5`) · **Blocks:** OPR-7d.
**Why after OPR-7f:** both plans edit `presenter/cell.rs` (7f its wrapper and output div builders, this plan the text
that goes into them). Start only when OPR-7f is `complete`.
**Needs:** R with knitr, Julia 1.12, Python with ipykernel, `uv` and `node` for the marimo tests. Each skips when its tool
is absent; say in the notes which ran.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5 && pwd
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
| The presenter, error tracebacks | Stripped (`strip_ansi_codes`, CSI only) | read |
| knitr | R\'s escapes pass through unchanged; the knitr corpus fixture `markdown-stress/ansi_and_unicode` pins that | read |

The written position is "strip, no colorization" (plan4b P3-16) with the CSS for ANSI classes tracked in `bd-18410csp`.
No document says what the presenter does, which is why the gate sees a difference. This plan fixes only that, in the
direction a later color feature needs.

## Decision (Gordon, 2026-11-09; the direction, not the whole policy)

- The **record keeps raw ANSI**: stripping is a presentation step, so a later colorizing presenter needs no re-execution.
- Presentation goes through **one function**, `output_text` (name is a suggestion), taking the text, the output
  format and whether the cell carries resolved knitr options. For now: strip when the cell carries none (Jupyter, Julia,
  an imported record, fallback), leave knitr cells as R wrote them. This is decision D3\'s existing split, and it keeps
  the knitr corpus pin. It is an origin test, not a format policy; revisit it when the real policy is set.
- Color is **not** part of this plan. The HTML branch of the one function is where it will land (converting SGR
  sequences to the `ansi-*` classes that `bd-18410csp` ports from `_quarto-rules.scss`); the other formats will keep
  stripping.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **One definition, one table.** A fixture `crates/quarto-core/tests/fixtures/ansi/strip-cases.json` of inputs and
      expected outputs: SGR color and reset, bold and combined parameters, 256-color and truecolor, cursor and erase
      sequences, OSC 8 hyperlinks (BEL and ST terminated), a lone ESC, an escape split by a line break, text with no
      escape. Two tests read it: a Rust test of the presenter function, and a vitest of `stripAnsiCode` in
      `ts-packages/quarto-api`. They must agree on every row; where they differ today, decide the expected output (the
      TS pattern is the canonical `ansi-regex`; the Rust one is CSI-only) and fix the Rust side to match. Check the vitest
      is wired into CI (`.github/workflows/ts-test-suite.yml`) and say so.
- [ ] **Presenter tests first** (`presenter/cell.rs` tests): a stdout stream, a stderr stream, a `text/plain`
      `display_data` and `execute_result`, and an error traceback, each with SGR input, carry no ESC in the markdown for
      a cell with no resolved options; a cell **with** resolved options (a knitr-shaped cell) keeps its bytes.
- [ ] **Record test:** present a record whose output has ANSI; the record\'s output bytes are unchanged afterwards and
      survive `write_record` and `read_record`.
- [ ] Implement the function and route every stream, `text/plain` and error site in `presenter/cell.rs` through it
      (the `fenced_output_div` callers and the error path), deleting the separate `strip_ansi_codes`.
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
- [ ] **Gate:** re-run the parity harness for `julia/` (the OPR-7c report gives the command) and confirm
      `julia/engine-reordering` is identical; the knitr corpus gate must not move. Put the counts in the notes.
- [ ] **End to end through the binary:** render a Julia and a Python document with colored output on the plain route
      and on the recorded route (the OPR-7a scratch switch `OPR7A_FORCE`, re-added uncommitted as its notes describe, or
      the harness); read both HTML files; record the invocation and that no ESC byte is present in either.
- [ ] `cd hub-client && npm run build:wasm` (the presenter is in the WASM).
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the Windows
      cross-clippy (say if it cannot run); `cargo xtask lint`; `q2 render --strict` on this file.
- [ ] Say in the notes how this relates to `bd-18410csp` (comment on it: the seam exists, the conversion is the open
      half) and to the Positron traceback check in RIU-3.

## Exit criteria

- On every engine in the list, the recorded route\'s stream, `text/plain` and error output contains no raw ESC for a
  cell with no resolved options, equal to the plain route\'s; knitr cells are unchanged.
- One function decides, one shared test table covers the Rust and TS stripping, the record is raw.
- `julia/engine-reordering` is identical in the parity gate; no golden or corpus fixture moved.

## Out of scope

Color (HTML conversion and the CSS, `bd-18410csp`); what knitr should do with R\'s escapes; coloring for Typst, LaTeX or
docx; the plain TS route. Those need the real ANSI policy, which is not decided.

## Notes from the run

(Fill in at landing.)
