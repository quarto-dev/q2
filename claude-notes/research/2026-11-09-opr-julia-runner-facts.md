---
title: 'OPR-4b: the Julia runner reports expression starts, anchors and output kinds'
date: 2026-11-09
description: 'What the QuartoNotebookRunner change does, the runner commits and branch, the wire and test changes, the test results, the timing, and how to run the new behavior. The work lives in a local branch of the runner repository; nothing here changes q2 product code.'
---

**Plan:** [OPR-4b](../plans/2026-11-08-opr-4b-julia-runner-facts.md) · **Spike:** [OPR-4a note](2026-11-09-opr-julia-spike.md) ·
**Epic:** [one presenter path](../plans/2026-11-07-one-presenter-path-epic.md)

## Where the work is

- Repository: the runner checkout at `external-sources/QuartoNotebookRunner.jl` (fork `gordonwoodhull/QuartoNotebookRunner.jl`).
- Branch: **`q2/opr-expression-facts`**, created from `5e04284` (the revision the Julia engine pins today). The checkout\'s own
  branch (`feat/quarto2-handled-languages`) was not touched. Nothing was pushed.
- The branch was worked in a scratch `git worktree` under the session scratchpad. The worktree is temporary; the branch and its
  commits live in the runner repository and survive it.

| Commit | Subject |
|---|---|
| `1551a78` | `test(emit-units): expression starts, output anchors and kinds (failing)` |
| `b3724f6` | `feat(emit-units): report expression starts, output anchors and kinds` |
| `ffe5dab` | `style: format with JuliaFormatter` |

The first commit is red on its own on purpose: it is the tests-first step, and it fails because the keys do not exist yet.
The patch series is about 1100 lines, so it is not copied into this repository; `git log 5e04284..q2/opr-expression-facts`
in the runner repository shows it.

## What changed

With `emitUnits` set, each **code** cell\'s `metadata.quarto` gains four keys (inline results and non-code cells are unchanged):

| Key | Shape |
|---|---|
| `expression_starts` | `[int]`, 0-based line within the code after the `#|` options and any blank lines that follow them |
| `output_anchors` | `[int]`, parallel to `outputs`: the start line of the next expression, or the line count for the last one |
| `output_kinds` | `[null or "message" or "warning" or "error"]`, parallel to `outputs` |
| `output_expressions` | `[int]`, 1-based, parallel to `outputs`; for debugging |

How it works:

- **The gate.** The host sets a new `trace` flag on the worker request (`WorkerIPC.RenderRequest.trace`) exactly when the notebook
  is run with `emitUnits` (the same condition that already adds `metadata.quarto`). Inline results never trace. The worker
  carries the flag on the cell\'s `InlineDisplay`, so every behavior below is gated on it and an ordinary run is unchanged.
- **Attribution.** `include_str` records `loc.line - first code line` for each top-level expression and sets the running
  expression index before evaluating it. Each queued display and each stream run records that index.
- **Kinds.** Under `trace` the cell\'s logger is a `KindLogger` around the usual `ConsoleLogger`. It sets the kind from the log
  level (`@info` and below: `"message"`, `@warn`: `"warning"`, `@error`: `"error"`) for exactly the one write that
  `handle_message` makes. A raw `println(stderr, ...)` has no level, so its kind is `null`.
- **Stream runs.** Under `trace` a run is keyed by (stream, displays before, expression, kind); without it by
  (stream, displays before) as before, so the old merging is unchanged. The prototype split runs unconditionally; this is the
  gating the plan asked for.
- **Stderr fix A (scope chosen by Gordon).** Under `trace`, `warning: false` no longer drops stderr at capture, so the
  presenter can hide just the warnings using the kinds. Without `emitUnits` the runner drops stderr as before.
  `warning` stays in Julia\'s display-only list.
- **Wire.** Worker IPC `PROTOCOL_VERSION` 8 to 9 (internal to the runner). `RenderRequest` gains `trace`. `CellResult.streams`
  tuples go from `(name, text, displays)` to `(name, text, displays, expression, kind)`, and `CellResult` gains
  `expression_starts` and `display_expressions`. Without `trace` the new tuple fields are `0` and `""`.

## Known limits (all pinned by tests or stated here)

- Raw `println(stderr, ...)` has kind `null`, not `"warning"`. Treat `null` on stderr as "stderr, neither message nor warning".
- A cell that throws records starts only for the expressions that ran, and the throwing expression\'s anchor is the line count of
  the code, not the start of the never-run next expression (`an error in the cell anchors at the end of the code`).
- Julia shows only the last expression\'s value, so Julia cells never interleave as knitr cells do. Quarto 1 does not interleave
  Julia either, so this is new behavior.
- Output from C code through libc `puts` is lost, before and after (pre-existing).
- A late `@async` print is attributed to whatever expression is running when it lands (the only possible rule).

## Test changes (nothing weakened)

New tests, written first and seen failing for the stated reason (keys absent, stderr dropped), then passing:

- `test/testsets/expression_facts_test.jl` (6 test items, 27 assertions): byte-identical output and unsplit runs without
  `emitUnits`; the twelve-expression cell (`output_expressions=[1,4,8,9,10,11,12]`,
  `output_kinds=[null,"warning",null,"message",null,null,null]`, with a `display(HTML(...))` as the figure stand-in); starts relative
  to the code after `#|` options for a cell with a comment, a function, a loop and a multi-line array; a subprocess write and a
  late `@async` print attributed; the throwing cell; `warning: false` with and without `emitUnits`.
