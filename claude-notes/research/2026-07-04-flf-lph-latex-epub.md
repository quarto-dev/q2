# FLF/LPH research: LaTeX and EPUB in q2 via Pandoc + Quarto 1's Lua

**Date:** 2026-07-04 · **Worktree:** `.claude/worktrees/flf-lph-research`
**Plan:** `claude-notes/plans/2026-07-04-flf-lph-format-research.md`
**Spike artifacts:** `spikes/flf-lph/` (fixtures, capture shim, replay harness, outputs)

## Executive summary

- **FLF (Formats are Lua Filters)** is *overstated product-wide* but *strongly
  true for LaTeX and EPUB*. Across all of quarto-cli, format-specific code is
  ~10.5K LOC Lua vs ~16.6K LOC TypeScript — but the heavy TS formats are
  HTML/reveal/dashboard (styling + JS-asset plumbing). For LaTeX every content
  transformation is Lua (~2.5–3K LOC); the TS is the latexmk compile loop
  (~2.6K) plus ~30 `.tex` line-postprocessors. For EPUB there is almost no
  format-specific code at all (51 LOC TS, ~70 LOC Lua): EPUB rides the HTML
  Lua path (`isHtmlOutput()` is true for `epub*`) and Pandoc's built-in EPUB
  writer does the container.

- **LPH (Lua Pandoc Hypothesis) is ~95% true, verified empirically.** Q1's
  filters already run inside Pandoc — one `main.lua` driving an emulated
  custom-AST chain. A standalone replay of the captured invocation (no Q1
  TypeScript anywhere) produced a **byte-identical `.tex`** and a
  **content-identical `.epub`**. Nothing about the Lua needs porting; it needs
  *hosting*: a defaults file, a metadata file, one env var of params, a
  staged template directory, and Q1's pandoc datadir.

- **The q2 integration route works today**: `pampa -t json` → `pandoc -f json`
  with `[q2-compat.lua, main.lua, citeproc]` produced `.tex` **byte-identical
  to Quarto 1's output**. Exactly **one** AST-convention mismatch surfaced
  (labeled display equations), fixed by a **23-line additive adapter filter**
  — zero changes to Q1's filters. This directly answers "does q2's coarser Lua
  hook surface hurt?": for these formats, **no** — the format Lua runs inside
  pandoc, not in q2's engine, so q2's Pre/Post hook surface is not even
  involved.

- **PFR (port to Rust vs keep as Lua):** the whole in-pandoc chain (all Q1
  filters + citeproc + LaTeX writing) costs **~0.2s** per document; pampa's
  parse is 0.01s and lualatex is seconds. Porting the format Lua to Rust
  would optimize a component that is <5% of PDF wall time. The measurements
  support keeping the Lua.

## 1. FLF quantification (Quarto 1 source)

Methodology: `wc -l` on `.lua` under `src/resources/filters/` (217 files,
34,098 lines) and `.ts` under `src/format/` + latexmk; format-specific Lua =
whole files named for a format + grep-counted gated branches in shared files.

| | Lua | TypeScript |
|---|---|---|
| Format-specific LOC (whole product) | ≈10,500 (31% of filter Lua) | ≈16,600 |
| Biggest Lua formats | Dashboard ~2,700; Typst ~2,400; LaTeX ~1,750 (plus shared-file branches → ~2.5–3K) | HTML 6,966; reveal 2,482; dashboard 2,429 |
| LaTeX/PDF | ~2.5–3K (post/latex 642, layout/latex 760, floatreftarget latex branch ~300, callout/theorem/cites/crossref branches, latexcmd/latexenv/latexdiv/tikz) | 2,592 — `format-pdf.ts` 1,388 + latexmk 1,204, i.e. compile-loop + option plumbing, near-zero content transform |
| EPUB | ~70 (callout renderer shared with reveal ~55; pagebreak, `epub:type=appendix`, tabset fallback, sections gate) | 51 (`format-epub.ts`) + ~130 book-integration lines |

