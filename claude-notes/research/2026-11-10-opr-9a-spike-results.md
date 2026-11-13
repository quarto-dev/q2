---
title: 'OPR-9a spike results: S1 to S6'
date: 2026-11-10
description: 'Measurements for the plain-render switch: the Julia format matrix on 22 targets, the presenter-fix prototype (parity on all 22 targets, no existing quarto-core test fails), the interleaving normalizer prototype (lol_html, a 24-line pin for capture-facts), zip extraction for docx, pptx and epub, the reserved-key question, and the browser replay path.'
---

**Plan:** [OPR-9a](../plans/2026-11-10-opr-9a-non-html-spikes.md) ·
**Binaries:** S1, S4 and S5 used `target/debug/q2` built from a clean tree (branch `opr/7d-implementability-review`, `git status` empty). S2 and S3 used a copy (`q2-s2`) built from the same tree plus the 9b prototype patch; the "before" runs used `q2-clean`, built from the clean tree after the patch was reverted. The tree was clean after every prototype, and `cargo build --bin q2` was rerun on it at the end ·
**Scratch:** `<scratchpad>/s1` (matrix), `s2` (matrix with the fix), `s3-*` (the four pages), `jup` (Jupyter), `s4` (zip draft), `s5` (reserved keys). The prototype patches are `9b-prototype.patch` (330 lines, 13 files) and `s3_proto.rs.txt` there; nothing is committed.
Marks: (ran) measured here, (read) from source, (relied) taken from an earlier note.

Every S1 number below is the unfixed presenter; S2 repeats the matrix with the fix.

## S1. Format matrix (ran)

*Question:* for each target, what does the Julia runner emit and what does each route write?
*Method:* the `julia/records-plot` fixture, the default environment\'s `Project.toml` and `Manifest.toml` (which carry Plots), rendered `--to <format>` on a plain route (`project: type: default`) and a recorded route (`execute: record: true`).
Per format: the mime keys in `.quarto/execution-records/doc.ipynb`, the files under `doc_files`, the media inside zips.

**Formats rendered:** html, revealjs, dashboard, gfm, commonmark, markdown, docx, pptx, odt, epub, typst, rtf, rst, org, plain, native, icml, fb2, docbook, mediawiki, djot, textile (22).
**Skipped:** `pdf` (both routes stop with "Format 'pdf' is not yet supported", so there is nothing to measure); `latex` and `beamer` (unknown formats in this tree, relied from the review); the remaining `format.rs` identifiers (json, ms, man, texinfo, tei, zimwiki, dokuwiki, haddock, jira, xwiki, opml-like and the slide-show families) were not rendered: they run the same raw-html path as the text formats below, and I did not measure them.

### Where the bundle is chosen (read)

In the Julia runner, not in q2: `QuartoNotebookRunner/src/QuartoNotebookWorker/src/render_mimetypes.jl:42-75`. The runner has a restricted list for exactly two targets, `docx` (openxml, text/plain, text/markdown, text/latex, svg, png) and `typst` (typst, text/plain, text/markdown, text/latex, svg, png). Every other target gets the default list, which includes `text/html`, svg, png and `application/pdf`. Recorded mime keys confirm it (ran):

| Targets | Mime keys recorded for the Plots cell |
|---|---|
| typst, docx | `image/png`, `image/svg+xml`, `text/plain` |
| all 20 others | `application/pdf`, `image/png`, `image/svg+xml`, `text/html`, `text/plain` |

So 9b needs no runner change for docx and typst. For every other target the record carries `text/html`, and the presenter\'s "`text/html` first" rule picks it. **A runner change is a possible alternative** (make the runner give gfm, markdown and the like an image-only list); I recommend against it, because the runner is a third-party package and the rule belongs in the presenter, where Quarto 1 keeps it (`display-data.ts:45`).

### Results (ran)

