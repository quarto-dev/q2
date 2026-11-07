# KNR-1 corpus report: knitr capture and the baseline presenter

Date: 2026-11-01. Plan: `claude-notes/plans/2026-10-30-knr-01-capture-spike-and-harness.md`
(Phases 1D and 1E). Branch `feature/knitr-execution-records-epic` worktree, R 4.3.2, macOS
arm64. Every number below comes from a run of `corpus_report` (the runner in
`crates/quarto-core/tests/integration/knitr_corpus_runner.rs`) unless marked "read" (from
reading diffs) or "inferred".

## What was run

The corpus is `crates/quarto-core/tests/fixtures/knitr-corpus/manifest.toml`: 213
(fixture, format) cases (option sweeps in header and YAML form, verbatim documents, and
the committed S1 to S9 fixtures). Each case is run once with capture on. The same R run
gives knitr\'s own markdown (the oracle) and every chunk\'s pieces. The baseline then presents
the pieces with the unmodified presenter (`pieces_to_cell`, then one `render_cell` call per
chunk, spliced into the source where the chunk was). Both sides are normalized (random
widget ids, figure file names) and compared at three levels: bytes, parsed AST
(`pampa::readers::qmd::read` printed with the native writer), and rendered HTML (the whole
pipeline after the engine, through a fixed-output engine).

Classes: *identical* (same bytes), *syntax-level* (same AST), *semantic* (same HTML,
different AST), *blocker* (different HTML).

## The two gates

| Gate | Result |
| --- | --- |
| `control_run_is_identical_after_normalizer` (two capture-off runs of every case) | passes after two rules |
| `capture_does_not_change_live_output` (capture on vs off, normalized) | passes, 213 cases |

