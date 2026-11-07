---
title: 'RFU-4: verification and small fixes for the records-in-use epic'
date: 2026-11-08
description: 'The q2-side boxes the RIU-6 demo left open: find or run the hub verification, re-check that a failed optional record copy fails soft, check the preview highlight for an edited cell, write down what q2 can say about a stale Julia server or a missing Plots project, and the RIU-3 browser items nobody has seen.'
status: ready
depends-on: [opr-5]
lane: C
---

**Epic:** [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `rfu/4-verification-and-small-fixes` · **Lane C** (`workspace-5`) · **Blocks:** nothing.
**Needs:** Julia 1.12, Playwright for the browser checks, a built `target/debug/q2`, `hub-client` tooling.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5 && pwd
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

- [ ] **Hub verification.** The records-in-use epic\'s exit criterion 1 says preview and hub work from records alone,
      "verified through the real binary and a browser". Find where the hub half was verified and cite it, or run it:
      `q2 provide-hub` with the writer gate on (`QUARTO_ENGINE_RECORD_WRITE=1`) and a hub client. Record the invocations
      and what you saw; say what was automated and what was not seen by a person.
- [ ] **Decision 12 re-check.** Records-in-use decision 12 (a failed optional `record: true` copy fails soft) says "not
      re-verified against the code"; re-verify with the real binary: make `.quarto/execution-records` unwritable
      (a regular file; create `.quarto/` first) and render with `execute: {record: true}`; record the exit code, the
      message and whether the render still produced its output.
- [ ] **An edited cell loses its highlighting in the q2 preview** (RIU-3a). `q2 render` highlights an unexecuted
      `{python}` cell (checked 2026-11-07), so the gap is probably in how the records-only preview presents an
      unmatched cell. A Playwright check against `q2 preview` with `-M 'execute:{record-sources: <dir>}'` (an in-project directory) and an edited cell; file a box
      in "Findings for Gordon" if it is a bug. Do not fix it here.
- [ ] **Stale Julia servers and a missing Plots project** give confusing results (`startpath must be non-empty`; an
      `ArgumentError` exported as an error output that looks like a working run; a stream of `SYSTEM: caught exception
      ... failed Task notice` in a plot cell\'s stderr seen in the Phase A run). Reproduce each, and write down what q2
      can say when it sees either, with where in the code it would say it. Implement nothing without Gordon\'s say-so.
- [ ] `[either]` **RIU-3 browser items never seen:** `record-sources` set to `positron,engine` (by `-M`) running the engine, a cache
      older than the document shown in a browser, `--share --allow-edit` with a guest (after OPR-5 Part B also check that a guest document key outside the host list is ignored with a page warning), "Clear results" clicked, a real
      click or scroll sync. Run what Playwright can; mark the rest `not run` for Gordon to watch.
- [ ] Write the results as one note, `claude-notes/research/2026-11-xx-rfu-verification.md`, with a table: item, result,
      ran, read or not run. Render-check it with `target/debug/q2 render --strict`.
- [ ] Gates (only if you changed code, which you should not): `cargo clippy -p <crate> --all-targets -- -D warnings`.

## Exit criteria

- Every box above has a recorded result and the note is committed.
- Any bug found is written up with a minimal reproduction under "Findings for Gordon"; nothing is silently fixed.

## Notes from the run

(Fill in at landing.)