| Target class | Plain route | Recorded route | Verdict |
|---|---|---|---|
| html, revealjs, dashboard | HTML page | revealjs and dashboard byte-identical; html differs by 7 lines, the interleaving of the two-expression cell | match apart from interleaving |
| gfm, commonmark, markdown, djot | `![](doc_files/figure-<fmt>/cell-4-output-1.svg)` plus the svg file | the Plots `<img src="data:image/png;base64,…">` raw html inline, no figure file | **differs** (presenter priority) |
| rst, org, native, textile, mediawiki | svg file in `figure-<fmt>` | one `data:image/png` in the raw html block, no figure file | **differs** (same cause) |
| plain, icml, fb2, docbook, rtf | svg file in `figure-<fmt>` | no figure file, no inline image | **differs**; the figure is lost on the recorded route |
| docx | `figure-docx/…svg`; `word/media/rIdN.png` | `figure-html/…svg`; `word/media/rIdN.png` | same media, different directory name and rel ids |
| typst | `figure-typst/…svg` | `figure-html/…svg` | same file, different directory name |
| epub | one `EPUB/media/file0.png` | one `EPUB/media/file0.png` | match |
| pptx | `ppt/media/image1.svg` | **no media** | **differs**; the figure is lost |
| odt | `Pictures/0.svg` | **no `Pictures/`** | **differs**; the figure is lost |

Findings the summary table does not carry:

1. **The plain route already names the directory by format** (`figure-gfm`, `figure-typst`, `figure-odt`, and so on). Only the recorded route hardcodes `figure-html` (`FigureNaming::next_file`, `presenter/figures.rs:228`). So Gordon\'s decision `figure-<format identifier>` is already what plain does; 9b only has to make the recorded route agree. This sharpens the review: the plain route is the reference, and the old doubt "which name?" is settled by what exists.
2. **The failure is the whole non-HTML family, not gfm and pptx.** Twelve of the sixteen text and archive formats I rendered differ. The review addendum named gfm and pptx; add odt, rtf, icml, fb2, docbook, plain and the wiki and markup formats. The cause is the same (the presenter takes `text/html` before images whatever the target).
3. **Three kinds of loss on the recorded route:** (a) the image is inlined as base64 in a raw html block (gfm, commonmark, markdown, djot, rst, org, native, textile, mediawiki); (b) the figure is silently dropped (pptx, odt, rtf, icml, fb2, docbook, plain); (c) the figure is kept under a wrong directory name (docx, typst). (a) and (b) are visible to authors; (c) is not.
4. `pdf` does not render at all on this tree; Julia has no `pdf` question to answer yet.

**Fact check against "facts already known":** the gfm, pptx, docx and typst results all reproduce (ran). No contradiction.

### What S1 decides

- **9b target classes:** (1) HTML-compatible targets (html, revealjs, dashboard, epub and the other html-family identifiers): unchanged; (2) docx and typst: already image-only from the runner, need only the directory name; (3) everything else: needs the `text/html` branch gated by `display_data_mime_type`, plus the directory name.
- **9e allowlist, starting contents:** html, revealjs, dashboard, epub, docx and typst are the targets that match (or match after the directory fix) today. gfm and the rest of class (3) join only after S2 shows parity. I recommend **docx, typst and epub as the first graduates**, because they need no `text/html` decision.
- **Runner change:** not needed.

## S2. The presenter fix, prototyped (ran)

*Question:* after the fix, do plain and recorded match beyond interleaving on every format, and what else changes?
*Method:* the 9b change as a patch in the scratchpad (never committed): a `target` on `CellOptions` that defaults to html, `present_cell_step` setting it from `PresentInput.target`, the `text/html` branch taken only when the target is HTML-compatible or `display_data_mime_type` returns it, and the figure directory named from the format. I used a `with_format` builder on `FigureNaming`, `FsFigureSink` and `RuntimeFigureSink` instead of changing their constructors, so the roughly 15 test callers did not have to change; the real 9b chooses. **The two-edit 7j prototype was not applied: 7j had already landed** (`959fed073` is an ancestor of this branch), so the handoff\'s premise was stale. Binary `q2-s2`, built from `c8e6fc0ca` plus the patch.

