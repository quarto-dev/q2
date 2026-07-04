# FLF/LPH research: LaTeX, EPUB, docx, pptx in q2 via Pandoc + Quarto 1's Lua

> Round 2 (same day) added: §8 FLF percentages scoped to LaTeX, §9 docx/pptx
> inventories + spikes, §10 postprocessor AST-portability audit (Carlos's
> no-postprocessors goal).

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

---

# Round 2 addenda (2026-07-04)

## 8. FLF quantified on LaTeX specifically

Buckets: (a) Lua filters; (b) TypeScript a Rust port must own; (c) other
(templates/partials/schema). All LOC measured on `/Users/gordon/src/quarto-cli`
(the dev checkout actually driving the spikes). LaTeX-specific Lua was
measured per-branch this round (not estimated): whole files 1,717 + measured
latex branches in shared files 1,088 = **2,805** (largest shared branches:
floatreftarget 376, cites 190, columns 154; callout.lua contributes 0 — its
latex renderer lives inside quarto-post/latex.lua).

**Raw accounting (everything LaTeX/PDF-specific, incl. beamer templates):**

| Bucket | LOC | Share |
|---|---|---|
| (a) Lua filters | 2,805 | **32%** |
| (b) TS → Rust | 3,948 = format-pdf.ts 1,388 + latexmk/ 2,237 + output-tex.ts 246 + config/pdf.ts 77 | **44%** |
| (c) Other | ~2,124 = pdf template+partials 788 + beamer resources 1,147 + dedicated schema (latexmk 78, pdfa 111) ≈ 189 (+ scattered latex-tagged entries in shared schema files, not separable) | **24%** |
| Total | ~8,877 | |

**But bucket (b) is three very different things:**

| TS sub-bucket | LOC | Nature |
|---|---|---|
| PDF compile orchestration (latexmk + output-tex + config/pdf) | 2,560 | run lualatex, parse log, tlmgr auto-install, biber/makeindex, rerun loop. Not document transformation; must be Rust regardless of PFR; ~29% of everything |
| `.tex` line-postprocessors (in format-pdf.ts) | ~885 | document transformation done as post-writer text rewriting — the subject of §10 |
| Format definition / option assembly / KOMA / PDF-standard | ~560 | config plumbing → Rust structs/yaml, boring |

**The framing that answers "how FLF is LaTeX":** restrict to *document
transformation* code (what turns AST constructs into LaTeX constructs) —
Lua 2,805 vs TS postprocessors 885:

> **Lua is 76% of LaTeX document-transformation code today; and if the
> §10 migration is done, ~95% (residual Rust text-pass ~150–200 LOC).**

The other 24%/56% of raw LOC is compile-loop machinery, option plumbing, and
templates — code that exists in TS today but is not "format semantics."

## 9. docx and pptx (EPUB replaced as study cases)

### Inventory + FLF split

| | docx | pptx |
|---|---|---|
| (a) Lua filters | **~650–700** (quarto-post/docx.lua 203, layout/docx.lua 124, modules/openxml.lua 34, layout/wp.lua 68, callout.lua docx branches ~96, floatreftarget docx/odt renderer ~72, table.lua ~20, docxCalloutImage ~25, landscape ~18, pagebreak/shortcodes/mediabag ~14) | **~52 live** (post/pptx.lua RawBlock fixup 15, floatreftarget pptx renderer 19, output-unroll gates 9, pagebreak no-op 2, _format helper 5) + **30 dead** (layout/pptx.lua `pptxPanel` — defined, never called; drop in port) |
| (b) TS → Rust | **~65** (format-docx.ts 46 — option assembly + 5 callout-icon paths into filter params; createWordprocessorFormat share ~17) | **~21** (powerpointFormat() defaults 17 + base.ts cell-output unrolling 4) |
| (c) Other | 5 icon PNGs + ~7 schema lines; **no default reference-doc** | **nothing** — no reference-doc, no template dir, ~5 shared schema entries |
| TS postprocessors on output | **none** (no zip/OOXML rewriting anywhere) | **none** |
| FLF on transformation code | **~100% Lua** | **~100% Lua** |

