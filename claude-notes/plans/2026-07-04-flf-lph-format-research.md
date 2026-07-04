# FLF/LPH format-porting research & spikes (LaTeX, EPUB)

**Worktree:** `.claude/worktrees/flf-lph-research` (branch `worktree-flf-lph-research`, forked from main)
**Date:** 2026-07-04
**Status:** in progress

## Overview

Research/proof-of-concept exercise on how Quarto 1's non-HTML format support
(LaTeX and EPUB as the study cases) maps onto Quarto 2. Three named hypotheses:

- **FLF (Formats are Lua Filters):** the bulk of Q1's format-specific code is
  Lua filters, not TypeScript. Task: quantify this on the Q1 source
  (`external-sources/quarto-cli`).
- **PFR (Port-or-keep choice):** for format-specific Lua, we can (1) port to
  Rust or (2) keep as Lua. Gordon's prior: option 2 is fine for slow formats
  (LaTeX, EPUB, docx, pptx); Typst may deserve a Rust pampa writer. This
  exercise measures how bad option 2 is.
- **LPH (Lua Pandoc Hypothesis):** these formats will use Pandoc for output,
  so the Q1 Lua filters may simply keep running *inside Pandoc* — no porting
  at all. Total LPH would obviate PFR. Quantify how much of the Lua fits LPH
  and what doesn't.

Deliverables: quantification of FLF, inventory of LaTeX/EPUB format-specific
logic (TS + Lua + accessory), spikes running those formats in q2 by copying Q1
Lua filters in, an assessment of whether q2's coarser Lua hook surface hurts,
and a list of design questions for discussion. Not a final design.

## Work Items

### Phase 1: Exploration (parallel)
- [x] Quantify FLF: Lua vs TS format-specific code in quarto-cli, per format
- [x] Inventory Q1 LaTeX/PDF format logic (TS, Lua, templates, post-processing)
- [x] Inventory Q1 EPUB format logic (TS, Lua, templates, post-processing)
- [x] Map q2's current state: Pandoc-output path, Lua filter system + hooks,
      existing non-HTML format support, custom AST node handling

Phase 1 findings (summaries; full agent reports go into the Phase 4 doc):
- FLF: format-specific Lua ≈10.5K LOC vs format-specific TS ≈16.6K product-wide
  (strong FLF refuted), BUT for LaTeX+EPUB all content transformation is Lua
  (LaTeX ~2.5-3K, EPUB ~70); the TS is latexmk/compile-loop (~2.6K) + a 51-line
  EPUB shim. EPUB rides the HTML Lua path (isHtmlOutput()==true for epub*).
- Q1 invocation contract: ONE pandoc run with --from qmd-reader.lua,
  filters:[main.lua, citeproc] via --defaults, --data-dir=Q1 datadir,
  --metadata-file, QUARTO_FILTER_PARAMS (base64 JSON) env,
  template+~22 partials; then TS .tex line-postprocessors (~30) and the
  latexmk PDF loop.
- q2: HTML/RevealJS only; never shells to pandoc; pampa has a Pandoc-JSON
  writer + subprocess seam (json_filter.rs); embedded pandoc-compatible Lua
  engine with 2 filter positions (Pre/Post around AstTransformsStage) + Q1
  `at:` name shim; no param()/quarto.config, no _quarto.ast Lua API; custom
  nodes are native Rust (Callout, FloatRefTarget, Theorem, Proof, PanelTabset).

### Phase 2: Analysis
- [x] Assess LPH: which Q1 Lua filters could run inside Pandoc under q2, which
      can't (and why: quarto param injection, custom nodes, TS coupling, ...)
      → LPH ~95% true; remainder is TS orchestration (report §4)
- [x] Assess q2 Lua hook surface vs Q1's — moot for pandoc-written formats:
      the format Lua runs inside the pandoc subprocess with Q1's own chain
      intact; user filters injectable via quarto-filters entry-point arrays

### Phase 3: Spikes
- [x] Spike: LaTeX via captured-contract replay (no Q1 TS) → byte-identical
      .tex; compiles with TinyTeX; system pandoc 3.8.1 also byte-identical
- [x] Spike: EPUB replay → identical modulo pandoc's random UUID/timestamps
- [x] Spike: pampa -t json → pandoc -f json + [q2-compat.lua, main.lua,
      citeproc] → byte-identical to Q1 for both fixtures; EPUB
      content-identical (one table-id nuance)
- [x] Record filter fixes needed → exactly ONE additive 23-line adapter
      (equation-label convention); zero edits to Q1 filters; isolated, not
      systemic. Adversarial fixture quantified the TS postprocessor gap
      (margin cites, sidenotes, code annotations) as the honest non-LPH part.

### Phase 4: Report
- [x] Write findings report → claude-notes/research/2026-07-04-flf-lph-latex-epub.md
- [x] Reconcile this checklist (this edit)
- [ ] Discuss design questions with Gordon (report §6 + §10)

### Round 2 (Gordon's follow-up, same day)
- [x] FLF percentages scoped to LaTeX: Lua 32% / TS 44% / other 24% raw;
      restricted to document-transformation code, Lua = 76% (→ ~95% after
      §10 migration). Report §8.
- [x] Repeat analysis for docx and pptx (inventory agents + spikes):
      docx ~650-700 Lua / ~65 TS / 5 PNGs; pptx ~52 live Lua / ~21 TS / 0
      resources; both replays byte-identical to Q1 except OOXML timestamps.
      Report §9.
- [x] Audit postprocessors vs Carlos's everything-on-the-AST goal:
      docx/pptx/epub already have zero postprocessors; LaTeX's 20 line
      processors → 7 AST-now, 4 post-citeproc-AST, 6+1 with effort, 2
      genuinely post-writer (longtable internals). Report §10.

### Round 3 (same day): architecture clarification + epic design questions
- [x] Clarify pampa-vs-pandoc AST confusion: one-way seam, no round trip;
      "port postprocessors to AST" = TS→Lua under Pandoc (pre/mid-chain),
      not TS→Rust. Report §12.
- [x] Design questions gating a "LaTeX 100%" epic, top 5 ranked (crossref
      ownership, citeproc choice, upstream-first vs fork, pandoc pinning,
      conformance oracle) + scoping questions. Report §13. Gordon has read
      them and deliberately left them open for the epic's design phase.

## Notes

- Q1 source: `external-sources/quarto-cli` (symlinked into this worktree from
  the main checkout; not version-controlled).
- q2 binary building in background at start of session.
