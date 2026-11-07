# KNR interlude I1: sizing the presenter gaps

Date: 2026-11-01. Plan: `claude-notes/plans/2026-11-01-knr-interlude-spikes.md` (I1).
Branch `spike/knr-i1` (unmerged, unpushed), worktree `workspace-3`, R 4.3.2 and the system
knitr, macOS arm64. This note is findings only: the prototypes are rough, and nothing here is
product code.

Every number comes from a `corpus_report` run whose JSON is in the session scratchpad
(`.../scratchpad/i1/`), unless marked "reported, not re-run".

## Answer

Five prototype fixes close **97 of the 154 blockers** (154 to 57) and open none. The prototype
is about 330 lines in the shared presenter and the knitr mapper (`presenter/cell.rs` +191/-10,
`engine/knitr/pieces_map.rs` +141/-8, one re-export line), written in one session. Of the 57
that remain, 48 are small gaps of the same kind, 2 more are trailing-whitespace gaps, and 7 are
not presenter work (see the residual table).

So the range narrows. KNR-2 is **4 to 7 sessions** (was 5 to 10). That is a judgment, not a
measurement: the prototype shows the output is reachable and what each fix costs, but it has
no tests, no record schema, and none of Phase 2E (format-aware parts) or 2F (pass-through
presenter), which are most of the remaining estimate.

