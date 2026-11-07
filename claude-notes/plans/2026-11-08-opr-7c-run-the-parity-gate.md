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
- [ ] Known candidates from the review (Julia, one tiny document, 2026-11-08, relied): the cell wrapper\'s
      `id="cell-N"` and `data-quarto` attributes, and figure names (`cell-N-output-M.png`). For figure names
      prefer keeping the engine\'s own path when it lies inside the document directory (the presenter has the
      mechanism, `preferred` in `presenter/figures.rs:~151-180`, knitr epic decision 25); changing figure paths
      silently breaks user links. For `id` and `data-quarto`, check what consumes them (cross-references, lightbox,
      preview scroll sync; grep `crates/` and `ts-packages/`) and say whether anything does.
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
