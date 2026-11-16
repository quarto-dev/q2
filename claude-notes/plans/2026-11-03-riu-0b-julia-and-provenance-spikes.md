---
title: 'RIU-0b: Julia and node-level provenance spikes'
date: 2026-11-03
description: 'Two scratch spikes that settled what RIU-3, RIU-5 and RIU-6 could not be written against: how a Julia cell appears in a q2 record and in Positron, and whether spliced record output can be stamped with a provenance kind through the SourceInfo system. Both ran; this plan records the questions, the answers and where each landed.'
status: done  # run 2026-11-03; findings in research/2026-11-03-riu-0b-findings.md; the Positron window steps were reported by Gordon
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Findings:** [RIU-0b findings](../research/2026-11-03-riu-0b-findings.md). Section names below (J1 to
J8, P1 to P8, U21 to U29) point there.
**Depends on:** [RIU-0](2026-11-02-riu-00-spikes.md) (the S4 conversion, the S5 push method, the S7
export).
**Blocks:** nothing: it is done, and RIU-3 Phase E, RIU-5 and RIU-6 are written from its results.
**Estimated sessions:** 1 (spent).

Scratch code is on `spike/riu-s8-julia` (`2fbd63449`) and `spike/riu-s9-provenance` (`4c8fbb8e0`),
cut from `feature/records-in-use`, never merged. The Positron side used the S5 push command
unchanged (`spike/q2-s8-julia` in `/Users/gordon/src/positron-spike-s3` holds no new commit).

## Spike J: Julia

**Decision behind it:** epic decision 12 puts Julia in scope for the demo. RIU-0 S1 B10 showed only
that Julia renders the same with the gate on and off.

**What it showed**

- A Julia cell is owned by the `julia` engine (q2's Julia engine, a QuartoNotebookRunner server),
  never by Jupyter, whatever the document asks for. `jupyter: julia-1.11` and `engine: jupyter`
  both resolve to `julia` (J1, J2; the claim and fallback rules are exercised on mock engines by the tests at `resolution.rs:1506-1553`).
- RIU-5's S7 keep rule (owned by `jupyter`) skips every Julia cell (J4). The import gate in
  `positron_import.rs:147` accepts only the owner `"jupyter"`, so a Julia unit, whose owner from the
  registry is `"julia"`, gets nothing from a Positron cache (J6).
- Julia outputs are strings, several MIME types per bundle (a plot gives PDF, PNG, SVG and HTML,
  and no `text/plain`); no array-valued or JSON-valued data appeared (J5).
- Pushed into an open Julia document in the Positron dev build, text, vector and HTML outputs
  matched by content hash and persisted (Gordon, J7). A multi-MIME plot showed two plots and base64
  text, because Positron renders every item of a bundle (Gordon, J8; the cause is read from
  `quartoOutputViewZone.ts`).

**Where it landed:** RIU-5 (keep rule admits `julia`; representation rule), RIU-3 (owner function and
gate), RIU-6 Phase C (Julia leg and its exit criteria).

## Spike P: node-level provenance for spliced output

**Decision behind it:** epic decision 14 (provenance goes through the existing `SourceInfo` system).

**What it showed**

- After the reparse, spliced nodes are identifiable: reconcile keeps the document's own nodes (with
  the document file id) and takes new content from the reparse (a new file slot), so file id plus the
  unit's text range picks them out. A post-walk after the merge works; nothing has to ride through the
  reparse (P1).
- The kind: `quarto-source-map` is an external crate and already defines an atomic
  `By::jupyter_output()` kind for execution output, which nothing in this tree uses. A new
  first-party kind would need a release of that crate; a kind built in `quarto-core` is non-atomic
  (P2). The TypeScript `ATOMIC_KINDS` set lacks `jupyter-output`.
- The wire format, the prose-edit round trip, the tiling auditor's CI-gated kinds and the structural
  hash all hold (P3 to P6). The incremental writer never receives the spliced AST (P4, P5).
- A stamped `CodeBlock` loses its copy-button decoration, because the decoration key needs an
  `Original`; so `CodeBlock` keeps its `Original`. A value merges with adjacent prose into one
  `Str`, so a `Str` is stamped on overlap rather than containment (P7 and "Two failures").
- Record-level `quarto_provenance` does not map onto engine provenance: a Positron record has no
  engine entry (Answer 5).

**Where it landed:** RIU-3 Phase E; the epic ("What is known", decision 14, decisions for review).

## Checklist

- [x] Spike J run; findings written; RIU-5 skip rule and RIU-6 Phase C written from the result.
- [x] Spike P run; findings written; RIU-3 Phase E written from the result.
- [x] Spike branches recorded in the findings note (never merged).
- [x] Findings note rendered with `q2 render --strict`.
- [ ] Not run: Positron producing a Julia cache itself, and a Julia cell with option lines in
      Positron (named in the findings, "Not tested"). RIU-6 Phase C covers them.