- `src/QuartoNotebookWorker/test/streams_test.jl`: two new items (trace splits runs by expression and kind, matching the
  untraced cell\'s old three runs; trace keeps stderr under `warning: false`).
- `src/QuartoNotebookWorker/test/plots_test.jl`: a real **Plots.jl** figure (`display(Plots.plot(...))`, a PNG) is attributed to its
  expression, with the stream runs around it placed by display count.

Existing tests changed **only where the wire changed on purpose**:

- `streams_test.jl` (five expectations), `full_render_test.jl` (one), `render_test.jl` (two): each `streams` tuple gains the two
  fields, written out as `0, ""` (untraced). The destructuring `(name, text, _)` became `(name, text, _, _, _)`. Every
  assertion still checks the same name, text and display count.
- `protocol_test.jl`: the `CellResult` round trip now builds and checks the two new fields with non-trivial values (stream
  expression and kind, `expression_starts`, `display_expressions`); the `RenderRequest` round trip checks `trace` both ways.
  These assertions grew stronger.

## Results

| Run | Result |
|---|---|
| Main suite, whole `test/` (TestItemRunner) | 1152 pass, 1 fail, 4 error, 1157 total, 27 min |
| Worker suite, reduced environment | 248 pass, 0 fail, 248 total |
| New and `emit_units` items, again after formatting | 58 pass, 0 fail |
| Worker suite again after formatting | 248 pass |
| JuliaFormatter over the 14 changed `.jl` files | applied (whitespace only), commit `ffe5dab` |

The five main-suite failures are the environment, not the change. `bad exeflags` and the four `julia-1.9.4`, `julia-1.10.7`,
`julia-1.11.2`, `julia-1.11.2 with env override` items run `julia +channel`, a juliaup launcher feature. This run put a plain
`julia` symlink on `PATH` (the real launcher breaks under the scratch `HOME`), and that binary treats `+1.9.4` as a file name
(the log shows `opening file ".../examples/+1.9.4"`). Those channels (1.9.4, 1.10.7, 1.11.2) are not installed either. I did not
re-run them on the unmodified `5e04284`, so "fails without my change too" is inferred from the message, not measured.

What the **worker suite** did not cover: its declared test project needs `CairoMakie`, `PlotlyBase`, `RemoteREPL` and
`SymPyPythonCall`, which cannot resolve offline, so `Pkg.test` for the worker (`WorkerSetup.test()`) failed at resolution
(`Unsatisfiable requirements detected for package CairoMakie`). I ran the worker test items directly in a scratch environment
holding the dependencies that are in the depot, and **skipped** the `cairomakie`, `plotlybase`, `remoterepl`, `sympycore`,
`pythoncall` and `rcall` items. None of those call the changed stream code paths directly, but I did not read each one to
confirm, so Gordon should have the worker suite run once in CI or on a machine with network access.

### Timing

A cell of 1200 outputs (600 `@info` and 600 `display("fig ...")`), five cache-busted runs each of `emitUnits` off and on,
interleaved on one warm server:

| `emitUnits` | runs (s, sorted) | median |
|---|---|---|
| off | 7.92, 8.45, 9.47, 10.99, 11.93 | 9.47 |
| on | 7.10, 8.75, 9.28, 9.74, 10.45 | 9.28 |

On and off are within noise of each other. These absolute times are roughly twice the spike\'s (4.22 s baseline, 4.10 s
prototype), because the cell differs from the spike\'s and the machine was shared with other lanes; compare the two columns of
this table, not the table with the spike. The conclusion is the spike\'s: no measurable overhead.

## How to run the new behavior

```julia
import QuartoNotebookRunner as QNR
server = QNR.Server()
json = QNR.run!(server, "doc.qmd"; markdown = read("doc.qmd", String),
                options = Dict{String,Any}("emitUnits" => true))
cell = first(filter(c -> string(c.cell_type) == "code", json.cells))
cell.metadata.quarto.expression_starts   # and output_anchors, output_kinds, output_expressions
```

Through the socket API, send `options.emitUnits = true` in the `run` request, as the q2 Julia engine already does whenever core
passes units. With `emitUnits` absent or false, none of the four keys appears and stderr under `warning: false` is dropped as
before.

To run just the new tests, from a project that has the runner developed and the test dependencies, load
`TestItemRunner` and filter on the file name (this is what this run did; `test/runtests.jl` runs everything):

```julia
import TestItemRunner
TestItemRunner.run_tests("test"; filter = ti -> contains(ti.filename, "expression_facts"))
```

## What I did and did not do

- **Ran:** the new tests red and then green; the main suite in full; the worker test items in a reduced environment; the timing;
  JuliaFormatter, then the tests again.
- **Read only:** the runner\'s `InlineDisplay`, `render_io`, `render_code`, `render` and `evaluation` code paths, as modified.
- **Not done:** the worker\'s own `Pkg.test` (dependencies not available offline); the skipped worker items above; the five
  juliaup-channel items on the unmodified revision; a q2 render through the changed runner (OPR-4c proves the keys arrive in
  q2); any change to q2 product code; a push.

## For OPR-4c

Gordon pushes `q2/opr-expression-facts` to the fork and writes the pushed revision into OPR-4c. The Julia engine\'s
`Project.toml` pin (`rev = "5e042849..."`) then moves to that revision. The three keys should arrive in the notebook cells
that `read_engine_notebook` already reads; the q2 side needs only a mapper from `output_kinds`, `output_anchors` and
`expression_starts` to the presenter\'s `quarto_*` cell-metadata keys (OPR-4d).