### A second hardcoded `figure-html` (ran)

The first matrix run, with only the sink patched, still wrote `figure-html` on gfm. The cause is `engine/ts_presentation.rs:83`, which computes `quarto_file_path` for TS-record engines (Julia) as `{stem}_files/figure-html/{base}-output-{kept}.{ext}`; the sink takes a preferred path over its own name (`FigureNaming::claim`). **9b\'s plan names only `FigureNaming`; it must also name `presentation_for` and its caller in `engine/ts_record.rs`.** The prototype adds a `presentation_for_format` and passes `ctx.format`.

### The matrix, 22 formats, both routes (ran)

The `records-plot` fixture as in S1, with `run.sh` rewritten (`<scratchpad>/run.sh`; the old one was lost with its scratchpad).

| Result | Formats |
|---|---|
| Same figure files and same `figure-<format>` directory on both routes, no inlined base64 | **all 22**: html, revealjs, dashboard, gfm, commonmark, markdown, docx, pptx, odt, epub, typst, rtf, rst, org, plain, native, icml, fb2, docbook, mediawiki, djot, textile |
| Before the fix, differed (S1) | gfm, commonmark, markdown, djot, rst, org, native, textile, mediawiki (inlined base64); pptx, odt, rtf, icml, fb2, docbook, plain (figure lost); docx, typst (directory name) |

What still differs between the two routes\' output files, the figures aside:

- **Interleaving**, the accepted difference: the two-expression cell is one code block on plain and two on recorded. I read the diffs of gfm (5 lines), typst (4) and native in full; for the other text formats I counted the differing lines (revealjs and dashboard 0, html 7, plain 3, fb2 2, rtf 4, docbook 8, rst, org and icml 9, textile and mediawiki 12, markdown 13, djot 14, native 17) and did not read them.
- **Random SVG clip ids** (`clip460` against `clip740`): Plots writes a fresh id per render, so the pptx and odt SVG files differ byte for byte. A comparison must strip them before it hashes media, or not hash SVG at all.
- **docx:** `rId` numbering and the media names that carry it, `core.xml` timestamps, and the **extra bookmarks: 4 `w:bookmarkStart` on the recorded document, 0 on plain**. The native output shows where they come from: the recorded route stamps `id="cell-N"` on every cell wrapper and plain stamps none (`( "cell-1" , [ "cell" ] ...` against `( "" , [ "cell" ] ...`), and pandoc\'s docx writer turns an identified div into a bookmark. So the bookmarks are the wrapper ids of OPR-7f, not a figure or caption difference (inferred from the native output and the counts, ran). **Why plain has no ids (read, ran):** plain Julia markdown comes from Quarto 1\'s `toMarkdown`, which writes the auto cell id on the wrapper only for ipynb, JATS and html-family output (`external-sources/quarto-cli/src/core/jupyter/jupyter.ts:1366-1376`, `isHtmlOutput` at `src/config/format.ts:55-74`); q2\'s `presentation_for` (`engine/ts_presentation.rs:52`) supplies the id for every target. A prototype that drops the engine id unless `target.to_html` took the recorded docx from 4 bookmarks to 0, left markdown without `{#cell-N}`, and left html, revealjs and epub with 4 of 4 ids on both routes (see "The cell-id gate" below).
- **native only:** the recorded side\'s metadata carries `record = true`, because that is how the harness turns recording on; it is not a presenter difference.
- Not inspected: the odt `content.xml` and the epub `ch001.xhtml` diffs (expected to be interleaving).

### The harness, Julia part (ran)

`gfm`, `typst`, `revealjs`, `docx` and `pptx` added temporarily to the `julia/fig-subcap` and `julia/records-plot` manifest entries (reverted), `OPR_PARITY_ONLY=julia/ OPR_PARITY_KNITR=0`, 208 s:

| Case | gfm | typst | revealjs | docx, pptx |
|---|---|---|---|---|
| `julia/fig-subcap` | interleaving only | identical | identical | not compared (zip) |
| `julia/records-plot` | interleaving only | interleaving only | identical | not compared (zip) |