Character split: format work is three-way — Lua does AST→format *content*
transformation; TS does option plumbing, SCSS/theme compilation, and external
engine orchestration; templates/SCSS (~50K hand-authored lines, neither) do
presentation. Pandoc's own writers (the largest share of real format logic)
are external and counted nowhere.

Verdict: "the bulk of format-specific code is Lua" is false at whole-product
level by raw LOC, but true for the job that matters to porting LaTeX/EPUB:
everything Quarto adds *on top of Pandoc's writer* is Lua.

## 2. The Q1 invocation contract (captured, not inferred)

Captured by pointing `QUARTO_PANDOC` at a recording shim
(`spikes/flf-lph/pandoc-shim.sh`) during `quarto render --to latex|epub3`.
The main pandoc call is:

```
pandoc --defaults <tmp>/quarto-defaults.yml <tmp>/quarto-input.md \
       --metadata-file <tmp>/quarto-metadata.yml \
       --data-dir <quarto-cli>/src/resources/pandoc/datadir
```

with env `QUARTO_FILTER_PARAMS` (base64 JSON: crossref/callout titles,
language strings, `quarto-filters` list, format-identifier, paths incl.
TinyTexBinDir, results/dependency file paths) and
`QUARTO_FILTER_DEPENDENCY_FILE`. The defaults file carries: `from:
qmd-reader.lua`, `to`, `output-file`, `filters: [main.lua, citeproc]`,
`template` (a TS-patched copy staged in a temp dir next to ~22 partials),
syntax highlighting theme + definitions; for EPUB instead: no template, two
`include-in-header` style files, `html-math-method: mathml`. The input `.md`
is the qmd body with front matter lifted into `--metadata-file` (markdown
engine case). The datadir's `init.lua` additionally requires
`QUARTO_SHARE_PATH` to locate `_format.lua` etc.

Also observed: a cheap pre-pass pandoc call (`-f markdown -t markdown -L
leveloneanalysis.lua`) that Q1 uses to decide heading auto-shift — in q2 this
is a native AST query, not a subprocess.

## 3. Spike results

All artifacts under `spikes/flf-lph/`; replay driver is `replay.py`.
Fixture 1 (`fixture/doc.qmd`): callouts (incl. collapse), labeled
figure/table/equation/theorem + crossrefs, decorated code block,
column-margin note, citation, pagebreak shortcode, layout-ncol panel.
Fixture 2 (`fixture2/doc.qmd`): adversarial — `reference-location: margin`,
`citation-location: margin`, footnote-in-caption, code annotations `# <1>`,
margin table.

| Spike | Result |
|---|---|
| Replay LaTeX, Q1 reader+filters, no TS (fixture 1) | **byte-identical** to Q1's `.tex` |
| Same, system pandoc 3.8.1 instead of bundled 3.8.3 | **byte-identical** |
| Compile replayed `.tex` with TinyTeX lualatex ×2 | doc.pdf produced (38KB); inspected — callouts/tcolorbox, crossref numbers, marginnote, theorem, bibliography all present |
| Replay EPUB, no TS (fixture 1) | identical except pandoc's own random UUID + timestamps |
| Replay LaTeX (fixture 2, postprocessor triggers) | 42-line diff vs Q1 = exactly the TS postprocessor work (see §4) |
| **pampa JSON → pandoc + Q1 chain** (fixture 1, latex) | **byte-identical to Q1** after one 23-line adapter filter (`q2-compat.lua`) + 2 orchestrator-side stubs (a `quarto_pandoc_reader_opts` meta key; an empty dependency file) |
| pampa JSON route, fixture 2 | byte-identical to the qmd-reader replay (same distance from Q1 = TS postprocessors only) |
| pampa JSON route, EPUB | content-identical; one cosmetic delta: pampa keeps `id="tbl-simple"` on the `<table>` (Q1 drops it) |
| Timings | in-pandoc chain (all filters + citeproc + writer): **~0.2s**; pampa parse: 0.01s; Q1 end-to-end `--to latex`: 0.85s; lualatex: seconds |

