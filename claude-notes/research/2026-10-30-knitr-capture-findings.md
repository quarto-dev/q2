---
title: 'knitr capture: findings and reproducible experiments (2026-10-30)'
date: 2026-10-30
description: 'What the author of the knitr execution-records epic established by reading knitr and q2 code and by running small R experiments, with the scripts to repeat them, the line references, and what is still unverified.'
---

**For:** reviewers and implementers of
[the knitr execution-records epic](../plans/2026-10-30-knitr-execution-records-epic.md).
**Environment:** R 4.3.3, knitr 1.50, rmarkdown 2.29, macOS. The experiments ran on the
installed knitr 1.50. The source reads used `external-sources/knitr`, which is a
**development snapshot** (`DESCRIPTION` Version 1.52.5, `NEWS.md` headed 1.53), not 1.50, so
every `block.R` and `output.R` line number below is the snapshot\'s and matches no release
(`evaluate(` is at `block.R:261` in v1.44, `:262` in v1.50 and `:313` in the snapshot). The q2
code is `crates/quarto-core/src/engine/knitr/resources/rmd/` on branch
`feature/knitr-execution-records-epic` (workspace-3). Added 2026-10-31 after the
[paper review](2026-10-30-knitr-epic-paper-review.md).

Nothing here was run through q2's own `execute.R` path; the experiments use plain `knit()`
and `rmarkdown::render`. Treat the experiments as evidence about knitr, not about q2.

## 1. How knitr assembles the document

`knit()` calls `process_file`, which loops over groups (`output.R:346-370`; the knit-exit
branch is lines 347-353 and the per-group assignment `res[i] = ...` is at 359), calling
`process_group` (`block.R:2-8`) for each: a chunk goes to `call_block`, a text group to
`call_inline` then the `text` hook (default `identity`). Results are stored in `res[i]`, the
`document` hook runs (`output.R:295`, `one_string(knit_hooks$get('document')(res))`), and
the pieces are joined with `"\n"` (`one_string`, defined at `utils.R:1130`). q2 sets no `document` hook
(`grep` of `hooks.R`).

**Experiment 1 (join).** Record the return value of the `chunk` and `text` hooks and compare
their `"\n"` join with the file `knit()` wrote:

