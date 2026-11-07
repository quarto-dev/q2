---
title: 'RFU-7: the Positron fork, extension update, demo guide, e2e and cache location'
date: 2026-11-08
description: 'The work that lives in the Positron fork (a different repository): update the demo extension for q2 convert and the execute: record-sources key (set with -M), run the extended raw-record experiment, add the R e2e test, write the demo run guide, prototype the cache-location feature, add the missing e2e cases, and apply Gordon''s command-2 decision. Commits go on a fork branch and never to the q2 integration branch; the q2 side lands only a note.'
status: ready
depends-on: [rfu-1, rfu-2, opr-5]
lane: C
---

**Epic:** [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first; "Landing" differs here, see below).
**Branch:** a fork branch `q2/rfu-7-demo-followups` in `/Users/gordon/src/positron-riu6-a`, plus the q2 branch
`rfu/7-positron-fork` for the landing note · **Lane C** (`workspace-5`) · **Blocks:** nothing.
**Needs:** the Positron dev build and its Playwright setup (see the fork\'s `CLAUDE.md`, `AGENTS.md` and `.claude/rules`;
**read them first**; they govern the fork), R 4.3, Julia 1.12, Python, `target/debug/q2` from the integration branch.

## Start condition

Gordon recorded both decisions below on 2026-11-09. Gordon flipped `status` to `ready` on 2026-11-09. RFU-1, RFU-2 and OPR-5 must be complete so the final flags and
export behavior exist. If any is not complete, stop and report.

## Decisions by Gordon (2026-11-09)

**Fork commits allowed on branch `q2/rfu-7-demo-followups`:** **yes.** Local commits only on that new branch (cut from
`spike/riu-6-phase-a`); nothing is pushed and no other fork branch is touched. Gordon pushes or merges it himself.

**Command 2, progress and cancel: document, do not fix.** The demo rule applies: this is a known issue in the run guide.
The notification stays indeterminate ("q2 is running FILENAME. The first run can take a few minutes."), there is no cancel
button, and the only stop is the timeout (`q2.exportTimeoutSeconds`, default 300 s, SIGTERM). The guide says so and names
the setting.