Mechanism: all docx/pptx-specific rendering is Lua emitting
`RawBlock("openxml", ...)` islands (callout tables, styled captions, section
breaks, pagebreaks) that Pandoc's writers serialize. The only non-Lua duties:
docx needs the 5 icon paths resolved into `QUARTO_FILTER_PARAMS`
(`param("icon-<type>")` in Lua silently drops icons if absent), and pptx
needs the engine-time cell-output unrolling replicated (q2's executed-cell
assembly, pre-pandoc).

### Spikes (same harness as §3)

| Spike | Result |
|---|---|
| Capture Q1 `--to docx` / `--to pptx` | Same minimal contract as latex, minus template: defaults (to/output-file/filters/from/syntax) + metadata + params env + datadir. Not even a reference-doc. |
| Replay docx standalone (no Q1 TS) | **identical to Q1's .docx except `docProps/core.xml` timestamps** — every other zip entry byte-identical, incl. word/document.xml (verified per-entry `cmp`) |
| Replay pptx standalone | **identical except core.xml timestamps**; all 4 slides byte-identical |
| Content check | docx document.xml shows Quarto styles (ImageCaption, CaptionedFigure, callout tables, Bibliography) — the Lua chain demonstrably did the format work |

LPH verdict for docx/pptx: **total** — cleaner than LaTeX (no compile loop, no
postprocessors, no template staging). Both formats are pandoc-writer +
Lua-emitted-OOXML; a q2 orchestrator needs only option assembly and (docx)
icon params / (pptx) cell unrolling.

## 10. Postprocessor audit vs "everything on the AST" (Carlos's goal)

Scope note: **docx, pptx, epub have zero output postprocessors in Q1** — the
goal is already satisfied there. LaTeX is the only offender among these
formats: 20 `LineProcessor`s (~885 LOC TS) run over the generated `.tex` in
two passes. (The PDF compile loop is not content post-processing and is out
of scope of the goal.)

Full catalog with producers and line ranges is in the round-2 agent output;
summary classification:

