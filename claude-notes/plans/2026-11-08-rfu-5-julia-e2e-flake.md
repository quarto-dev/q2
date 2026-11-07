---
title: 'RFU-5: the Julia e2e tests are slow and flaky, find the EPIPE closer'
date: 2026-11-08
description: 'A research plan. The Julia push tests in the Positron fork passed once, then timed out at 120 s with no outputs. Measures cold and warm runs, re-runs the e2e file three times, and runs the cheapest experiments to find what kills the q2 client while the Julia server writes progress. Writes a note and proposes fork changes; commits nothing to the fork.'
status: ready
depends-on: []
lane: B
---

**Epic:** [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `rfu/5-julia-e2e-flake` · **Lane B** (`workspace-4`) · **Blocks:** nothing (RFU-7 uses the note).
**Needs:** Julia 1.12, the Positron fork at `/Users/gordon/src/positron-riu6-a` (read it and run its tests; **do not
commit to it**), Playwright, `target/debug/q2`.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c rfu/5-julia-e2e-flake feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only rfu/5-julia-e2e-flake
```

## Goal

**Intended:** the demo\'s tests pass on a clean run. **Seen:** the Julia push tests passed once, then timed out at 120 s
with 0 outputs on a second full run; cold start measured 20 s, warm 10 to 12 s. The Julia engine is Quarto 1\'s
`julia-engine` extension: a detached server reused by the next render, started with `serve(; timeout = 300)`
(`resources/extension-subtrees/julia-engine/_extensions/julia-engine/quartonotebookrunner.jl:~48`), so a gap of over five
minutes means a cold start. `q2 call` has no `julia` subcommand, so `status` and `stop` are not reachable from q2.

Done already (read): timing cold and warm; with a warm server both Julia tests pass; the fork change `53bf9ca105` adds
`warmJuliaServer` (at `test/e2e/tests/quarto/quarto-render-and-push.test.ts:17`) and `setTimeout(480000)` on the Julia
tests. The cause of the 120 s runs (a cold or stale server) is likely, not proven.

## Checklist (every box is `[unattended]`)

- [ ] Re-run the whole e2e file three times and report pass counts; keep each run\'s Julia server log (copy it before the
      next run overwrites it).
- [ ] **Find the EPIPE closer, cheapest experiment first.** The client socket is held by a **Node child of q2**, not by
      q2, so an EPIPE on the server\'s progress write means that child (or q2 above it) was killed. Candidates: the
      extension\'s `execFile` timeout (`extensions/q2-records-demo/src/exportCommand.ts` in the fork), Playwright\'s
      test timeout or worker teardown, q2\'s own kill path (`crates/quarto-core/src/engine/ts_process.rs`).
      Experiments: (1) in a scratch directory run the export on the Julia fixture, SIGKILL q2 and its Node child a few
      seconds in, read the server log (an EPIPE there means a hung-up client is sufficient); (2) match the EPIPE
      timestamp in an e2e server log to per-test end times (inside a test: a timeout kill; at the end: teardown);
      (3) look for a second q2 run (the warm helper, or an orphan) in flight when a passing test tore down.
- [ ] Write the note `claude-notes/research/2026-11-xx-rfu-julia-e2e-flake.md`: the runs and counts, each experiment with
      its result (ran, read or not done), the most likely cause, and **proposed** fork changes for RFU-7 (a diff in
      the note, never committed to the fork). Render-check it.
- [ ] If the cause is on the q2 side (`ts_process.rs`), say so and propose the fix; do not change q2 code in this plan
      without Gordon\'s say-so (a one-line fix with a test may be made if it is clearly in `ts_process.rs` and covered
      by the gates below; say so in the notes).
- [ ] Gates (only if you changed q2 code): `cargo clippy -p quarto-core --all-targets -- -D warnings`;
      `cargo nextest run -p quarto-core`; the Windows cross-clippy (`ts_process.rs` has platform-specific
      process code; see `.claude/rules/cross-platform.md`).

## Exit criteria

- Three full runs reported with pass counts and logs; the experiments run; the note names the most likely cause and
  proposes the fix; nothing is committed to the fork.

## Notes from the run

(Fill in at landing.)