The single genuine AST-convention mismatch: pampa parses
`$$...$$ {#eq-euler}` at parse time into
`Span(id, ["quarto-math-with-attribute"], [Math])`, while Q1's
`crossref/equations.lua` expects raw `Math , Space , Str "{#eq-euler}"`
tokens. `q2-compat.lua` re-expands the span; it runs inside pandoc *before*
`main.lua` via the defaults filter list. Additive, no Q1 filter edited. The
`quarto-shortcode__` span encoding, Pandoc-3 `Figure` blocks, tables,
callout divs, layout attributes — all of pampa's other output was consumed
by Q1's normalize/parse chain unchanged. pampa's extra JSON fields (`s`
source ids, duplicate `a` attr objects) are ignored by Pandoc's JSON reader.

## 4. What does NOT fit LPH (the honest remainder)

Everything below is TypeScript work the pandoc leg cannot do, measured either
by the fixture-2 diff or by inventory. This is the part q2 must own natively:

1. **`.tex` line postprocessors** (~30 `LineProcessor`s in `format-pdf.ts`,
   ~900 LOC + drivers). Observed firing in fixture 2: margin citation
   resolution (`{?quarto-cite:id}` → `CSLReferences` inside `\marginpar`, plus
   suppressing the end-of-document bibliography), `\footnote` → `\sidenote`
   for `reference-location: margin`, code-annotation `# <1>` →
   `\hspace*{\fill}\circled{1}` and list labels `5CB6E08D-list-annote-N` →
   `\circled{N}`. These are deliberate deferrals: the Lua *emits markers*
   because the rewrite needs post-citeproc, post-template text. Port target:
   a small Rust "tex-postprocess" stage (they are regex/state machines over
   lines — mechanical to port), or investigate moving some to a Lua filter
   placed *after* citeproc in the pandoc chain (possible for the sidenote and
   annotation rewrites; the longtable/caption ones genuinely want writer
   output).
2. **PDF compile loop** (latexmk family, ~1,700 LOC TS): run engine, parse
   log, tlmgr auto-install missing packages, biber/makeindex, rerun-until-
   stable. Pure orchestration; q2 needs this in Rust regardless of PFR.
3. **Format option assembly** (`createPdfFormat`/`formatExtras`, part of
   format-pdf.ts): KOMA class defaults, caption placement, heading
   auto-shift, template+partials staging, pandoc defaults construction, the
   params-JSON channel. In the spike this came for free from the capture;
   a real q2 implementation regenerates it (typed config → defaults yml +
   metadata + params). Modest, boring code.
4. **EPUB plumbing** (~130 LOC): math-method per epub2/epub3, two
   include-in-header styles, `epub-cover-image` derivation; book single-file
   chapter merge + Quarto-side crossref pre-resolution (project feature,
   out of spike scope).
5. **Resource staging**: Q1's datadir (`init.lua`, `_format.lua`, readqmd),
   `src/resources/filters/**` (~34K LOC Lua), pdf template + 22 partials,
   syntax themes/definitions — q2 must vendor these as local resources per
   the external-sources policy (they'd be a `resources/q1-filters/` import,
   like `resources/scss/`).

Quantified: LPH covers all ~2.5–3K LOC of LaTeX Lua and all EPUB Lua
verbatim, plus citeproc; the non-LPH remainder is ~2.6K LOC of TS to port
(compile loop + line processors) + option assembly, all of it orchestration
rather than document semantics.

## 5. Answers to the framing questions

- **"Does q2's smaller Lua hook surface hurt our filters?"** Not for
  pandoc-written formats: under LPH the Q1 format filters run inside the
  pandoc subprocess with Q1's own seven-group chain intact, so q2's two-hook
  engine isn't in the loop. User filters targeting these formats can be
  injected into the pandoc leg via the params `quarto-filters`
  entry-point arrays (`beforeQuartoFilters`/`afterQuartoFilters`), which the
  contract already supports.