**Command 2, the dirty-document wording: keep it, document it.** The note ("q2 runs the saved file, so cells you have
edited since saving will not show outputs") and the matched/skipped report after the push stay as they are; the guide
describes both. This agrees with records-in-use decision 7. No fork code change for either item.

## Landing differs from a q2 plan (explicit)

The fork is a separate repository and is **not** part of `feature/one-presenter`. Its work lands as **commits on a fork
branch**, never pushed. The q2 repository receives only a **landing note** listing the fork branch, each commit hash
and subject, and the results.

```bash
cd /Users/gordon/src/positron-riu6-a && pwd && git status --short
git switch -c q2/rfu-7-demo-followups spike/riu-6-phase-a    # tip 99aa72fcb4 when this plan was written
# ... fork work and fork commits (follow the fork's own commit conventions) ...
cd /Users/gordon/src/q2/.worktrees/workspace-5 && pwd
git switch -c rfu/7-positron-fork feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# at the end: write the landing note, set status: complete, then the usual
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only rfu/7-positron-fork
```

The fork has one untracked file, `test/e2e/tests/quarto/q2-raw-record-spike.test.ts` (the Phase A spike); use it as a
starting point and commit a cleaned version if it becomes a real test. Do not touch other fork branches (several are
checked out in other worktrees).

## Checklist (tests first where code is involved; every box is `[unattended]` unless marked)

- [ ] **Update the extension for `q2 convert` and the `execute: record-sources` key.** There is no option for the list after OPR-5 (decision R5, Gordon, 2026-11-09), so `buildPreviewArgs` and any export call pass `-M 'execute:{record-sources: ...}'` (dotted keys are not expanded, so use the braces); a directory entry must be inside the project (v1), so the extension cannot point the preview at Positron\'s cache directory and uses the `positron` keyword, or links the cache into the project. `extensions/q2-records-demo/src/q2Contract.ts`
      `buildExportArgs` (about lines 34-38: `convert`, `--to positron-ipynb`, `-o`) and `buildPreviewArgs` (about line 54:
      `--records`/`--execution-records`), the comment at line 7, `exportCommand.ts`, `previewCommand.ts` and their vitests. Keep the
      "q2 too old" detection (an older q2 has no `convert`, so it exits 2 on a missing `--to`) and add a vitest for it.
      The fork\'s e2e (`warmJuliaServer`, about line 17 of `quarto-render-and-push.test.ts`) also calls `record
      export-positron` and needs the same change. Verify against the real `target/debug/q2`.
- [ ] **The extended raw-record experiment** (carried over from the Phase A work): extend the raw-record experiment to the
      two shapes it did not cover and that the export still skips for a stated reason: a **fallback cell** (a
      `text/x-quarto-markdown` output, from a knitr chunk the structured route cannot map, or a LaTeX target) and a
      **marimo** document. Playwright. Write the result as a note in the q2 repository,
      `claude-notes/research/2026-11-xx-rfu-7-raw-record-experiment.md`; if it shows no harm, recommend lifting the two
      skips (a follow-up for Gordon, not done here).
- [ ] **R e2e test.** An R version of `quarto-render-and-push.test.ts`: command 2 on an R document, outputs appear;
      command 3 shows them from the cache with no R process started by the preview. Needs RFU-2 (knitr export). An R
      session exists in the dev build (`POSITRON_R_VER_SEL=4.3.2`, the `r` fixture). Serialize edits with the Julia
      e2e changes in this plan (one file).
- [ ] **Julia e2e.** If RFU-5\'s note is on the integration branch, apply its proposed fork changes; otherwise skip this
      box and say so.
- [ ] **The demo run guide.** `extensions/q2-records-demo/demo/README.md` does not exist (the RIU-6 plan and the commands
      findings cite it, but at fork `99aa72fcb4` the `demo/` folder holds the three `.qmd` documents only). Write it: the
      script from the commands findings note ([RIU-6 findings](../research/2026-11-07-riu-6-commands-findings.md)) plus the
      known limits: the Viewer\'s big X leaves q2 running (the extension kills q2 on `onDidRequestStop` and
      `onDidDispose`, `previewCommand.ts:~93-94`, and on deactivation, `:~113`; closing the Viewer evidently does not
      dispose the panel; decided: document it, use the Viewer\'s Stop); command 2 replaces a matched cell\'s outputs with
      no prompt (decided); command 2 does not change Positron\'s kernel state, so a later cell that uses a variable from
      a pushed cell fails; command 1 values are editor decorations, gone on reload, and command 3 cannot show them; what
      q2 says about a stale Julia server or a missing Plots project (RFU-4 writes that down). Apply the command-2
      decision above to the wording. Write it with the `/reader-expectations-prose` skill.
- [ ] **Positron\'s cache location: prototype a Positron feature, idiomatically** (Gordon wants a Positron-side prototype
      here). The extension takes the sibling of `ctx.globalStorageUri` (`previewHelpers.ts:~31-35`), which is
      profile-scoped, while the cache service always writes under `defaultProfile.globalStorageHome`
      (`src/vs/workbench/contrib/positronQuarto/browser/quartoOutputCacheService.ts:~133-136`). First investigate how
      Positron exposes workbench-owned paths to extensions (a public `positron.*` API, a contributed setting, or an
      internal command such as `_positronQuarto.pushOutputsFromFile`, `quartoPushOutputs.ts:~29`) and pick the most
      idiomatic; the service already holds `_cacheDir`. Then tests first (a vitest for the extension, and a check with a
      non-default profile), replace `cacheDirFromGlobalStorage`, and make a missing answer an error that names the
      cause. Done when commands 2 and 3 work under a non-default profile.
- [ ] **E2E cases not run in any window:** untitled document, "q2 too old" (exit 2), non-default profile, a dirty
      document; and **command 1 on Julia and R spans** (command 1, `positronQuarto.runInlineExpressions`,
      `quartoRunInlineExpressions.ts`, never calls q2; its span detector takes the document\'s primary kernel language,
      `quartoInlineExpr.ts:~35`, and nothing in `quartoInlineExprExecutor.ts` is Python-specific; only Python was run).
- [ ] **Command 2 progress, cancel and dirty-document wording:** decided "document, do not fix" for both (above). No code;
      the demo run guide box covers it. Check the guide states the no-cancel and 300 s timeout facts and the dirty-document
      note and report.
- [ ] `[Gordon]` Someone watches the R and Julia e2e once. Mark each step seen, Playwright, or not run.
- [ ] Write the landing note in the q2 repository: the fork branch, each commit hash and subject, what ran under
      Playwright, what was seen by a person, what was not run. Render-check it with `target/debug/q2 render --strict`.

## Exit criteria

- The fork branch has the commits with passing vitests and e2e runs reported; the extension works against the landed
  `q2 convert` and the `record-sources` key; the guide exists; the landing note is on the integration branch.
- Nothing was pushed; no other fork branch was touched.

## Companion (Gordon)

Push or merge the fork branch when he chooses.

## Notes from the run

(Fill in at landing.)
