---
title: "Holding and interleaving output across engines"
date: 2026-11-13
---

## Question

What would it take to make `results: hold` (and interleaving in general) work for every
engine, to let an engine default to holding, and to let the author ask for interleaving,
with a Quarto-idiomatic option name? Also covered: re-checking a DeepWiki analysis of the
knitr-only presenter options, which turned out to be partly wrong.

Branch `explore/results-hold-all-engines` (from main at `936c29e13`). No product code was
changed. One temporary probe test was run and reverted (see "Evidence").

## Short answer

- **Interleaving is not knitr-only anymore.** The Julia runner writes anchors, expression
  starts and output kinds (OPR-4b), and `engine/ts_record.rs` maps them onto the same
  `quarto_output_anchors`, `quarto_expression_starts` and `quarto_output_kinds` keys
  (OPR-4d). Julia cells already interleave by default. Gordon accepted that on 2026-11-09
  (OPR-9d decision 4). Only Jupyter lacks anchors.
- **Only the holds are knitr-gated.** `present_with_inputs` (`presenter/knitr_inputs.rs`)
  applies `fig.beforecode` and `apply_holds` only inside the `resolved_options` branch,
  which only knitr records reach. So `results: hold` is **silently ignored for Julia
  today**, even though its cells carry the anchors `apply_holds` needs. `apply_holds` and
  `before_code_anchors` (`presenter/cell.rs`) are already engine-neutral.
- **Most of the other knitr options already present for Jupyter.** `CellOptions::from_lookup`
  is shared, so `results: hide/asis`, `fig.show: hide`, `collapse`, `comment`, `prompt` and
  `class.*`/`attr.*` all change a Jupyter cell's markdown today (probe below). What they
  lack is a canonical dashed name and a display-only declaration. They sit in the
  `KNITR_ONLY` exemption of the reverse sweep, so editing one in a Jupyter or Julia cell
  re-runs the cell. That is safe, but wasteful.
- **Holding is cheap, interleaving is the hard part.** "Hold everything" is just
  presenting without anchors (`render_cell_whole`), which is what Jupyter cells get now.
  For Jupyter, interleaving needs new capture. A plausible route exists for IPython only
  (see "Jupyter interleaving").
- **A new display-only key does not re-run stored cells.** `exec_hash.rs` hashes the
  declaration's `id` and `version` and filters options through the presentation list; the
  list itself is not hashed. A cell that never set the key hashes the same. Only a cell
  that already uses a key that becomes display-only (for example a Jupyter cell with
  `results: hide`) re-runs, once.

## What the code does today, per engine

| | knitr | Julia (QNR, record route) | Jupyter |
|---|---|---|---|
| Anchors in the record | yes (adapter) | yes (`metadata.quarto.output_anchors`, line counts) | no |
| Default presentation | interleaved | interleaved (new vs. Quarto 1) | held (source, then outputs) |
| `results: hold` / `fig.show: hold` | applied | **ignored** (knitr branch only) | no effect (already held) |
| `fig.beforecode` | applied | forwarded as a cell attribute | forwarded as a cell attribute |
| `results`/`fig.show` hashed? | no (structured profile) | yes | yes |

Quarto 1 interleaves knitr only. Jupyter and Julia both show source then outputs.

## Semantics: knitr hold is partial

knitr's `results: hold` moves only printed text (stdout) to the end and merges it into one
stream. `fig.show: hold` moves only plots. Messages, warnings and errors stay where they
were produced. So "hold" in knitr is not "all output after the code". The Jupyter look
(and Quarto 1 Julia) is "all output after the code, each output kept separate, in capture
order". That is `render_cell_interleaved` with `anchors = None`.

A Quarto-level option should mean the simple thing (all outputs after the code). knitr's
`results: hold` and `fig.show: hold` stay as finer-grained knitr spellings that refine the
interleaved mode. Lifting them out of the knitr branch makes them work for any record with
anchors (Julia today).

## Naming

No Quarto 1 option covers this. The closest is `output-location`
(`cell-textoutput.yml`, revealjs only: `default | fragment | slide | column |
column-fragment`). It is described as "location of output relative to the code that
generated it", but a Lua filter implements it by moving all of a cell's output divs. q2 does
not implement it yet (only the recorded Q1 filters mention it). Overloading it would mix a
format-specific layout feature with a presenter decision. Recommendation: a new
engine-neutral key, valid in a cell `#|` header and under `execute:`.

Candidates, best first:

1. `output-interleave: true | false`. A boolean is the most declarative, and it reads
   correctly at document scope (`execute: {output-interleave: false}`).
2. `output-placement: interleaved | after-code`. An enum leaves room for later values,
   but there is no known third value.

Either way, the knitr spellings are not aliases of it, because their meaning is partial.
They keep their names (`results`, `fig.show`) and get dashed canonical spellings in the
slice-7 manner only if that is wanted. `fig-show` is not a Quarto 1 name.