**Class 1 — AST-portable today, mechanically (7):** sidecaption wrapper (#1),
biblatex/natbib refs-div placement (#5, #6), `{?quarto-cite:}` → `\fullcite`
/ `\bibentry` (#8, #10), footnote→sidenote (#14), code-annotation list labels
(#17). Each replaces a marker the Lua itself emitted with static or
locally-computable LaTeX — the Lua could emit the final form directly. These
exist as text rewrites for historical reasons, not technical ones.

**Class 2 — AST-portable after citeproc (4):** guid relocation (#4),
bibliography indexing/suppression (#11), refs-chapter cleanup (#12),
margin-citation entry placement (#20). These need citeproc-*rendered*
entries; but citeproc output IS AST (a `#refs` Div of `CSLReferences`), and
Pandoc runs filters in declared order — a Lua filter listed *after* citeproc
sees it. `filters: [main.lua, citeproc, margin-cites.lua]` keeps everything
on the AST, inside the same pandoc invocation.

**Class 3 — portable with effort, because the AST has context the text pass
lacks (6):** callout float `[H]` forcing (#2 — at AST time we KNOW a float is
inside a Callout; the text pass reverse-engineers env nesting), caption
footnotes → footnotemark/text (#15 — Note nodes are visible inside caption
inlines), sidenotes inside tables/longtables (#18, #19 — Note-inside-Table is
an AST query), code-annotation `\circled{N}` in highlighted code (#16 —
post-writer in Q1 only because *Pandoc's* skylighting runs in the writer; q2
owns its own highlighting stage, so q2 can highlight at AST time and emit the
final tokens), template `\printbibliography`/`\bibliography` suppression
(#7, #9 — properly belongs in the template as an `$if()$` conditional, not in
any processor).

**Class 4 — genuinely fights Pandoc's writer (2):** margin-longtable column
width rewriting (#3 — patches the writer-computed
`p{(\columnwidth - N\tabcolsep)...}` specs to account for
`\marginparwidth`) and longtable bottom-caption relocation (#13 — reorders
lines inside the writer-generated longtable preamble). The information needed
(final column specs, caption line position) is *created by* Pandoc's
longtable writer; no pre-writer transform can see it. Options: (i) keep a
minimal Rust text-pass for exactly these (~100–150 LOC); (ii) have the Lua
emit the entire longtable as raw LaTeX for the margin/bottom-caption cases,
bypassing the writer (floatreftarget.lua already uses the
`pandoc.write(...,"latex")` + patch idiom for nested-longtable fixups, so
this is an established pattern, not a new invention); (iii) upstream writer
options to Pandoc (slow).

**Verdict on the laudable goal:** For docx/pptx/epub — already true, nothing
to do. For LaTeX — **18 of 20 processors can move onto the AST** (7
mechanically, 4 as post-citeproc Lua, 6 with modest effort +1 to the
template), leaving a hard core of 2 longtable processors where "the AST"
simply doesn't contain the writer's layout decisions. Recommendation: do the
Class 1+2 migration when vendoring the filters (it *simplifies* them —
markers and their consumers collapse into direct emission), take Class 3
opportunistically, and either keep a ~150-LOC Rust tex-text pass for Class 4
or adopt the raw-longtable-emission idiom and eliminate text processing
entirely. Caveat: every Class 1–3 migration is a divergence from upstream
quarto-cli's filter code — it trades "diff-able against Q1" for architectural
purity; batch them deliberately, not ad hoc.

### Design questions raised this round

7. Migrating Class 1–3 processors into (vendored) Lua diverges from upstream
   Q1 filters — how much do we value staying diff-able against quarto-cli?
8. Class 4: minimal Rust text-pass vs raw-longtable emission from Lua?
9. pptx: adopt Q1's engine-time cell unrolling into q2's executed-cell
   assembly (it is engine-side, not filter-side).

## 11. Design questions gating a "LaTeX 100%" epic (round 3)

Premise: add latex/pdf as a real q2 format; migrate line-processors
(including effortful ones) to AST transforms; port the driver TS to Rust
meticulously; template partials through citeproc; format extensions out of
scope.

**Q0 — MASTER: who owns document semantics for pandoc-written formats?**
- Route A (spike route): serialize pampa's *raw* parse; the vendored Q1 Lua
  chain owns normalize/crossref/callouts/floats/layout inside pandoc.
  Maximum fidelity (proven byte-identical), but q2 carries two crossref/
  callout implementations forever (Rust for HTML, Lua for pandoc formats),
  and "AST transformers" for the migrated processors means *Lua against Q1's
  custom-AST runtime*.
- Route B: q2's native transforms own semantics; lower the resolved q2 AST
  to LaTeX. One semantics, but Q1's 2.8K LaTeX Lua expects Q1 custom nodes —
  this is a rewrite (PFR option 1 by the back door).
- Route C (recommended): A plus an explicit HTML-vs-LaTeX conformance suite
  pinning numbering/labels/prefixes, and a declared long-term direction.
  Decides: language of every migrated processor, adapter size, book design.

**Q1 — which citeproc?** q2 has a native Rust citeproc (used for HTML);
Q1's chain runs pandoc's citeproc as the last filter, and the margin-cite
machinery (Class 2) keys off its `CSLReferences` output. Keep pandoc
citeproc in the leg (fidelity; biblatex/natbib modes bypass citeproc
entirely and lean on the template) or route citations through q2's native
citeproc pre-serialization (one engine, but the Class-2 migrations change
shape and HTML/LaTeX bibliography rendering may diverge)? Gates the
bibliography plan and Class-2 processor migration.

**Q2 — vendoring & divergence policy.** ~34K LOC of Q1 filter Lua + datadir
+ templates land in resources/. Fork-and-own (freeze a baseline, migrate
processors in our copy) vs upstream-first (land the processor→AST
migrations in quarto-cli itself — we control it — so Q1 sheds its
postprocessors too, then vendor the result)? Upstream-first keeps the trees
diff-able and de-risks Q1; fork-and-own is faster. Gates sequencing of
every migration plan.

**Q3 — where the migrated transforms run** (follows Q0/Q1): pre-citeproc
additions to main.lua vs post-citeproc filters in the chain vs q2-side Rust.
And the Class-4 pair (margin-longtable widths, bottom captions): raw-
longtable emission from Lua vs a ~150-LOC Rust text pass. "100% AST" is
only reachable via the former.

**Q4 — pandoc distribution.** Bundle a pinned pandoc (Q1-style; +~50MB per
platform; GPL distribution) vs discover system pandoc with a min-version
gate (spike showed 3.8.1/3.8.3 interchangeable, but no floor is
established). Gates release runbook, installers, CI matrix.

**Q5 — format identity & config.** Add `FormatIdentifier::Latex` (+ `Pdf`
as latex+compile recipe); is beamer in the epic? ConTeXt (explicitly out?)?
Port Q1's latex/pdf YAML schema (latexmk 78 + pdfa 111 + scattered entries)
into q2's config validation — as part of this epic or a shared
schema-porting effort?

**Q6 — params channel.** Replicate `QUARTO_FILTER_PARAMS` byte-compatibly
(recommended — filters run unchanged) — but q2 must *compute* the params:
crossref/callout titles come from Q1's language yml (i18n resources port),
paths from tool discovery (TinyTeX). Which params does q2 own vs stub?

**Q7 — templates.** What does Q1's template patching actually do
(template.patched vs template.tex — analyze patchTemplate)? Are user
`template-partials` supported day one? Is brand.yml → LaTeX in scope?

**Q8 — who highlights code for LaTeX?** Pandoc's skylighting (Q1 path;
keeps the annotation processor post-writer-shaped) vs q2-native
highlighting emitting raw LaTeX at AST time (aligns with the AST goal,
makes processor #16 portable, but changes output vs Q1 and owns the
`Shaded`/`Highlighting` env + ANSI-color definitions). Gates the
code-annotation and ANSI-fixup migrations.

**Q9 — engine scope.** Markdown-engine-only first, or executed documents
(fig-format pdf, image conversion, mediabag, pdf-images.lua remote fetch)
in the epic?

**Q10 — books/projects.** Single-document epic with book-PDF as a
follow-up? Book PDF needs single-file merge + Quarto-side crossref
pre-resolution — whose crossref (loops back to Q0). Decide scope now even
if deferred.

**Q11 — conformance oracle & CI.** Golden `.tex` corpus diffed against Q1
(snapshot goldens, or run Q1 in CI?), compile smoke tests (TinyTeX in CI —
weight?), pandoc-version ceiling tests like the existing pandoc-oracle. And
the adapter-completeness sweep: which corpus establishes the full
pampa-vs-qmd-reader divergence list (equations were found by a 2-fixture
spike; need breadth)?

**Q12 — error reporting & UX.** latexmk log parsing → Q-* error catalog
entries? Auto-install policy for missing packages (tlmgr touches the user's
TeX tree — auto, prompt, or flag-gated)? Progress/streaming UX for multi-run
compiles?

**Q13 — user Lua filters.** Plain `filters:` entries (not extensions):
supported in the pandoc leg day one via the entryPoint arrays? Does q2's
existing UserFiltersStage get bypassed for latex output (else filters run
twice, once per engine)?

**Q14 — CLI surface & artifacts.** `keep-tex`, output-dir/tex-safe
filename recipes, a `q2 latexmk` equivalent for bare `.tex` compilation
(Q1 ships quarto-latexmk; keep or drop)?