The figure-directory blockers that 7j warned about for gfm, typst and revealjs are gone. docx and pptx land under "both routes failed" with `stream did not contain valid UTF-8`: the harness reads output with `read_to_string`, and 9d\'s zip extractor is what fixes it. They say nothing about parity; the hand matrix above is the only evidence for them.

### Tests (ran)

`cargo nextest run -p quarto-core` with the patch applied (and the manifest edit reverted): **8256 passed, 0 failed, 36 skipped.** No existing test pins the old behavior, so **every test in 9b\'s list is new** (the first red test is a gfm cell with an html-plus-image bundle). The `present_cell_step` signature change touched two test callers only (`cell_step.rs` and `knitr_inputs.rs`).

### Jupyter (ran)

A stored notebook (`<scratchpad>/jup/nb.ipynb`: one cell with an html-plus-PNG bundle, one with an html table, one with html only), rendered through the stored-replay path, which needs no kernel and so no venv. Before: `q2-clean`; after: `q2-s2`.

| Target | Change |
|---|---|
| html | none (byte-identical) |
| gfm | the Plots-style cell goes from a raw `<img src="data:image/png;base64,...">` to `![](nb_files/figure-gfm/cell-1-output-1.png)` plus the file; the table and html-only cells are unchanged |
| pptx | the figure appears (`ppt/media/image1.png`; the deck gains a slide); before there was no media |
| docx | the figure appears (`word/media/rId9.png`); before there was no media |

So on this fixture, **one of three outputs changes on each non-HTML target and none changes on html**. I did not run a kernel-backed notebook, the 13 shadow goldens or the parity-corpus notebooks on non-HTML targets, and I did not read the docx and pptx text beyond the media; the count is for the landing report and OPR-8\'s notes, with that limit.

### The cell-id gate (ran)