Precedence: an explicit `output-interleave: false` wins (render whole). Under `true`, or
when the key is unset and the engine's default is to interleave, knitr's `results: hold`
and `fig.show: hold` refine the result.

## Engine defaults

The presenter must not test engine names (OPR principle), so a default has to arrive as
data. Two routes:

- **Engine declaration:** a field next to the hash definition (Rust engines:
  `ExecutionEngine`; TS engines: the record-wire declaration in
  `julia-engine/src/record-wire.ts`) that ends up on `EngineEntry`. The step reads it when
  the cell and the scope leave the key unset. This is clean and versioned with the record.
- **Do nothing for knitr and Jupyter:** knitr must default to interleaving (Quarto 1
  parity, the corpus), and for Jupyter the question does not arise (no anchors). So the
  only engine whose default is actually in question is **Julia**.

**Decision needed (Gordon):** should Julia default to holding? That would reverse OPR-9d
decision 4 and restore Quarto 1 Julia parity, leaving interleaving as opt-in. It would also
make the parity harness's `julia-interleaved-cell` normalizer and the pinned
`julia/capture-facts` entry unused. Check how the harness treats an unused rule before
removing them.

## Jupyter interleaving (feasibility)

Jupyter kernels report no source positions. Splitting a cell into one `execute_request` per
statement would change display semantics (every expression would show) and execution
counts. A narrower route exists for **IPython only**. q2 talks to the kernel itself
(`engine/jupyter/execute.rs`) and already runs per-language setup code
(`execute_info::jupyter_setup_code`). The setup code could register an
`InteractiveShell.ast_transformers` entry that inserts a marker call *before* each top-level
statement except the first. Inserting before rather than after keeps the user's last
expression last, so `ast_node_interactivity = "last_expr"` still displays it. Each marker
would publish a display message with a private MIME type. ipykernel flushes stdout and stderr
before it publishes a display, so the markers arrive in order with the output. The engine
would turn the markers into anchors and expression starts and strip them.

Cell magics (`%%`) bypass the AST and would degrade to held, which is the right fallback.
IRkernel and IJulia have no equivalent, but R and Julia normally go through knitr and QNR
anyway. This needs a spike before any commitment. Until then, the new key is *inert* for
Jupyter, and `jupyter_option_sweep.rs` needs an `INERT_REASONS` entry ("Jupyter records
carry no anchors; every cell already renders held").

## The other knitr options, corrected

| Option | Presents for non-knitr today? | Record inputs | Recommendation |
|---|---|---|---|
| `results: hold`, `fig.show: hold` | no (knitr branch) | anchors (Julia has them; Jupyter none, where hold is the default anyway) | lift out of the knitr branch |
| `fig.beforecode` | no (forwarded as an attribute) | anchors | lift with the holds (same branch), low priority |
| `results: hide`, `results: asis` | **yes** | none (kinds sharpen the stderr split) | declare display-only under a canonical name if wanted |
| `fig.show: hide` | **yes** | none | same |
| `fig.show: animate` | no | capture-side (one gifski file) | knitr-only |
| `collapse` | **yes**, merges outputs into the fence with `##` (R idiom) | none | keep knitr-flavored; no new name |
| `comment` | **yes** (a new feature for Jupyter, not parity) | none | same |
| `prompt` | **yes**, but writes R's `> `/`+ ` on Python; one expression without starts | expression starts (Julia has them) | knitr-only in spirit; the prefix is wrong for other languages |
| `class.*`, `attr.*` | **yes** | none | already shared |
| `fig.sep`, `fig.ncol`, `fig.valign` | no (`synthesize_layout`, knitr branch) | figure count | not needed: `layout`, `layout-ncol` and `layout-valign` are native |
| numeric `echo`, `warning`, `message` | yes where starts or kinds exist (Julia) | starts, kinds | already shared |
| `strip.white`, `highlight`, `size`, `background`, `indent`, `chunk.echo` | n/a | n/a | inert (slice 6 and 7) |

Where DeepWiki was wrong: it placed `comment`, `class.*`, `results: asis` and `fig.show:
hide` in "needs lifting", but they already present. It said anchors are knitr-only, but
Julia supplies them. It said `prompt` and interleaving have no non-knitr source, but Julia
supplies them too. Its claim that every newly display-only option "re-runs stored cells once"
holds only for cells that already set the key.

## Is "every knitr presenter feature on every engine" realistic?

Mostly yes. The presenter is already shared. The real limit is what each engine
**records** when it runs a cell, not the presenter. Grouped by what an option needs from the
engine:

1. **Already works for every engine.** The probe confirmed `results: hide/asis`,
   `fig.show: hide`, `class.*`/`attr.*`, `comment`, `collapse` and `prompt`. What remains:
   - declare them display-only for Jupyter and Julia, because an edit now re-runs the cell;
   - add dashed Quarto names where wanted;
   - fix `prompt`, which always writes R's `> ` and `+ ` (Python and Julia need their own
     prompts, or it stays R-only).

   `collapse` writes `##` before each output line, which is an R console habit; it is odd but
   readable in Python. `comment` is new for Jupyter, not Quarto 1 behavior.
