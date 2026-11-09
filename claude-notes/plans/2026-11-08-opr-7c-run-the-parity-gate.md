---
title: 'OPR-7c: run the parity gate for the plain-render switch'
date: 2026-11-08
description: 'Runs the OPR-0b parity corpus and harness on the final presenter, classifies every difference between the engine-owned route and the recorded route for knitr and Julia, fixes the small presenter blockers, and proposes the allowlist. The rule (knitr epic decision 26) is zero unallowlisted differences; Gordon signs the allowlist, which unblocks OPR-7d.'
status: ready
depends-on: [opr-0b, opr-1b, opr-2, opr-7b, opr-4d]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/7c-parity-gate` · **Lane D** (`workspace-6`) · **Blocks:** OPR-7d (through Gordon\'s sign-off).
**Needs:** R 4.3 with knitr and Julia 1.12 with the packages the corpus documents need.

Dependencies count as resolved when `complete` **or** `cancelled`; OPR-4d may have been cancelled with the Julia work
(protocol, "Status values").

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-6 && pwd
git switch -c opr/7c-parity-gate feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7c-parity-gate
```

## Goal (decision D10)

The gate Gordon chose: knitr epic decision 26. A difference passes only if it is **Identical** or **SyntaxLevel**
(bytes differ, parsed AST equal); a **Semantic** or **Blocker** difference needs a signed-off allowlist entry
(`opr-parity-allowlist.toml`, format of the knitr `allowlist.toml`). The switch (OPR-7d) requires **zero unallowlisted
differences** on both corpora.

## Checklist (every box is `[unattended]`)

- [ ] Run the corpora with the harness from OPR-0b (the exact command is in that plan\'s notes; it sets
      `OPR_PARITY_REPORT=1`) on a machine with R and Julia. Commit the report under `claude-notes/research/`.
- [ ] Classify every difference. For each Semantic or Blocker, decide: **fix** or **allowlist**.
      Fix those that are small and live in `presenter/`, `record_splice.rs` or `engine/knitr/` (one session or less
      each), with a test first; do not widen the plan beyond that. Anything larger: list it in the report as a
      blocker for Gordon and do **not** allowlist it yourself.
- [ ] Known candidates, now measured by OPR-0c ([note](../research/2026-11-09-opr-measurements.md) sections 2 and 5):
  - **`id="cell-N"`, `data-quarto`, `data-execution_count`** (plain Julia only). The consumer check is done: a grep of
    `crates/`, `ts-packages/`, `hub-client/src` and `resources/` finds no reader of any of the three (read). Re-run
    the grep at the time of the gate, then propose them for the allowlist with that as the reason.
  - **Generic `cell-output` class** on a figure\'s output div: plain Julia has `cell-output cell-output-display`, the
    presenter only `cell-output-display` (deliberate, matches the knitr figure hook). Only reader found:
    `resources/js/dashboard/quarto-dashboard.js:49`, which uses `.cell-output` for iTables HTML tables, not figures,
    and non-figure display outputs keep the class (read). Re-check dashboards with a Julia figure when you run the gate.
  - **Figure file names, Julia, a cell that emits a bare PNG** (Plots emits inline HTML, which is not affected). Plain
    numbers the file by the notebook cell index with prose counted (first code cell after one prose block:
    `cell-2-output-1.png`, the Quarto 1 convention, which also names a labeled cell by its label,
    `fig-b-output-1.png`); the presenter counts code cells only (`cell-1-output-1.png`) and never uses the label. **The
    `preferred` mechanism does not help here:** it honors `metadata.quarto_file_path`, which knitr supplies and which I
    found no Julia producer for in `crates/` or `ts-packages/`. So either make the presenter\'s counter match Quarto 1
    (count prose blocks, use the label), which is a small test-first fix if the record carries the position, or list it
    as a blocker for Gordon with the URL change spelled out. Changing figure paths silently breaks user links.
  - **Subfigure groups** (`fig-subcap` with `layout-ncol`): Julia recorded and a stored notebook leave the group and both
    subfigure references unresolved; Julia plain and both knitr routes resolve them with wrong numbers. These are
    `bd-gbaykhth` and `bd-au5gts6e`, and moving Julia from wrong to unresolved is a Semantic difference. Do not
    allowlist it yourself: list it as a blocker for Gordon (see OPR-7d decision 3).
- [ ] The corpus (OPR-0b) must contain the group case on Julia and on a stored notebook, a bare-PNG Julia cell with a
      labeled and an unlabeled cell, the mixed cached knitr document, and a minimal marimo document (expected identical
      between plain and recorded, since marimo has no presenter route). If a case is missing, add it here before
      classifying; they are the 0c baseline.
- [ ] Mark each remaining entry in `opr-parity-allowlist.toml` as `proposed` with a one-line reason.
- [ ] Re-run the whole gate after fixes; the final report lists: counts per class, every allowlist entry with its
      reason, every blocker for Gordon, the documents skipped and why.
- [ ] Gates for any fixes: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`;
      the Windows cross-clippy; `cd hub-client && npm run build:wasm` if `presenter/` or `record/` changed.

## Exit criteria

- Every difference is Identical, SyntaxLevel, fixed, or on the **proposed** allowlist; no unclassified difference.
- The report and allowlist are committed; blockers for Gordon are listed.
- **`[Gordon]` (after landing):** sign the allowlist (change `proposed` to `signed` entry by entry), and flip
  OPR-7d to `ready`. (The two decisions OPR-7d needs were recorded on 2026-11-09: the fallback is kept; there is no
  opt-out.)

## Notes from the run

(Fill in at landing.)