Nothing here overturns a settled epic decision. Three things refine one (see "Decisions for
KNR-2 and KNR-3").

## The cause table

The tagger (`tests/integration/knitr_corpus_causes.rs`, spike code) predicts each blocker\'s
causes from the fixture text alone. Output of `KNR_CAUSES_FROM=base.json`
(`scratchpad/i1/causes-base.txt`):

| Cause | Any | Sole | Primary |
| --- | ---: | ---: | ---: |
| interleave | 78 | 15 | 78 |
| markdown-mime | 14 | 12 | 13 |
| attributes | 46 | 27 | 30 |
| text-hooks | 45 | 18 | 19 |
| error-text | 8 | 3 | 3 |
| widget-class | 2 | 1 | 1 |
| figure-options | 23 | 0 | 0 |
| other | 10 | 10 | 10 |

"Primary" is the first cause in plan order. The tagger over-predicts (it flags `stop(` in a
chunk that does not run), which is why the closed counts below differ from these predictions.

## What each fix closed

Each row is one `corpus_report` run of all 213 cases, compared with the run before.

| Step | Blockers after | Closed | Opened | Jupyter-side tests |
| --- | ---: | ---: | ---: | --- |
| baseline | 154 | | | |
| (a) interleaving (`quarto_output_anchors`) | 113 | 41 | 0 | 368 pass |
| (b) `text/markdown` display data | 103 | 10 | 0 | 368 pass |
| (c) options as attributes | 81 | 22 | 0 | 368 pass |
| (d) and (e) text hooks, error text, widget class | 57 | 24 | 0 | 368 pass |

The Jupyter-side tests are `test(presenter::) | test(jupyter) | test(replay) | test(pieces_map)`.
The full `cargo nextest run -p quarto-core` gave 7103 of 7104, the one failure being
`revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`, the failure already
known before this spike (`scratchpad/i1/nextest.log`). `cargo clippy -p quarto-core
--all-targets -- -D warnings` passes. Lines of presenter touched were measured for the whole
prototype only, not per fix.

What each fix was:

- **(a)** `pieces_to_cell_anchored` counts the source lines before each output;
  `render_cell_interleaved` emits source fences and output groups alternately. With no usable
  anchors it calls `render_cell`, so single-group cells keep their bytes. 13 of the 15
  sole-cause interleave cases closed.
- **(b)** A `text/markdown` branch: raw markdown inside a `.cell-output-display` div.
- **(c)** `CellAttrs` (cell div text, source fence classes, id and attributes, output fence
  attributes) filled by `chunk_attrs` from `class.*`, `attr.*`, `column`, `keep-hidden`,
  `code-fold`, `code-summary`, `code-line-numbers`, `lst-cap`, `lst-label`.
- **(d)** In the mapper: numeric `echo` (select or exclude expressions), `prompt`,
  `results = hide / hold / asis`. In the presenter: the `comment` prefix.
- **(e)** No `ename:` header line for knitr errors, and `.cell-output-display` alone for
  knitr HTML output (the widget class).

## Findings that change KNR-2 or KNR-3

1. **Anchors cannot come from piece order alone.** Fix (a) opened two cases
   (`fig.show="hold"`, both forms) until the mapper applied the option: knitr holds plots to
   the end of the chunk, while the capture records them where they were drawn. The same holds
   for `results="hold"`. The `quarto_output_anchors` design stands, but the anchors are
   computed from the pieces **plus the options that reorder or drop them** (`fig.show`,
   `results`, numeric `echo`). In the prototype these ran in Rust when the baseline was
   built, which stands in for the presenter\'s knitr layer. In KNR-2 they must run at **present
   time**, over anchors stored in capture order, not at record time (a header-option edit has
   to show without re-running, epic decisions 10 and 11), and not in R, so the stored pieces
   stay version-stable (as the KNR-1 report found for message formatting).
2. **`text/markdown` is dropped by the Jupyter presenter on purpose.** Two goldens
   (`unhandled-markdown-mime`, `markdown-beats-nothing-plain-wins`) fail when the branch is
   unconditional. I made it opt-in through `metadata.quarto_knitr.asis` set by the mapper.
   KNR-2 must choose between that marker and a presenter parameter. This is a design choice
   for Gordon.
3. **The record loses two distinctions the presenter needs.** (i) `message` and `warning` both
   map to `stderr`, so `message=FALSE`, `warning=FALSE`, `class.message` and `class.warning`
   cannot be applied separately (6 residual cases). Part of this may be the presenter\'s single
   `warning` flag over-filtering, not missing data: R already enforces a logical `warning` or
   `message` at capture, and in `options/warning/*/01_false` the oracle shows the message
   that the baseline dropped (inferred from the oracle, not re-run). If so, the kinds are
   needed only for `class.message`, `class.warning` and numeric `warning` and `message`.
   KNR-2 verifies this first. (ii) knitr\'s list of known option names is
   not captured, so `foo.unknown=1` (which knitr writes as a cell attribute) cannot be told
   from a real option (4 cases). Both need a field in the capture or the record (KNR-1 capture
   and KNR-3), not a presenter change.
4. **`include_graphics` keeps the user\'s path.** knitr writes `![](made.png){width=200}`; the
   presenter copies the bytes to a figure directory and writes no width. The mapper already
   keeps knitr\'s path in `metadata.quarto_knitr.file`, so this is a presenter change plus the
   image size (5 cases; Quarto 1 also writes a width, strand bd-x8hqtvm0: reported in the KNR-1
   report, not re-run).
5. **Which gaps belong in the capture rather than the presenter?** None of the 97 closed.
   Items 3(i) and 3(ii) are the two that do.

## The residual 57

Read from the dumped oracle and candidate markdown of each case
(`scratchpad/i1/residual-d.txt`; the dump hook is `KNR_DUMP=<dir>` in the runner, spike code).

| Class | Cases | What |
| --- | ---: | --- |
| Gap: figure and caption attributes | 12 | `fig.cap`, `fig.subcap`, `fig.alt` (as `fig-alt`), `tbl-cap` on a kable cell, the figure label on a widget |
| Gap: `include_graphics` | 5 | user path kept, width attribute |
| Gap: figure selection | 5 | `fig.show="hide"` (3), `dev="svg"` (2: the svg figure is not written) |
| Gap: `asis` shape | 6 | `results="asis"` and `output="asis"`: knitr writes the text raw and no empty `.cell` wrapper |
| Gap: message vs warning | 6 | needs finding 3(i) |
| Gap: source-shape hooks | 5 | `collapse` (3, outputs merged into the source fence with `## `), `fenced.echo` (2) |
| Gap: cell attributes | 7 | `foo.unknown` (4, finding 3(ii)), `panel` (2), `layout-ncol` (1) |
| Gap: other | 2 | `knit_asis` fence length (5 backticks against 3), widget filters |
| Gap: trailing whitespace | 2 | `whitespace`, `long_line`: the presenter\'s `trim_end()` drops trailing spaces |
| Divergence candidate | 4 | blockquote continuation lines (2), empty `.cell` wrapper (2) |
| Fallback rule | 1 | `knit_hooks_override`: a user hook rewrites the chunk text |
| Out of scope | 1 | `s9/01_inline` (KNR-4) |
| Unexplained | 1 | `s8/01_setup_state` (`params:` front matter, plus an empty wrapper) |

57 = 48 + 2 + 4 + 1 + 1 + 1. The "48" are the first seven gap rows and the "Gap: other" row.

## Candidate allowlist of intentional divergences

For the runner\'s allowlist reader (KNR-2 adds it with its first entry). Each needs Gordon\'s
confirmation.

- `structure/chunk_in_blockquote` and `s7/01_structure`: the presenter\'s blank continuation
  lines carry the `>` prefix and knitr\'s do not, which ends the quote in the live output.
  The reason is already in the KNR-1 report (the baseline form is better).
- `markdown-stress/empty_output` and `s1/03_eval_false`: knitr writes an empty `.cell` div for
  a chunk with no visible source and no output, and the presenter writes nothing. The
  presenter\'s own comment (`presenter/cell.rs`, the `render_cell` doc) gives the reason: an
  empty wrapper is still a box in the page. Read, not tested in a browser.
- `setup-state/knit_hooks_override` is not an allowlist entry: it needs a rule that a document
  overriding the `source` or `output` hooks becomes a fallback unit.

## Recommended phase order for KNR-2

1. Tests over hand-built records first (the plan says so), then anchors with the
   option-aware mapper (finding 1). It closes the most and every later fix reads its output.
2. Options as attributes (c). Independent of the rest; no record change.
3. `text/markdown` and the `asis` shapes, after deciding finding 2.
4. Text hooks, error text, widget class (d, e), including the two trailing-whitespace cases.
5. Figures: captions, `fig-alt`, `include_graphics`, `fig.show="hide"`, svg.
6. Items that need the record or the capture to change first: message versus warning, known
   options, `collapse` and `fenced.echo`. Agree these with KNR-3 before starting.
7. The allowlist reader and the residual.

I do not recommend splitting KNR-2 into two plans. Step 6 is the one cross-plan dependency.

## Not done

- The Quarto 1 column (`KNR_Q1=1`) was not re-run for the residual; only the width gap is
  known to be shared (above).
- The prototypes have no unit tests and no `CellAttrs` for Jupyter use; they exist to count
  cases. `fenced.echo`, `collapse`, `panel`, `layout-*`, captions and `include_graphics`
  were not attempted.
- Windows cross-check and the wasm32 build were not run (no platform-specific code was added).