2. **Needs output positions** (interleaving, `results: hold`, `fig.show: hold`,
   `fig.beforecode`). knitr and Julia record them, and `2026-11-13-output-interleave.md`
   covers both. Jupyter does not. IPython is plausible (see "Jupyter interleaving"); other
   kernels are not.
3. **Needs per-expression information** (exact `prompt`, numeric `echo`, numeric
   `warning`/`message`). Julia supplies it and these features already work there. For
   Jupyter it would come from the same IPython spike; without it they degrade gracefully.
4. **Not presenter features.** These act while the code runs: `fig.show: animate` (R
   composes the file), `fig.keep`, `ref.label`, `tidy`, numeric `eval`. Another engine would
   have to implement each one itself. That is per-engine feature work, not lifting.
5. **Not worth lifting.** `fig.sep`, `fig.ncol` and `fig.valign` only build the native
   `layout*` keys. `strip.white`, `highlight`, `size`, `background`, `indent` and `chunk.echo`
   are inert (slices 6 and 7). `fenced.echo` depends on knitr-only record data; whether
   Jupyter `echo: fenced` already covers it was not checked.

**Verdict.** On the presenter side it is realistic, and for Julia too. For Jupyter it is
partial, gated by the IPython spike, with no route for other kernels. Group 4 is out of reach
by design. A better goal is "every presentation feature under a Quarto name, wherever the
engine records what it needs", keeping knitr spellings as aliases only where the meaning
matches.

Suggested order after `output-interleave`:

1. Declare group 1 display-only (cheap).
2. Make `prompt` language-aware.
3. The IPython anchor spike, which opens groups 2 and 3 to Jupyter.

## Implementation sketch (if Gordon picks the direction)

Structure follows `2026-11-03-presenter-generic-cell-options.md`.

1. **Tests first.** A Julia-shaped record (anchors, no resolved options) with
   `results: hold` holds stdout; with `fig.show: hold` holds plots. Both fail today. A record
   with anchors and the new key set to `false` renders whole. Jupyter cells are unchanged
   byte for byte.
2. **Lift the holds.** Move the `fig.beforecode` and hold block in `present_with_inputs` out
   of `if let Some(merged)`. Read the flags from `CellOptions` (generalize `fig_show_hide`
   to a `fig_show` value, add `fig_beforecode`), not from the raw `merged` JSON. The knitr
   corpus (`s1/04_results`, `s1/05_fig_show_hold`, `s1/16_fig_show`) must stay green.
3. **The new key.** Read it in `from_lookup`. In the step, `false` drops the anchors before
   `render_cell_interleaved`. Declare it display-only in Jupyter `DISPLAY_ONLY_KEYS`, in
   the Julia `record-wire.ts` list (edit the fork, then `pull-extension-subtree`), and in
   knitr `STRUCTURED_PRESENTATION_OPTIONS`. Add it to the R layer's known options
   (`hooks.R` `quarto_opts`) so `from_knitr_known` does not forward it as a cell attribute.
4. **Engine default** (only if Julia defaults to holding): a declaration field reaches
   `EngineEntry`, and the step reads it when the key is unset. Update the parity harness
   allowlist.
5. **Sweeps.** Add a `jupyter_option_sweep.rs` row (Inert, with a reason) and a
   `record_splice/tests/option_sweep.rs` row (Presents). If `results` and `fig.show` become
   display-only for Jupyter and Julia, move them out of `KNITR_ONLY` in the reverse sweep.
6. **Docs.** Add a user page for the option, and a migration note if Julia's default
   changes.

## Side findings

- `engine/ts_record.rs` documents `output_anchors` as "the expression each output follows",
  and the OPR-4b wire table says "the index of the expression". The runner
  (`QuartoNotebookRunner.jl` `src/evaluation.jl:546`, branch `q2/opr-expression-facts`)
  writes the **number of source lines before** each output, which matches knitr. The code
  is right, but both descriptions are stale.

## Evidence

- Probe (temporary test in `presenter/jupyter_option_sweep.rs`, reverted): a stored Python
  cell presented with each option set, compared against the bare cell. Inert:
  `results: hold`, `fig.show: hold`. Presents: `results: hide`, `results: asis`,
  `fig.show: hide`, `collapse`, `comment`, `prompt` (`> 1 + 1`), `class.output`, and
  `fig.beforecode` (only as the forwarded attribute `fig.beforecode='true'`).
- Hash: `record/exec_hash.rs` `Fields::new` hashes `id` and `version`;
  `execution_options` filters the options through the profile's list.
