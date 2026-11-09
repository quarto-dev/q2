---
title: 'OPR-7d: switch plain render of Julia and knitr to the recorded route'
date: 2026-11-08
description: 'Turns the internal present_from_record control on by default, so a plain q2 render of a Julia or knitr document is presented by the Rust presenter from an in-memory record, writing nothing to .quarto. Keeps the LaTeX family and uncovered targets on the engine route, quietly. Applies Gordon''s decisions on the retained knitr fallback and on an opt-out control.'
status: blocked-on-gordon
depends-on: [opr-7a, opr-7b, opr-7c, opr-7f]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/7d-switch-plain-render` · **Lane D** (`workspace-6`) · **Blocks:** OPR-8.
**Needs:** R 4.3 with knitr and Julia 1.12; `hub-client` tooling for the WASM gate and the preview chain.

## Start condition

`status` is `blocked-on-gordon` until Gordon has signed the allowlist (OPR-7c) and flipped this plan to `ready`; both
decisions below are recorded (2026-11-09). If the allowlist is unsigned or the status is not `ready`, stop and report.

## Decisions by Gordon (2026-11-09)

**1. The retained knitr fallback (knitr epic decision 24) for plain render: keep, as today.** It stays for chunks Rust
cannot map and for fallback targets; OPR-7d changes nothing about it, and removing it remains the separate,
trigger-gated KNR-8. Reason: Gordon chose the recommended option; the corpus does not cover every use of knitr, so the
fallback is a safety net the switch should not remove.

**2. An opt-out control: none.** The `present_from_record` flag is flipped on and stays internal; there is no environment
variable, document key or other user-facing way back. Reason: Gordon chose the recommended option; the parity gate (zero
unallowlisted differences, OPR-7c) is the safety, and a control would be a user surface to document and later retire. A bad
case is fixed in q2, not worked around. OPR-7a\'s "behind a control" means this internal flag only, which agrees.

**3. Julia subfigure groups on the flip (added 2026-11-09 from OPR-0c; needs Gordon).** Today plain Julia resolves a
`fig-subcap` group\'s references with wrong numbers; the recorded route leaves them unresolved (`bd-gbaykhth`, same
failure as a stored notebook). Flipping Julia changes wrong to unresolved. Choose: (accept it and record the regression
against `bd-gbaykhth` / flip knitr and Julia together / flip knitr first and hold Julia until the group resolves).

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-6 && pwd
git switch -c opr/7d-switch-plain-render feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7d-switch-plain-render
```

## Goal (decision D5)

A plain `q2 render` of a Julia or knitr document builds the record in memory and presents it through the Rust
presenter, as a recorded render does. Concretely:

- The internal `present_from_record` flag from OPR-7a is on by default. There is no opt-out (decision 2).
- **Nothing is written under `.quarto`** on a plain render except what is written today (`render-manifest.json`);
  `execute: record: true` still writes the record file (execution-records decision 3).
- The LaTeX family (`latex`, `beamer`, `pdf`), `pptx`, `context` and unknown targets stay on the engine route for
  knitr, **quietly**: `plan_capture` (`engine/knitr/target_route.rs`) says its info diagnostic once per document
  whenever a record is wanted; on a plain render that would fire on every such render, so it must not (Gordon:
  degrade quietly, as other fallbacks do). An explicit record request keeps the diagnostic.
- Cached knitr chunks fall back with no record (OPR-7b). If [OPR-7e](2026-11-09-opr-7e-cache-hits-as-records.md) is
  `complete`, a `cache=TRUE` chunk is a record cell on a hit as well as on a miss, and levels 1 and 2 still fall back;
  read its notes. If 7e is blocked or not done, proceed as in 7b: this plan does not wait for it (7e decision 5).
- Marimo is a documented exception and stays on its legacy path (decision D12).

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Tests first, through the binary** (model `crates/quarto/tests/integration/no_execute_cli.rs`): a plain render of a
      Julia and a knitr document is presented from a record (show it: the output carries the recorded-route markup,
      for example the `.cell` wrapper without `id="cell-N"` unless OPR-7c decided to keep it; nothing in the tree
      reads those attributes, OPR-0c grep); a Julia cell with a bare PNG gets the file names OPR-7c decided on; a marimo
      document\'s HTML is identical to before (it has no presenter route; `marimo_engine_e2e::` still passes); nothing new under
      `.quarto`; a knitr document to `pdf` or `latex` takes the engine route and prints nothing about it; a cached
      chunk falls back; `execute: record: true` still writes the file; there is no opt-out to test (decision 2).
- [ ] Flip the default (no opt-out, decision 2); change the fallback diagnostic\'s condition so it does
      not fire for a plain render.
- [ ] Re-run the OPR-0b/7c gate harness and attach the final report (zero unallowlisted differences).
- [ ] **End-to-end through the real binary,** plain `q2 render` on a Jupyter, a Julia and a knitr document and
      one LaTeX-target knitr document; read the HTML; record the invocations and what you saw.
- [ ] **Preview chain** (AGENTS.md, "Verifying Rust changes in `q2 preview`"): `cd hub-client && npm run build:wasm`,
      `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`; confirm `q2 preview` of the same documents still
      renders (an automated check; say what was and was not seen by a person).
- [ ] Gates: `cargo clippy -p quarto -p quarto-core --all-targets -- -D warnings`; `cargo nextest run` for the same
      crates; the Windows cross-clippy; `cargo xtask lint`.

## Exit criteria

- A plain render of Julia and knitr is presented from an in-memory record; the gate report shows zero unallowlisted
  differences; nothing new is written under `.quarto`.
- LaTeX-family and uncovered targets stay on the engine route with no message; marimo unchanged.
- Gordon\'s two decisions are implemented as recorded (fallback kept; no opt-out control).

## Notes from the run

(Fill in at landing.)