Follow-up to the docx bookmarks, after Gordon asked for the plans to be faithful to Quarto 1. Quarto 1 writes the auto cell id on the wrapper only for ipynb, JATS and html-family output (`jupyter.ts:1366-1376`; `isHtmlOutput`, `config/format.ts:55-74`: html, html4, html5, dashboard, s5, dzslides, slidy, slideous, revealjs, epub, epub2, epub3 — the same list as q2\'s `InlineTarget.to_html`). Prototype (`9b-plus-idgate.patch`, 351 lines): drop the engine\'s `id` in `present_cell_step` unless `target.to_html`. Binary `q2-idgate`, `records-plot`, both routes:

| Target | Result |
|---|---|
| docx | recorded 0 bookmarks (was 4), plain 0; the `document.xml` diff is 3 lines |
| markdown | no `{#cell-N}` on either route |
| html, revealjs, epub | 4 of 4 `id="cell-N"` on both routes |

`cargo nextest run -p quarto-core`: 8256 tests, 2 failed, both in `presenter::cell_step` because the prototype\'s test helper passed a non-HTML default target; with an html target all 12 `cell_step` tests pass. So no existing test pins the id on a non-HTML target. Not measured: knitr and native Jupyter on docx.

**What S2 decides.** 9b\'s scope grows by two (`ts_presentation.rs`\'s second `figure-html`, and the cell-id gate); its tests are all new; gfm, typst and revealjs graduate for Julia on harness evidence; docx, pptx and epub on harness evidence once 9d\'s extractor exists; the other text formats have hand-matrix evidence only.

## S3. The normalizer, prototyped (ran)

*Question:* does the design in 9d work on real pages, and which parser?
*Method:* a scratch integration test (`s3_proto.rs.txt` in the scratchpad), run on the plain and recorded html of the four real pages rendered with `q2-s2` (so `fig-subcap` has 7j and the S2 fix), after the harness\'s own `normalize_text` and the signed `julia-code-leading-newline` rule (trap 6).

### scraper fails trap 1 (ran)

Parse and re-serialize a recorded page whose `<main>` attributes are swapped in order: the raw texts differ, the round-tripped texts are equal. So a normalizer that re-serializes through `scraper` hides an attribute-order difference. `scraper` keeps no source spans, so splicing back is not possible either. My tree-edit version (move the later blocks\' `code` children into the first block, detach the rest) also failed to make `presentation-attrs` and `records-plot` equal; I did not chase why, since the trap-1 failure decides.

### lol_html works (ran)

Two passes, and every byte outside the touched blocks is kept as written:

1. `lol_html` marks, with comments: every `div.cell`; the closing `</code>` of each cell\'s first code block; and every later code block (`div.cell div.cell-code`, a **descendant** selector, not a direct child, because the `fig-subcap` group cell nests its blocks in `figure > div`).
2. Plain string splicing cuts each marked block\'s scaffold (its opening tag, the block, the copy button and the closing tag) out and appends its `code` inner html, after a newline, to the first block. The splice asserts the exact scaffold text; a writer change makes it fail loudly, which 9d must report as a blocker, never as a pass.

| Page | Fires, plain / recorded | Equal after normalization |
|---|---|---|
| `julia/presentation-attrs` | 0 / 1 | yes |
| `julia/records-plot` | 0 / 1 | yes |
| `julia/fig-subcap` | 0 / 1 | yes |
| `julia/capture-facts` | 0 / 3 | no (its four other kinds, below) |

Plain never fires (trap 4 holds), and the attribute-order control is reported, because the raw text is compared.

**The controls (ran).** Each is one mutation of the recorded page; each applicable one is reported (the normalized pair differs) on `presentation-attrs`, `records-plot` and `fig-subcap`: drop one output; swap two outputs; change one character in a split-off block; fold two cells into one; change prose outside any cell; the serialization control (swap two attributes). "Fold two cells" does not apply to `fig-subcap` and "change prose" not to `records-plot` and `fig-subcap` (my mutation found no site). **"Merge two stdout outputs into one" has no site on any real page**: I ran it on the synthetic page of 9d\'s first test (one plain cell with two stdout outputs; the recorded cell split into two blocks): the pair is equal, the merged control is reported. On `capture-facts` the controls are reported too, but the base pair already differs, so they prove nothing there.

Limits: a control is "reported" when the pair differs, not when the right line is named; nested `div.cell` elements are not supported (the counter resets); and a recorded cell whose first unit hides its code but has output would put the output before the merged code (the known gap of 9d; absent here).

### The `capture-facts` diff after the normalizer (ran)

By the unclipped line diff of the prototype (a multiset diff in both directions, as `describe_difference` is, but with no clipping), **24 lines** remain after the normalizer, down from 41 before it. The 51 of the 9d plan came from another count on the tip; I did not reproduce it. Grouped by hand from the printed lines, they fall into the four kinds that 9d\'s decision 4 names:

| Kind | Lines | What they are |
|---|---|---|
| `data-prompt` attribute | 2 | the cell\'s opening tag with `data-prompt="true"` on plain, without on recorded |
| `&gt; ` prompt prefix | 6 | three code lines on plain, the same three on recorded with the prefix |
| numeric `echo` | 4 | plain shows `a = 1`, `b = 2` and `b` in the block; recorded shows only `b = 2` (consistent with the named kind; not re-derived from the source) |
| stderr blocks | 12 | two `cell-output-stderr` blocks with their scaffold, button and closing lines |

**What S3 decides.** The parser is `lol_html`, in the two-pass form above. 24 lines is signable if the entry names the four kinds with these counts and the report attaches the whole diff.

## S4. Zip extraction (ran)

*Question:* what must be extracted and stripped to compare docx, pptx and epub?
*Method:* the draft document below, rendered to docx, pptx and epub on both routes, unzipped, run through `xmllint --format`, and diffed.

The draft hand-run document (a `println`, a labeled figure from a hardcoded PNG, a labeled table, a bare expression, and a `fig-subcap` group cell). It needs no Plots, only Julia\'s standard library:

````markdown
---
title: Zip extraction draft
engine: julia
---

```{julia}
using Base64, Markdown
struct Dot
    bytes::Vector{UInt8}
end
Base.show(io::IO, ::MIME"image/png", d::Dot) = write(io, d.bytes)
dot() = Dot(base64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="))
nothing
```

```{julia}
println("hello")
```

```{julia}
#| label: fig-dot
#| fig-cap: A one-pixel dot.
dot()
```

```{julia}
#| label: tbl-md
#| tbl-cap: A small table.
Markdown.parse("| a | b |\n|---|---|\n| 1 | 2 |")
```

```{julia}
40 + 2
```

```{julia}
#| label: fig-pair
#| fig-cap: Two dots.
#| fig-subcap: [first, second]
display(dot())
display(dot())
```
````

(My first attempt failed inside Julia on a missing `using Markdown`; the fixture above is the corrected one. All six renders exit 0.)

### Results (ran)

- **docx.** Part list identical on both routes. Differences in `word/document.xml`: (i) **extra `w:bookmarkStart`/`w:bookmarkEnd` pairs** with hashed names (`_aebbe65e…`, `_8b2cbd04…`) on the recorded route; (ii) every `w:bookmarkStart w:id`, `wp:docPr id` and `pic:cNvPr id` shifted by the extra bookmarks; (iii) the picture `descr` carries `doc_files/figure-docx/…` versus `figure-html/…` (the directory finding again); (iv) the `r:embed` ids `rId9, rId14, rId18` versus `rId11, rId17, rId21`, mirrored in `word/_rels/document.xml.rels` and in the media filenames `word/media/rIdN.png`. `docProps/core.xml` differs by the created and modified timestamps; `docProps/app.xml` is identical.
- **pptx.** Part list identical, including one `ppt/media/image1.png`; slides 1 and 3 identical; slide 2 differs only in the picture `descr` directory. `docProps/core.xml` timestamps differ. (With a PNG-only fixture pptx matches; the loss in S1 arises only when the bundle offers `text/html`.)
- **epub.** Same parts. `EPUB/text/ch001.xhtml` differs in the **`fig-subcap` group**: plain has one code block holding both `display(dot())` lines; recorded has them as two blocks (`cb8`, `cb9`), which is the interleaving/7j family of difference. `EPUB/content.opf` differs by the `urn:uuid` identifier, `dc:date` and `dcterms:modified`.

### The extractor and normalization list (what 9d writes)

Compare **text parts, canonicalized**, never the zip bytes:

| Format | Compare | Strip or normalize |
|---|---|---|
| docx | `word/document.xml`, `word/_rels/document.xml.rels`, sorted `word/media/*` by content hash | timestamps (`docProps/core.xml`, ignore the file); renumber `w:bookmarkStart w:id`, `wp:docPr id`, `pic:cNvPr id`, `rId*` and media names by first occurrence (after 9b the real pair has no cell bookmarks; one that appears is reported) |
| pptx | `ppt/slides/slide*.xml` in numeric order, `ppt/slides/_rels/*`, `ppt/media/*` by hash | core.xml timestamps; layout and master parts are the template, identical, skip them |
| epub | `EPUB/text/*.xhtml`, `EPUB/nav.xhtml`, `EPUB/media/*` by hash | `content.opf` identifier, date and modified; `toc.ncx` uid |

Two things the normalizer cannot hide, and which 9b removes (S2): the extra **docx bookmarks** on the recorded route (the cell-wrapper ids; a presenter fix, below) and the **directory name** in picture descriptions. Extraction itself is cheap: `unzip -p` plus `xmllint --format`, or the `zip` crate already in the tree; the harness\'s `read_to_string` is what must change (it fails on binary and so reports "both failed").

## S5. Reserved option keys (ran, read)

*Question:* is `quarto-unit-id` the only author-reachable key the epics made reserved?
*Method:* a Julia document whose cells carry `#| quarto-unit-id`, `quarto_attrs`, `quarto_expansion`, `quarto_output_kinds`, `quarto_file_path` and `quarto_markdown_passthrough`, rendered with `record: true`, then the cell metadata read from the record and the HTML compared (`<scratchpad>/s5`). Julia only; **I did not run a Jupyter cell** (it needs the Jupyter venv), but Jupyter takes the same `#|` path through `ts_record.rs`\'s cell metadata.

Results:

| Key written as `#|` | Reaches record metadata? | Changes output? |
|---|---|---|
| `quarto-unit-id` | yes, kept as written (`"abc"`) | no on Julia; **knitr aborts with "Duplicate map key"** because core injects its own `#| quarto-unit-id` (`engine/knitr/chunk_records.rs:36,93`) |
| `quarto_attrs` | the runner\'s value wins (`id: cell-3`); the author\'s id and classes are gone | no |
| `quarto_output_kinds` | overwritten by the runner (`[null]`) | no |
| `quarto_expansion` | yes, kept (`{"index":5}`) | no: the presenter reads `quarto_expansion` off *outputs* (`presenter/expansion.rs:29`), not off cell metadata |
| `quarto_file_path`, `quarto_markdown_passthrough` | yes, kept | no: both are read off *output* metadata (`presenter/cell.rs:762, 368`) |

The span-id path is not author reachable (read): ids come from the shared unit list and are never generated or read from the author\'s text (`knitr/span_records.rs` header and `ids_come_from_the_shared_unit_list`).

**Answer:** `quarto-unit-id` is the only key that does harm today, and only on knitr. The other `quarto_*` keys an author can type are stored in cell metadata but inert, because the presenter reads them from output metadata, which an author cannot set. **9c\'s scope is one key**, with one caveat for Gordon: the keys are inert *today*; a cheap defense is to reserve the whole `quarto_` and `quarto-` prefix (a diagnostic naming the prefix), and I would do that only if he wants it.

## S6. Browser replay (read)

*Question:* does the flip add a runtime path to the WASM client?
*Method:* static, as the plan says. Not built; no WASM run.

- The WASM crate builds no pipeline of its own for engines; `render_page_for_preview` and the hub entry points run `EngineExecutionStage`, whose engine registry carries only `ReplayEngine` for a captured engine, and the other engines cannot run (no subprocess) (`wasm-quarto-hub-client/src/lib.rs:1460`).
- The flag\'s only consumer in the stage is `if want_record || self.present_from_record` (`stage/stages/engine_execution.rs:894`), which calls `record::coalesce`. A replayed Jupyter capture carries a record (`ReplayEngine` docs, `replay.rs:18-25`), and the stage pushes any `result.record` into `engine_records` (`:795-802`) whatever the flag says. So **after the flip, the WASM path coalesces a replayed record where before it did not** (in the browser `NullObserver` wants no record, so `want_record` is false there; the preview recorder wants one but is native).
- `coalesce` fails with `UnknownCell` or `DuplicateCell` when a record\'s cell ids are not in the unit list (`record/coalesce.rs:55-66`). The stage computes the units in its one scan before any engine runs, so the unit list is present on replay; whether a *stored* capture\'s ids still match the current unit-id derivation is the open point. Captures older than the current hash rule could now fail with "execution record: unit … was not found" instead of rendering.
- Evidence it works on native: the flip experiment (9096 tests, 3 failures, none in replay) ran every `ReplayEngine` test in `quarto-core` through the new path (relied: review addendum).

**What it decides:** 9g\'s WASM box is not "compile and smoke" strictly. It should add **one test: replay a Jupyter capture recorded before the flip through `render_page_for_preview` and assert it renders**, because a stale capture is the only way the new path can fail. I did not build the WASM to run it; that is the order the plan gave (only if unclear), and it is unclear, so it is the first thing 9g should run, not 9a.

## Decisions

Items 1 to 5: settled by me, as the handoff delegated them after S2 and S3; Gordon can overrule each.

1. **Which formats graduate for Julia (9e).** On harness evidence after 9b: html, revealjs, dashboard, gfm and typst. With 9d\'s zip extractor: epub, docx and pptx (their hand-matrix results already match). The other text formats (commonmark, markdown, djot, rst, org, native, textile, mediawiki, plain, icml, fb2, docbook, rtf) have hand-matrix evidence only: graduate them when they are added to the harness\'s comparable list and show interleaving alone; odt after the extractor handles it. Everything else, and `pdf`, stays gated. This supersedes the earlier recommendation (docx, typst and epub first): gfm needs no extractor and is verified.
2. **The gate under `want_record`: it applies.** One rule: asking for a record never changes the output route (the record is still built and written). S2 shows the recorded route is right on all 22 measured targets, so the gate now protects only the unmeasured tail (json, ms, man, texinfo, tei, the wiki families, the slide families) and any target without a parity case; `offers_presentation` tests the allowlist, not only `builds_record()`.
3. **Jupyter on non-HTML targets: accept, as a separate bugfix** (Gordon\'s earlier ruling). On the S2 fixture a Plots-style cell goes from an inlined base64 image (gfm) or no figure at all (pptx, docx) to a figure file; html is byte-identical. State it in the landing report and in OPR-8\'s notes, with the limits in S2 (no kernel-backed run).
4. **The pin\'s form: a file beside the allowlist, not a TOML literal.** The 24 lines are html with long, span-laden lines; a triple-single-quoted literal in the allowlist is unreadable in review and breaks on a triple quote in the text. The entry gets `pin = "<name>.pin"` (a path relative to the allowlist file, valid only with `document`), its `reason` states the four kinds with their line counts (2, 6, 4 and 12), and the gate report attaches the whole diff. The pin is compared exactly and unclipped.
5. **The post-flip rule for the gate-time entries.** Unchanged from 9g: remove them at the flip, keep the normalizer code and tests, assert identity with an empty allowlist.

Settled by Gordon (items 6 to 8):

6. **The `quarto-unit-id` diagnostic is an error** (9c), as the plan has it: a chunk that sets the key fails the render with a diagnostic naming the key as reserved. Gordon confirmed it over the warning that drops the author\'s line.
7. **The docx bookmarks are a presenter fix, faithful to Quarto 1** (Gordon, 2026-11-10: make it faithful to Quarto 1). Quarto 1 writes the auto cell id on the `.cell` wrapper only for ipynb, JATS and html-family output, and a label that labels the container for every target; 9b gates the engine\'s id by target the same way (`target.to_html`), so the bookmarks disappear and 9d needs no bookmark rule. This replaces my earlier recommendation to normalize them in the extractor.

8. **9c\'s scope is one key, on knitr chunks** (Gordon, 2026-11-10). `quarto-unit-id` is the only key that does harm; the check lives in `insert_unit_ids`, which only builds the knitr record, so a Julia or Jupyter cell that sets it is not diagnosed (inert there; S5 did not run a Jupyter cell). Reserving the whole `quarto_` and `quarto-` prefix is deferred until another key bites.

Nothing is open for Gordon in this note; 9e is still `conditional` until he sets it `ready`.

## What was not checked

- A kernel-backed Jupyter notebook, the 13 shadow goldens and the parity-corpus notebooks on non-HTML targets (S2 used one stored notebook).
- The docx and pptx text beyond media and bookmarks; the odt `content.xml` and epub `ch001.xhtml` diffs; most text-format diffs were counted, not read (S2).
- The formats under "Skipped" in S1 (json, ms, man, texinfo, tei, the wiki families, the slide families).
- The WASM build and its two vitest files (S6); the Windows cross-clippy and `cargo xtask lint` on the prototype (it was never committed).
- A knitr cell and a Jupyter cell for S5.
- `transforms/hephaestus.rs` (`ARTIFACT_DIR = "figure-html"`) for an effect on non-HTML formats (9b\'s item).
- Whether S1\'s lost figures (pptx, odt, rtf) were lost by the presenter or by the writer: moot after S2, which fixes them through the presenter.
