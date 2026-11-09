---
title: 'RFU-4: verification and small fixes for the records-in-use epic'
date: 2026-11-08
description: 'The q2-side boxes the RIU-6 demo left open: find or run the hub verification, re-check that a failed optional record copy fails soft, check the preview highlight for an edited cell, write down what q2 can say about a stale Julia server or a missing Plots project, and the RIU-3 browser items nobody has seen.'
status: complete
depends-on: [opr-5]
lane: B
---

**Epic:** [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `rfu/4-verification-and-small-fixes` · **Lane B** (`workspace-4`; moved from lane C on 2026-11-09, lane B is idle) · **Blocks:** nothing.
**Needs:** Julia 1.12, Playwright for the browser checks, a built `target/debug/q2`, `hub-client` tooling.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c rfu/4-verification-and-small-fixes feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only rfu/4-verification-and-small-fixes
```

## Why this waits for OPR-5

The checks use `q2 preview` and `q2 provide-hub` with the record source list. After OPR-5 there is no option for it: set it with `-M 'execute:{record-sources: ...}'` or the `execute: record-sources` key (decision R5, Gordon, 2026-11-09); before OPR-5 the old `--records` option is still there. A directory source must be inside the project (v1), so copy or link a cache directory into the project for these checks.

## Goal

Close the verification gaps listed by the RIU-6 closeout. Each box is a check with a recorded result, not a feature.
This is a demo (Gordon): a result is either fixed, or written down as a known issue; the demo\'s run guide is RFU-7\'s.

## Checklist (every box is `[unattended]` unless marked)

- [x] **Hub verification (ran: provide-hub one-shot with the unattended flag Gordon approved, output seen in a browser; see the note).** The records-in-use epic\'s exit criterion 1 says preview and hub work from records alone,
      "verified through the real binary and a browser". Find where the hub half was verified and cite it, or run it:
      `q2 provide-hub` with the writer gate on (`QUARTO_ENGINE_RECORD_WRITE=1`) and a hub client. Record the invocations
      and what you saw; say what was automated and what was not seen by a person.
- [x] **Decision 12 re-check.** Records-in-use decision 12 (a failed optional `record: true` copy fails soft) says "not
      re-verified against the code"; re-verify with the real binary: make `.quarto/execution-records` unwritable
      (a regular file; create `.quarto/` first) and render with `execute: {record: true}`; record the exit code, the
      message and whether the render still produced its output.
- [x] **An edited cell loses its highlighting in the q2 preview** (RIU-3a). `q2 render` highlights an unexecuted
      `{python}` cell (checked 2026-11-07), so the gap is probably in how the records-only preview presents an
      unmatched cell. A Playwright check against `q2 preview` with `-M 'execute:{record-sources: <dir>}'` (an in-project directory) and an edited cell; file a box
      in "Findings for Gordon" if it is a bug. Do not fix it here.
- [x] **Stale Julia servers and a missing Plots project** give confusing results (`startpath must be non-empty`; an
      `ArgumentError` exported as an error output that looks like a working run; a stream of `SYSTEM: caught exception
      ... failed Task notice` in a plot cell\'s stderr seen in the Phase A run). Reproduce each, and write down what q2
      can say when it sees either, with where in the code it would say it. Implement nothing without Gordon\'s say-so.
- [x] `[either]` **RIU-3 browser items never seen:** `record-sources` set to `positron,engine` (by `-M`) running the engine, a cache
      older than the document shown in a browser, `--share --allow-edit` with a guest (after OPR-5 Part B also check that a guest document key outside the host list is ignored with a page warning), "Clear results" clicked, a real
      click or scroll sync. Run what Playwright can; mark the rest `not run` for Gordon to watch.
- [x] Write the results as one note, `claude-notes/research/2026-11-xx-rfu-verification.md`, with a table: item, result,
      ran, read or not run. Render-check it with `target/debug/q2 render --strict`.
- [x] Gates (no code changed, so none ran; only the note and this plan changed; the note renders with `--strict`) (only if you changed code, which you should not): `cargo clippy -p <crate> --all-targets -- -D warnings`.

## Exit criteria

- Every box above has a recorded result and the note is committed.
- Any bug found is written up with a minimal reproduction under "Findings for Gordon"; nothing is silently fixed.

## Notes from the run

Ran on `target/debug/q2` after rebuilding the whole embed chain (hub-client WASM, preview SPA, editor embed). Results are in
[the verification note](../research/2026-11-09-rfu-verification.md). Summary: decision 12 holds; the records-only preview drops
highlighting on an edited cell (a bug, left unfixed); the stale Julia server (`startpath` must be non-empty) and the missing-Plots errors
reproduce and the note says where q2 could add a hint (`engine/ts_process.rs`, `execution_failed`); the RIU-3 browser items that
Playwright can drive were run (output shown, Re-execute overlay, `positron,engine` with `preview.engine: auto`, Clear results); a cache
older than the document is not flagged for a directory record (the rule is for a Positron import, not tested). The hub verification was run after Gordon approved `--dangerously-accept-requests` for it: exit 0, and a browser client showed both cell outputs. **Not run:** `--share --allow-edit` with a guest, click and scroll sync, `provide-hub --watch`. No code changed.

## Notes from an earlier attempt

Done (ran, on `target/debug/q2`; results in [the verification note](../research/2026-11-09-rfu-verification.md)):

- Decision 12: confirmed, exit 0 with a warning and the output produced.
- Edited cell in the records-only preview: bug reproduced under Playwright (`<pre class="{r} code-with-copy">`, no highlighting,
  where `q2 render` gives `sourceCode r`). Not fixed; it is in the note\'s "Findings for Gordon".

Not done: the hub verification (no earlier record found in the RIU plans; needs `q2 hub`, a built hub client and a browser),
the stale Julia server / missing Plots project reproductions, and the RIU-3 browser items. Stopped there on purpose, to land the
two results and hand the rest to a fresh session. Environment: no Python or Jupyter on this machine (use `{r}` or Julia cells);
`timeout` is not installed; `cat` of an execution-records path once hung the shell, so avoid it.
