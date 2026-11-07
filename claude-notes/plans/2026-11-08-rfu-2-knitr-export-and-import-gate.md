---
title: 'RFU-2: knitr cells export, and the import gate becomes a declared fact'
date: 2026-11-08
description: 'Removes the engine-ownership skips from the Positron export so a knitr record exports like Python and Julia, and replaces the import-side owner rule with one declared fact, kernel equivalence, that each engine states for the languages it owns. Gordon''s Phase A verdict is recorded (export drops nothing).'
status: ready
depends-on: [rfu-1, opr-3]
lane: B
---

**Epic:** [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `rfu/2-knitr-export-and-import-gate` · **Lane B** (`workspace-4`) · **Blocks:** RFU-7, RFU-8, RFU-6.
**Needs:** R 4.3 with knitr (the knitr fixtures skip without it), Python and Julia 1.12 for the fixture runs.

## Start condition

The Phase A verdict is recorded below (2026-11-09). Gordon flipped `status` to `ready` on 2026-11-09;
startability still needs RFU-1 and OPR-3 complete. If either is not complete, stop and report.

## The Phase A verdict (Gordon, 2026-11-09)

**Decision: RFU-2 proceeds as proposed, and the export drops nothing the document says to hide.** Gordon chose
"Proceed, export as-is". The export shrinks to a valid cache file and knitr cells export. A `results: hide` output, `asis`
markdown and raw HTML appear in Positron as the record holds them (as text), and a Julia plot bundle is handled only by
the existing temporary image rule. Reason: the epic\'s principle, "q2 does nothing special for Positron"; where q2 writes
a compatible record Positron shows it, and Positron-side feature design is out of scope.

Consequences for this plan: the export gets **no** `results: hide` filter and no new skip or filter rule for `asis` or raw
HTML; add a test that a `results: hide` output and an `asis` output stay in the exported cache file. RFU-8 documents only
what the export does, and mentions what Positron shows for these outputs only as Positron\'s behavior.
Evidence: [Phase A note](../research/2026-11-07-r-through-knitr-phase-a.md).

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c rfu/2-knitr-export-and-import-gate feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only rfu/2-knitr-export-and-import-gate
```

## Goal

**Export.** Today the export skips a unit not owned by `jupyter` or `julia` (`EXPORT_OWNERS`,
`crates/quarto-core/src/record/positron_export.rs:~101`, `SkipReason::EngineNotExportable`), which stops a knitr document
cold. Phase A showed Positron accepts a raw knitr record unfiltered. The export shrinks to what a valid cache file
needs: remove `EngineNotExportable` and, as far as the verdict allows, the inline-unit, extension-language, fallback
and duplicate-group skips and their `SkipReason` variants. Keep the validity wrapper and the temporary image rule
(Gordon: kept, commented as temporary, until the fork\'s view zone picks one representation). Do **not** add `knitr` to
`EXPORT_OWNERS`.

**Fallback and marimo cells stay skipped** unless the extended raw-record experiment note exists on the integration
branch (it is RFU-7\'s, run in the fork\'s harness): without that note the export keeps skipping a `text/x-quarto-markdown`
fallback cell and a marimo (`python.marimo`) cell, for the reasons it states today. Lifting those two skips is a
follow-up recorded in RFU-7, not part of this plan.

**Import gate (design agreed 2026-11-07).** Replace the `outputs_for` owner rule (`jupyter`, `julia`, or `knitr` with
token `r`; `crates/quarto-core/src/record/positron_import.rs:~147`) with one declared fact, **kernel equivalence**: for
which languages is a unit this engine owns ordinary kernel code, so the same text gives the same outputs in a plain
Positron kernel? A trait method on `ExecutionEngine` following `try_claims_language` (`engine/traits.rs:~176-184`):
an `Option`; `None` means "would have to load", and **`None` is not admitted** (undeclared engines are excluded).
Jupyter and Julia declare any plain language; knitr declares `r`. The data checks stay on the cell: no fallback marker,
syntactic language. Engines outside today\'s list are admitted only by declaration, each with a test. For TS engines
(Julia) the declaration must travel from the engine: read how `TsEngine` implements `try_claims_language` and use the
smallest **additive** channel; do not touch Julia\'s hash declaration (OPR-3 owns it). The Phase A table gives the
import-side cost: for R, the hash agrees and the shape differs, which RIU-3 already accepted (display only).

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Export tests first**, each failing on the current skip: a knitr record exports text output, a figure (one
      `image/png`), a message, a data frame, an error and a chunk with options.
- [ ] **Verdict test:** a knitr chunk with `results: hide`, one with `results: asis` and a raw-HTML chunk each keep their
      output in the exported file (nothing is dropped for Positron).
- [ ] A test that the export no longer consults the engine or language: a cell owned by any engine, in any language,
      appears when the record has outputs for it; fallback and marimo-style cells still skip, with their reasons.
- [ ] **Import tests first**, one per engine in the table: a unit takes Positron outputs only when its engine declares
      kernel equivalence for its language; an undeclared engine gets nothing.
- [ ] A CLI test through the binary (RFU-1 landed): `q2 convert <knitr fixture> --to positron-ipynb -o out.ipynb` exits 0
      and reports cells greater than 0. Keep the Python and Julia tests green.
- [ ] Track the removal of the image-representation rule: a box in the notes (when the fork\'s view zone picks one
      representation, delete the rule and its tests), not a strand.
- [ ] Implement the export change and the import gate; delete `EXPORT_OWNERS`, `JUPYTER_ENGINE`, `JULIA_ENGINE`,
      `KNITR_ENGINE` and the `lang == "r"` literal as gate inputs; update the help text; add a dated note to the RIU-5 plan
      (landed; do not rewrite its history).
- [ ] Run the real binary on the Python, Julia and knitr fixtures; read the JSON and the ipynb; record the invocations
      and output in "Notes from the run".
- [ ] Gates: `cargo clippy -p quarto-core -p quarto --all-targets -- -D warnings`; `cargo nextest run -p quarto-core -p quarto`;
      the Windows cross-clippy for each; `cd hub-client && npm run build:wasm` if `record/` changed (it compiles for
      `wasm32`); `cargo xtask lint`.

## Exit criteria

- A knitr record exports, with the same report shape as Python and Julia; the import gate reads a declared fact and
  no engine name; every table row has a test.
- Fallback and marimo cells still skip with their reasons (unless the experiment note existed and Gordon said otherwise).

## Notes from the run

(Fill in at landing.)
