---
title: 'One presenter path: review findings (2026-11-08)'
date: 2026-11-08
description: 'What the design-and-implementation review of the one-presenter epic found: what Gordon decided and when, what the three read-only agents and the reviewer found, which claims were verified by running or reading and which were relied on, what contradicted what, what remains unknown, and the decisions that are still Gordon''s.'
---

**Epic:** [one presenter path](../plans/2026-11-07-one-presenter-path-epic.md).
**Earlier notes:** [spikes](2026-11-07-one-presenter-spikes.md), [step-1 decisions](2026-11-08-one-presenter-review-decisions.md).
Marks: **ran** (I ran it and read the output), **read** (I read the code), **relied** (a sub-agent\'s claim I did not
check). Line numbers are for `94906556f`. The three agents were Sonnet agents, read-only, with no sub-agents of their
own: one designed `q2 render --execution-records` (before the discussion), and two reviewed the epic from a design
and intent lens (A) and a tree and implementation lens (B) after the first rewrite.

## What Gordon decided

The decisions note records step 1 in his words. Later in the session he also decided (see epic D1 to D13): the
amended enforcement rule; the document-scope rule keyed on resolved options; cached knitr chunks falling back with no
record, quietly; knitr\'s gate for plain render; that a plain render stays quiet for LaTeX-family targets; that
pptx, context and other targets are strands; marimo a documented exception; `preview.engine` preview-only; `cache`
refused on render; forwarding on both surfaces and the span rule from `classify_inline_result`; one document function
that returns outcomes; the offer-and-acknowledge handshake replacing the engine-name switch; docs at the end inside
existing pages; one pull request; lanes in workspaces 3 to 6 with `workspace-1` as the integration checkout.

## Verified by me

- **The `output: false` trace (read and ran).** `execute.R:348-353` forwards a document `output: false` to
  `results: hide`, `fig.show: hide`, `warning: FALSE`, `message: FALSE` before R runs; the record keeps the outputs and the
  presenter honors `results: hide`. The spike\'s S3 "contradiction" was an artifact of its test.
- **Hash declarations (read).** Julia\'s declaration is empty of display-only options (`record-wire.ts:19-20`); Jupyter\'s
  display-only list is `echo`, `output`, `warning`, `include` (`engine/jupyter/mod.rs:~251`); a knitr test asserts
  `include`, `warning`, `message`, `output` are never presentation-only.
- **Knitr fallback targets (read).** latex, beamer, pdf, pptx, context and unknown (`target_route.rs` tests); the fallback
  diagnostic is info-level, once per document, only when a record is wanted.
- **`--replay` (read).** `Option<String>`; the `QUARTO_REPLAY` fallback is read in `commands/render.rs`, not by clap, so
  a clap conflict cannot cover it.
- **The loops differ (read).** Forwarding only in `present_document` (`document.rs:~168`); errors, format-mismatch
  notes, engine entry per cell, matching and span handling as listed in OPR-0a.
- **Span routines (read).** `classify_inline_result` (`presenter/inline.rs:250`) takes the last text-bearing output and lets
  an error win; `present_span` (`record_splice.rs:~505`) takes the first output with data and ignores errors; it has no
  direct test.
- **Staleness (read).** `record_is_stale` takes no target format; its test says "format alone is not an edit"; preview
  shows a stale record.
- **Figures (ran).** A labeled figure with `fig-cap` resolves identically from a stored Jupyter notebook and from knitr.
  A two-figure cell with `fig-subcap` and `layout-ncol: 2`: Quarto 1 (99.9.9) gives "Figure 1", "Figure 1 (a)",
  "Figure 1 (b)"; q2\'s Jupyter-shaped route leaves all refs unresolved; q2\'s knitr route gives "Figure 3", "Figure 1",
  "Figure 2" on the plain route and **byte-identically** on the recorded route (presenter), after stripping image data.
  Strands `bd-gbaykhth` (existing) and `bd-au5gts6e` (filed 2026-11-08).
- **`message = isTRUE(format$execute[["warning"]])` in `execute.R` is the same line as Quarto 1\'s (line 320).** Nothing to fix.
- **Where `preview.engine` comes from (read).** The q2 preview epic, Phase C.6 (2026-05-11); reused by the records-in-use
  epic as "never execute".