The control run found only random ids, both in the htmlwidget fixtures (`htmlwidget-<20 hex>`
and plotly\'s `visdat` id). No temp path, timestamp or other noise appears in any fixture\'s
output, so the normalizer has no path rule. Four cases abort in knitr itself, identically
every time (S1 07 and 08 by design; `eval=1` in both forms, where `x` is not found because
only the first expression is evaluated). Three cases cannot run here for a missing package
(`gifski` for `fig.show="animate"`, two forms; `RSQLite` for `s5/01_odd_chunks`).

## Counts per class

The same table holds on knitr 1.44 (the minimum), 1.50 and 1.52 (the latest): the class of
every case is the same on all three versions (the three summaries differ only in a trailing
blank line).

| Outcome | Cases |
| --- | ---: |
| compared | 202 |
| \- identical | 0 |
| \- syntax-level | 48 |
| \- semantic | 0 |
| \- blocker | 154 |
| fallback (the baseline cannot present the document) | 4 |
| aborted by knitr | 4 |
| unavailable (missing package) | 3 |

No case is identical. knitr\'s hook output starts each chunk with a blank line and the
presenter\'s does not, so a byte comparison can never succeed; the 48 syntax-level cases
are the ones that differ only in blank lines and fence spelling. No case is semantic: when
the ASTs differ, the HTML differs too.

Per group (blockers marked `*` have a chunk with two or more source groups, see below):

| Group | Syntax-level | Blocker | Blocker\* | Other |
| --- | ---: | ---: | ---: | --- |
| options | 30 | 36 | 56 | 2 fallback, 2 aborted, 2 unavailable |
| quarto-cell-options | 0 | 10 | 3 | |
| markdown-stress | 1 | 2 | 4 | |
| rich-output | 5 | 14 | 0 | |
| structure | 3 | 1 | 1 | |
| setup-state | 3 | 2 | 0 | |
| engines | 1 | 0 | 0 | 2 fallback |
| s1 | 3 | 4 | 10 | 2 aborted |
| s2 | 1 | 4 | 4 | |
| s5, s7, s8, s9 | 1 | 3 | 0 | 1 unavailable |

Fallback rate: 4 of 213 cases (1.9%), 4 of 206 runnable ones:
`dev="pdf"` (two forms; a `pdf` figure is not an image the presenter writes), the shell chunk
and the `ojs`/`mermaid`/`dot` document (an `Other` piece: a non-R engine). The `child=`
fixture (`s5/01_odd_chunks`) could not run for the missing package, so the chunk-count
fallback is untested on a real case; a unit test covers it.

## Why the blockers differ (read from diffs; counts are of fixtures, not of causes)

1. **Interleaving (S2, expected).** 78 blockers have a chunk with source, output, source:
   `render_cell` is one call per chunk, so all the source lands first and all the output
   after, where knitr alternates `.cell-code` and `.cell-output` blocks. This is the
   recorded divergence KNR-2 closes with the S2 anchors. It masks other causes in those 78
   cases, which is why the sweeps that do not need interleaved output use a one-group body
   (the other 76 blockers).
2. **Options the presenter does not carry** (single group, about 28 cases): cell attributes
   (`foo.unknown`, `keep-hidden`, `code-fold`, `code-summary`, `code-line-numbers`,
   `lst-*`, `column-*`, `panel-*` on the `.cell` div; `class.output`, `class.source`,
   `class.error`, `attr.output`, `attr.source` on the fences; `fig.cap`, `fig.alt`,
   `fig.subcap` on the image). `render_cell` writes `::: {.cell}` and bare fences.
3. **knitr text hooks the presenter has no equivalent for** (about 16 cases):
   numeric `echo`, `prompt`, `comment`, `fenced.echo`, `results="asis"`/`"hide"`/`"hold"`,
   `message=FALSE` (the presenter\'s `warning` flag drops all stderr, so `message` and
   `warning` cannot be told apart), fence length when output contains backticks.
4. **`text/markdown` display data is dropped (read, kable and `asis_output`).** `render_cell`
   handles `text/html`, images and `text/plain` only. The `knit_asis` and kable pieces (`asis`
   with `text/markdown`) vanish from the output: 10 of the 14 rich-output blockers (kable and
   `asis_output` in four formats each, plus the two `prefer-html` cases). The other four are
   `include_graphics`.
5. **Error text.** The mapper puts `ename` and the message in the traceback, and
   `format_error_text` prints `simpleError: boom` before `Error: boom`.
6. **Widget wrapper class.** knitr: `.cell-output-display`; presenter:
   `.cell-output .cell-output-display` (S2 03 and 09).
7. **Inline expressions are untouched (S9).** The baseline is a splice of cells, so
   `s9/01_inline` differs by every inline span; that is KNR-4\'s subject, not a presenter gap.
8. **Blockquote blank lines** (`structure/chunk_in_blockquote`): the presenter\'s blank
   continuation lines carry the `>` prefix, knitr\'s do not, which ends the quote in the
   live output. The baseline form is the better one (S6 recorded it); an intentional
   divergence in the plan\'s sense.

## What differs between knitr versions

Class counts are identical on 1.44, 1.50 and 1.52. The text differs in three places:

- error text: 1.44 `Error in eval(expr, envir, enclos): boom`, 1.50 `Error: boom`, 1.52
  `Error:` then `! boom` on the next line;
- `class.output="foo"`: 1.44 fences it as `` ```{.foo} ``, 1.50 and 1.52 as `` ``` foo ``;
- fence length for output that contains backticks (1.44 uses three where later versions
  use four).

The pieces do not carry these: they are formatting by `sew`/hooks that the presenter
replaces, so a record built from pieces is version-stable where the markdown is not.

## A bug the 1.44 run found in my Phase 1B code

`capture.R` called `xfun::str_wrap` when it wrapped a warning. knitr 1.44 with xfun 0.40
has no such function (knitr carries its own `str_wrap`; `xfun::str_wrap` exists from knitr
1.50), so every chunk with a warning or message aborted on the minimum version: 85 of 213
cases. Fixed with `capture_str_wrap` (uses xfun\'s if it exists, knitr\'s otherwise).
1.44 now gives the same class table as 1.50. The existing `capture_tests.rs` run on the
system knitr (1.50) and missed it; the corpus run under a pinned library is what catches it.

## The Quarto 1 column

How it is gathered: `quarto render doc.<ext> --to html -M keep-md:true` with the
`quarto` on `PATH`, which here is a quarto-cli development checkout reporting `99.9.9`
(the runner accepts `1.x` or `99.9.9`). The kept `doc.html.md` is Q1\'s post-engine
markdown; it is compared with q2\'s *current* knitr output (the live path), html cases
only, behind `KNR_Q1=1`. Skipped when no such `quarto` is found.

Of the 189 html cases that were compared, 183 rendered under Q1: 135 identical, 2
syntax-level, 46 blockers; 6 failed in Q1 itself (numeric `echo`, `eval`, `warning` and `message` in YAML
form, which Q1 rejects: a Q1 limitation, not a q2 gap).

The 46 blockers are one gap in different clothes (strand bd-x8hqtvm0, filed because a
skein search for width, figure, image and widget found no existing strand):

- Q1 writes `{width=N}` on every knitr plot image from `fig.width` times dpi (384 for
  `fig.width=4`, 672 by default, 768 for 8); q2\'s live output has no width attribute.
- `include_graphics` of a 200px PNG: Q1 `{width=100}`, q2 `{width=200}`.
- htmlwidgets: Q1 `width:100%;height:464px`, q2 `width:672px;height:480px`.
- `s8/01_setup_state`: q2\'s knitr output begins with the document\'s `params:` front matter
  and Q1\'s does not. Not investigated (it may be a q2 gap or an artifact of my front-matter
  strip).

Q1 also shows the presenter-level gaps above only indirectly: it was not compared with
the baseline, only with the live path.

## Estimates and what KNR-2 inherits

- The presenter layer (KNR-2) needs: interleaving anchors, options as attributes,
  hook-equivalent formatting (numeric echo, prompt, comment, fenced.echo, results), message
  versus warning, `text/markdown` display data, error text, widget wrapper class. Items 1
  to 6 above are the work; 1 and 4 are the largest.
- Fallback rate 1.9% of cases means the fallback path carries real weight only for
  non-R engines and `pdf` figures.
- Wall time: a full run of the 213 cases is 70 to 90 s on an idle machine with capture on
  once, 200 s with the Q1 column. The two gates (each runs every case twice) took 330 to
  450 s when other cargo work was running.

## Not done, and why

- The keyed allowlist of intentional divergences (plan, runner item) has no entries
  because nobody has decided a divergence intentional yet; every blocker above is a gap
  until then. The runner has no allowlist reader; KNR-2 adds one with its first entry.
- The corpus covers html everywhere and four formats only for the `rich-output`
  documents; no LaTeX target (epic decision 13).
- `cache` levels 1 to 3 run once per case (a fresh directory), so a cache hit is not
  exercised by the corpus; S1 14 and S7 02 cover it by hand.