```r
suppressMessages(library(knitr))
pieces <- list(); render_markdown()
oc <- knit_hooks$get("chunk")
knit_hooks$set(
  chunk = function(x, options) { r <- oc(x, options); pieces[[length(pieces)+1]] <<- c("C", r); r },
  text  = function(x) { pieces[[length(pieces)+1]] <<- c("T", x); x })
knit("doc.Rmd", output = "doc.out.md", quiet = TRUE)
full  <- readChar("doc.out.md", file.info("doc.out.md")$size, useBytes = TRUE)
recon <- paste(sapply(pieces, `[`, 2), collapse = "\n")
identical(full, recon)
```

The document used: a YAML header, prose with `` `r 1+1` `` and `` `r NULL` ``, a plain chunk,
prose directly after a chunk, a list item containing an `echo: false` plot chunk, an
`include=FALSE` chunk, an `eval=FALSE` chunk, and trailing prose without a final newline.

**Result.** Not identical, for two reasons found by reading the diff (the fixture document
was not committed, so this experiment cannot be repeated from this note; the reasons agree
with `block.R:425` and `output.R:295-297`):
1. The `include=FALSE` chunk: the hook returned the full fenced source, but the document had
   an empty string there (the diff shows the extra ```` ``` r y <- 2 ``` ```` block).
2. The final newline: `knit()` writes the file with a trailing newline that the join
   lacks (the source had none).

Reading the code explains (1): the chunk hook runs inside `eng_r` (`block.R:396`) and
`include` is applied afterwards (`block.R:425`: `if (options$include) output else` the
indent string or `''`; `block.R:214` for non-R engines).

## 2. The cache short-circuits the chunk hook

`call_block` returns before `block_exec` for a cached chunk: `if (!params$include) return('')`
then `if (params$cache == 3) return(cache$output(hash))` (`block.R:100-101`). `cache=TRUE`
is level 3, so a hit yields the stored output string and the chunk hook never runs. Levels 1
and 2 store the piece list (`res.orig`, `block.R:337-340`) and a hit reads it with
`cache$output(options$hash, 'list')` (`block.R:308-311`). *Not run; read only.*

## 3. Preserve placeholders are random

**Experiment 2.**

```r
suppressMessages(library(htmltools))
a <- "<!--html_preserve--><div>A</div><!--/html_preserve-->"
b <- "<!--html_preserve--><div>B\nmultiline</div><!--/html_preserve-->"
doc <- paste0("text\n", a, "\nmid\n", b, "\nend\n", a, "\n")
str(extractPreserveChunks(doc))
str(extractPreserveChunks(a)); str(extractPreserveChunks(b))
```

**Result.** The same `<div>A</div>` received two different placeholder ids within one
document, and a third id when extracted alone. So the ids are not content-derived. The
current markdown is therefore not byte-stable across runs for any document with preserve
markers (q2 runs `extract_preserve_chunks` on the finished output file,
`resources/rmd/execute.R`, near the end of `execute()`). This experiment used stock
`htmltools`; q2\'s `patch.R` sets `htmltools.preserve.raw` and replaces `htmlPreserve`
(`patch.R:11-26`), so a q2 render may produce a raw ```` ```{=html} ```` block instead of a
preserve marker, which review confirmed for `htmltools::div("hi")`. Which markers occur in
q2\'s markdown is not established. Review also measured `htmlwidget-<hex>` ids and `plotly`
`cur_data` ids that differ between runs.

## 4. The pipeline between `evaluate()` and `sew`

For an R chunk (`eng_r`, `block.R`):

| Step | Lines |
|---|---|
| `evaluate(code, ..., keep_warning, keep_message, stop_on_error, output_handler)` | 313-324 |
| numeric-`eval` masking cleanup | 335 |
| cache copy for levels 1 and 2 | 337-340 |
| echo filter (`FALSE`, numeric) | 352-361 |
| `results` `hide` and `hold` | 362-366 |
| `warning` and `message` filters | 367-368 |
| `rearrange_figs` (`fig.keep`, `fig.show`) | 371 |
| merge adjacent pieces; `fig.beforecode` | 382-384 |
| `unlist(sew(res, options))` | 392 |
| chunk hook | 396 |
| `include` applied | 425 |

`sew` is an S3 generic (`output.R:535`) with methods for `list`, `character`, `knit_asis`,
`source`, `warning`, `message`, `error`, `recordedplot`, `knit_image_paths`,
`html_screenshot` and `knit_embed_url` (`NAMESPACE:11-22`; `source` and `warning` are at
lines 21-22). `sew.recordedplot` writes the
figure files with `save_plot` and then calls the plot hook (`output.R:657-677`).
`engine_output(options, out = LIST)` returns `sew(out, options)` (`engine.R:94`); most
engines call `engine_output(options, code, out)` with a string (19 lines in
`engine.R` contain `engine_output(options`) and have no pieces. q2 also replaces `sew` itself
in `patch.R` (lines 150-213) and overrides `add_html_caption`, so "wrap `sew`" means wrapping
q2\'s replacement.

Consequence for the epic: a capture at `sew` sees pieces **after** the presentation
filters; a capture right after `evaluate()` sees everything, in execution order, but must
render plots itself and must deal with `evaluate()`\'s own option-dependent arguments.

## 5. q2's `hooks.R` is two layers on knitr's

q2 starts from `knitr::hooks_markdown()` (`hooks.R:178`) and each hook calls the default,
then decorates (`delegating_hook`, `:179-184`). The decoration, by area (details and line
ranges in [KNR-2](../plans/2026-10-30-knr-02-knitr-presenter-layer.md)): the cell div with
classes, forwarded layout and unknown options, captions and label dedupe; the source hook
with Quarto code options; output divs; a format-aware plot hook with animations.

The Jupyter presenter (`engine/jupyter/text_execute.rs`, `render_cell` at `:562`) covers only
`echo`, `output`, `warning` and `include` and a bare `::: {.cell}`, but its output divs
were built to match q2-knitr's (comments in that file; `engine_output_parity.rs` compares
block shape). It has none of the layout, panel, column, caption, label, code-fold or
forwarded-attribute logic.

## 6. R-side option state

`call_block` merges `opts_chunk` (including a setup chunk's `opts_chunk$set(...)`),
`opts_template`, `opts.label`, `ref.label` and `opts_hooks` into the chunk's options at run
time (`block.R:9-92`). None of that is visible in the document text, so "present from the
document's options at present time" (EXR-6's rule for Jupyter) is insufficient for knitr.
*Read only; S8 in KNR-1 confirms with a fixture.*

## 7. Version facts

q2\'s R code has version branches at `patch.R:6` (1.37.2), `patch.R:152` (1.32.8) and
`patch.R:300` (`<= 1.45`), and at `hooks.R:332`, `:793` and `:842` (1.44 normalization).
`rmd.R:8` is a hard `stop()` for knitr < 1.44 but only inside `spin` for `.R` input, and
`execute.R:774` only chooses `knitr:::has_crop_tools` or an inlined copy, so neither is a
compatibility floor. No hard pin was found. `.github/workflows/*.yml`
has no R setup (grep for `setup-r`, `r-lib`, `Rscript`, `install.packages`), so R tests skip
in CI.

## What is not verified

- Reticulate's Python engine; how `knit_asis` objects carry their kind.
- What forcing `keep_warning` and `keep_message` to `TRUE` changes in `evaluate()`. Review
  reports from the code and docs (not run) that logical values are enforced by `evaluate()`
  and not by the later filters, and that `NA` means "not captured, bubbles to the caller".
- Behavior on knitr 1.44 to 1.49: only the git history was read.
- Whether an HTML comment marker survives `rmarkdown::render`, preserve extraction and
  `postprocess_markdown` (KNR-1 S7).
- Whether a user's `knit_hooks$set` in a setup chunk can be detected reliably at chunk time.
- Anything about q2's own `execute.R` path beyond reading it.