- **Existing strands (ran `braid`).** `bd-cm1mwyuq` covers Jupyter forwarding (its render half was fixed on 2026-11-04,
  `2649c02ea`); `bd-nn2fou8h` is in progress and overlaps OPR-1a and OPR-2.
- **Branch state (ran).** `feature/riu-follow-up-review` is 175 commits ahead of and 158 behind the `main` tip
  (`8f17e950d`, where workspaces 1, 3, 4 and 6 are parked) and is on no remote.

## Relied on, not checked by me

Agent B\'s timing runs (knitr about 0.1 s more with a record; Julia unresolved) and its Julia HTML diff
(`id="cell-1"`, `data-quarto`, figure names); the full-record `to_value` copy per render on `want_record`; agent A\'s
per-option table for knitr `include` and `output` (the record keeps the outputs, the presenter hides them); the
Julia capture enforcing only a cell\'s own `warning`; marimo declaring `[]`; the Julia route trace (the notebook is
deep-copied on every plain render and dropped); the render agent\'s claims that the engine stage can still start after
a splice and that `with_record` exists at library level (B confirmed the second by reading); the hub provider
findings (guest `_quarto.yml` and front matter take effect; consent only guards `run_and_store`); that no user docs page
describes per-engine cell options; the knitr corpus counts (280 accepted, 8 allowlisted, 4 not compared) and the Julia
parity suite being structural; the runner internals for OPR-4a (`include_str`, `_record!`).

## What contradicted what

- **S3 against the real render.** Resolved: R translates `output: false`; the spike\'s table of "scope ignored" was an
  artificial state. Layering is not needed for knitr (D3).
- **"Convert has a record option."** The epic and my notes said `preview`, `provide-hub` and `convert`; agent B found the
  third consumer is `q2 record export-positron` and `convert` is a stub with no option yet (RFU-1 adds it).
- **"A native HTML render cannot carry a record."** True of `render_to_file`, false at library level (`HtmlRenderConfig` has
  `record` and `with_record`); OPR-6 copies it in.
- **My first parallel plan list.** Agent B found OPR-1, 2, 3 and 4 edit the same files; the plans are now serial where
  they share files (lane A), and OPR-6 waits on OPR-1b.
- **Decision 2 as first worded.** Both agents found it breaks for Julia (declares everything hashed), for knitr
  `include` and `output`, and for engines with no declaration; it was amended (D2).
- **The `--replay` clap conflict.** Corrected to a runtime check.
- **"Parity evidence exists."** For knitr yes (a corpus); for Julia only a structural suite on stored notebooks, no live
  run; OPR-0b builds the live corpus.

## What remains unknown

- Whether runner-side expression starts, anchors and output kinds are feasible (OPR-4a); message against warning is the
  hardest.
- The cost of an in-memory record (OPR-0c) and whether OPR-7a needs a larger seam.
- Whether a mixed knitr document with cached fallback chunks comes out right (OPR-0c).
- What consumes `id="cell-N"` and `data-quarto` (OPR-7c); whether any other route changes the figure matrix.
- Whether the splice\'s and render\'s text substitution agree around blank lines (OPR-0a).
- Whether `execute: record: true` itself enters the document hash (OPR-0d).
- Hub trust and the hash effect of a metadata key (OPR-0d); whether a mixed-engine document works through the splice.
- Windows runtime behavior; the WASM and hub-client paths beyond the build gates in the plans.

## Open decisions after the review (Gordon\'s)

1. **R5, metadata key or flag.** After OPR-0d. Decides OPR-5\'s shape.
2. **The retained knitr fallback and an opt-out control** for the plain-render switch. After OPR-7c. Written into OPR-7d.
3. **Julia scope after OPR-4a:** which of expression starts, anchors and output kinds to build, or cancel.
4. **Signing the parity allowlist** (OPR-7c) and, outside the epic, **the Phase A verdict** and **command 2** (progress,
   cancel, dirty-document wording) in the RIU follow-up epic (RFU-2 and RFU-7).
5. **Companions:** pushing the engine and runner forks, the Positron fork\'s extension update, and the rebase of the
   integration branch onto `main` (158 commits behind) before the pull request.
