---
title: 'RFU-3: the EXR-16 sweep and the labeled-cell fig-cap splice test'
date: 2026-11-08
description: 'Two test plans for the Positron import path that RIU-3 left open. A table-driven sweep over the 21 conformance cases (build a Positron-style cache, import, convert and splice, and assert which units receive outputs), and a labeled Python cell with fig-cap carried through import and splice. Runs after the one-presenter loop merge so it pins the final splice behavior.'
status: ready
depends-on: [opr-1b]
lane: A
---

**Epic:** [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `rfu/3-positron-splice-tests` · **Lane A** (`workspace-3`) · **Blocks:** nothing.
**Needs:** nothing beyond the repository (R-dependent cases skip without R).

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3 && pwd
git switch -c rfu/3-positron-splice-tests feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only rfu/3-positron-splice-tests
```

## Why this waits for OPR-1b

These tests present spliced output. OPR-1a and OPR-1b change the splice\'s forwarding and span handling on purpose, so
a test written before them would pin behavior that is about to change. Write them against the landed behavior.

## Goal

- **EXR-16 sweep.** One table-driven test in `crates/quarto-core/tests/integration/positron_source.rs` (or a sibling in
  the same binary) that, for each of the 21 `CASES` in `positron_conformance.rs`, builds a Positron-style cache from the
  case\'s fixture, imports, converts and splices, and asserts which units receive outputs; cases where Positron and q2
  diverge (`positron-expected.json`) assert the divergence. (read, 2026-11-07) Today `positron_conformance.rs` tests
  only the generator and the loader, and `positron_source.rs` covers three durable fixtures (python, hand-built
  julia, r).
- **Labeled-cell `fig-cap` splice case.** A Python cell with `#| label: fig-x` and `#| fig-cap: ...` through import,
  convert and splice, asserting the output shows on the page. `knitr_positron_import.rs:~179` and `:~261` cover the
  hash and the record shape only.

## Checklist (tests first; every box is `[unattended]`)

- [ ] Read `positron_conformance.rs`, `positron_source.rs`, `knitr_positron_import.rs` and the fixtures under
      `crates/quarto-core/tests/fixtures/positron-conformance/` and `positron-cache/`.
- [ ] Write the sweep as a table; one row per case with the expected set of units that get outputs; divergence rows cite
      `positron-expected.json`. Write it so it **fails** if a case silently stops receiving outputs (mutate a case to
      see it fail, then restore).
- [ ] Write the labeled-cell case; assert the figure and its caption appear. Note that a multi-figure labeled cell with
      `fig-subcap` is a known gap on the Jupyter-shaped route (`bd-gbaykhth`); do not add that case here and do not file
      a new strand.
- [ ] Register new files in `crates/quarto-core/tests/integration/main.rs`, alphabetically.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core` (build any expected path or URI with `Path::join`).
- [ ] If a case reveals a product bug, **do not fix it here**: record it in "Findings for Gordon" with a minimal
      fixture; a bug outside every plan may become a strand (Gordon decides).

## Exit criteria

- The sweep covers all 21 cases and the labeled-cell test exists; both pass; the sweep fails when a case regresses.
- Findings (if any) are written up.

## Notes from the run

(Fill in at landing, including "Findings for Gordon".)