- **"Were fixes to the Lua filters needed?"** One additive 23-line adapter
  for the equation-label convention; zero edits to Q1 filters. The AST model
  mismatch is minimal and is *convention*, not structure. (Watch item: the
  `<table id=...>` nuance; likely benign or an improvement.)
- **PFR:** the Lua costs ~0.2s/doc inside a format whose compile step costs
  seconds. Rust port buys nothing user-visible for LaTeX/EPUB/docx/pptx.
  (Typst remains the exception if a native pampa writer materializes, per
  Gordon's prior.)

## 6. Design questions to discuss (held, per the brief)

1. **Who owns crossrefs/callouts for pandoc-written formats?** The spike fed
   pampa's *raw* parse to the Q1 chain, letting Q1's Lua do
   normalize/crossref/layout. q2's native transforms (crossref_index/resolve,
   CalloutTransform, FloatRefTarget) would be *skipped* for these formats.
   Alternative — lowering q2's resolved AST to Q1 conventions — means a much
   bigger adapter. Skipping keeps fidelity but risks HTML-vs-LaTeX divergence
   (numbering, prefixes) between the two engines; needs a decision on which
   is the long-term source of truth.
2. **Old vs new qmd syntax.** The pampa-JSON route (recommended) parses with
   pampa, so new-syntax documents work; the alternative qmd-reader route
   would freeze these formats on Q1's dialect. But: any construct pampa
   parses into *new* conventions needs an adapter-filter entry (equations are
   the existence proof — what else? needs a differential sweep over a bigger
   corpus).
3. **Vendoring**: importing ~34K LOC of Q1 filter Lua + datadir + templates
   into `resources/` — acceptable? Update cadence vs quarto-cli upstream?
4. **Pandoc as a dependency**: bundled (like Q1) vs system-discovered; the
   spike showed 3.8.1/3.8.3 interchangeability but a pinned floor is surely
   needed.
5. **Where do the line postprocessors go** — Rust port (mechanical) vs
   post-citeproc Lua where feasible?
6. **`FormatIdentifier` shape**: no `Latex` variant exists yet; `--to latex`
   currently parses as `Custom` and bails at the `is_native()` gate
   (`crates/quarto/src/commands/render.rs:626`).

## 7. Caveats

- Two small fixtures, markdown engine only; no executable cells, no books/
  projects, no beamer/ConTeXt, no extensions. Q1 = dev checkout 99.9.9 at
  `/Users/gordon/src/quarto-cli` (the user's symlinked `quarto`); pandoc
  3.8.x only.
- The replay reuses Q1-generated defaults/metadata/params as a stand-in for
  q2's future option assembly; the spike proves the *contract* is drivable
  without Q1's TS, not that q2 already builds it.
- `epub-baseline` PNG binary diff in the pampa-EPUB comparison is an
  artifact of regenerating the fixture image mid-spike (luatex rejected my
  first hand-rolled PNG), not a pipeline divergence.

## Reproduction

```bash
cd spikes/flf-lph/fixture
# capture (writes capture/<fmt>/call-N/)
REAL_PANDOC=<quarto-cli>/package/dist/bin/tools/aarch64/pandoc \
  CAPTURE_DIR=$PWD/../capture/latex QUARTO_PANDOC=$PWD/../pandoc-shim.sh \
  quarto render doc.qmd --to latex
# replay without Q1 TS
python3 ../replay.py ../capture/latex/call-2 ../replay/latex --fixture . \
  --partials /Users/gordon/src/quarto-cli/src/resources/formats/pdf/pandoc \
  --env QUARTO_SHARE_PATH=/Users/gordon/src/quarto-cli/src/resources/
diff ../replay/latex/doc.tex ../capture/latex/q1-baseline-doc.tex  # empty
# pampa route: see replay/q2json/ (defaults.yml has q2-compat.lua prepended)
```
