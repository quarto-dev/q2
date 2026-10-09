---
title: 'Plan: carry Elliot''s span comments onto the integration line (document import P0)'
date: 2026-10-03
---

**Date:** 2026-10-03
**Epic:** [`2026-10-03-document-import-epic.md`](2026-10-03-document-import-epic.md) (decision I14; interfaces and conventions there apply)
**Depends on:** nothing. **Unblocks:** the manual bubble checks in P5's Verification and the epic Close-out (no automated test outside P0 depends on it).
**Branch:** `import/p0-span-comments` from `feature/hub-import`.

## Why

Import writes a Word comment as a plain span with the comment inside (I4). On `feature/pandoc-wasm` such a comment renders as inline text. Elliot's unmerged `origin/feature/span-comments` (three commits of 2026-09-29, listed below) makes it render as a margin bubble anchored to the span (verified, epic Findings). We carry an early copy so the import work can be seen end to end. **This copy is temporary:** the epic's Close-out removes it before the final PR, and Elliot's branch lands through its own PR.

What his branch changes (`git diff --stat main...origin/feature/span-comments`, 16 files, all under `ts-packages/preview-renderer/src/q2-preview/`):
- `PreviewRoot.tsx`, `commentAnchor.tsx`, new `commentPending.ts`, `registry.ts`, `sourceIndex.ts`;
- `custom/CommentBlock.tsx` (the largest change), new `custom/CommentSpan.tsx`;
- `inlines/Span.tsx`;
- `richtext/`: `EditToolbar.tsx`, `RichTextEditor.tsx`, `astToProseMirror.ts`, `editorConfig.ts`, `schema.ts`, `serializer.ts`, new `spanMarkExtension.ts`, `styles.ts`.

His commits, oldest first:
- `42622bf23` "initial prototype"
- `67d87096e` "Significantly improve how comment spans are added and highlighted"
- `d0004a327` "Modify commented span style"

The branch has no other commits beyond `main`. `CommentSpan.tsx`\'s header comment documents the scope: plain authored spans only; spans with an editorial-mark class or any `quarto-*` class are left alone; comments nested in Emph/Strong/Link are out of scope.

**Commit labels.** Every code commit after Elliot's three has a subject starting `P0:`; the epic's Close-out drops these. Commits that touch only this plan file (Handoff log, checklist) start `Plan P0:` and are kept. T1's Handoff entry therefore goes in a `Plan P0:` commit after the cherry-picks.

## Checklist

### Tasks

- [x] **T1 Rebase.** Environment first (epic conventions): root `npm install`. Record `git rev-parse feature/hub-import` (the P0 base) in the Handoff log, and the baseline counts of T2's suites at that base (preview-renderer unit and integration, hub-client unit and integration), run before the cherry-picks. Create the branch, then `git cherry-pick -x 42622bf23 67d87096e d0004a327` (keeps Elliot's authorship; `-x` adds a trailer naming his original SHA, which survives rewording and helps tell this copy from his PR if it lands as a squash). `git merge-tree --write-tree feature/hub-import origin/feature/span-comments` merged cleanly on 2026-10-03, so no conflicts are expected; if there are some, resolve against `feature/pandoc-wasm`\'s versions, grep the whole tree for conflict markers after every conflict stop before committing, and record each resolution in the Handoff log. Do not edit his code beyond what conflicts require.
- [x] **T2 Green.** In `ts-packages/preview-renderer`: `npm run typecheck`, `npm run typecheck:tests`, `npx vitest run`, `npx vitest run --config vitest.integration.config.ts`. Then hub-client's unit and integration suites (they consume preview-renderer). The existing `CommentBlock.*.integration.test.tsx` suites have not been run against his rewritten `CommentBlock.tsx`, so breakage may come from his change, not only from the rebase. Fix either kind minimally, in separate commits labelled `P0:`, recording each in the Handoff log; these fixes are dropped with the rest at removal. **STOP** and ask Gordon if the fixes take more than a day or touch files other than `CommentBlock.tsx`, `CommentSpan.tsx` and their tests: the carry is temporary (I14), and larger repairs belong in Elliot's PR.
- [x] **T3 Probe test.** Promote `claude-notes/research/2026-10-02-document-import-spike/nested-comment-probe.integration.test.tsx.txt` into `ts-packages/preview-renderer/src/q2-preview/custom/CommentSpan.import.integration.test.tsx` with real assertions instead of the probe's logging. Assert the import-relevant shapes: a comment inside a plain span gives one bubble with the comment text, the span's text excludes the comment, and the span carries `.q2-commented-span`; a comment with `author`/`date` attributes behaves the same; a wrapper ending in two comment spans (a comment and its reply, epic I4): no expected outcome exists (his branch predates I4's reply shape), so this case is a characterization test, labelled so in its name and comment; record what renders and assert it; a comment directly in a paragraph still gives a bubble. Assert the highlight case renders inline, to document the exclusion. Commit labelled `P0:`.
- [x] **T4 Record what removal needs.** In the Handoff log: the subjects of Elliot's three commits as they landed with their `-x` trailers, the labelling rule above, and the full list of files P0 touches, including T3's test. Record Elliot's three **full** original SHAs (the ones the trailers name) and the P0 base. The SHAs of the cherry-picked copies are recorded too, but rebases will change them; the epic's removal matches the trailers\' full original SHAs and the `P0:` subjects within `<P0 base>..HEAD`, not author or subject alone. Later plans must not edit these files (epic Close-out).

### Verification

- [x] T2's suites pass, with counts recorded against the counts at `feature/hub-import` before P0.
- [x] `git log --format='%an %s' <base>..HEAD` shows Elliot's three commits first, then only `P0:`- and `Plan P0:`-labelled commits.
- [x] Workspace nextest at the plan boundary: "no Rust changes, counts unchanged" (epic conventions).

### Close-out

- [x] Checklist reconciled against what landed, committed.
- [x] Rebased onto `feature/hub-import` and fast-forwarded into it. Epic Progress is the orchestrator's to tick (execution conventions), not ticked here.
- [x] Gordon told that P0 has landed (so he can let Elliot know it is being carried).

## Handoff log

Append-only.

- 2026-10-03: plan written. Not started.
- 2026-10-03: revised after the implementability review (epic status line): removal matches the trailers\' full original SHAs.
- 2026-10-03: revised after the angle review: environment and suite baseline in T1; a one-day STOP on T2's fixes; the reply case in T3 is characterization-only.
- 2026-10-03 (T1): P0 base = `feature/hub-import` `ddb8eec22a317a15a27e969fa2257bf0f43dd384`. Environment: root `npm install` (its `package-lock.json` churn, stripped `peer` flags, was reverted, not committed) and `node scripts/fetch-pandoc-wasm.mjs --require`. Baseline at the base, before the cherry-picks (all typechecks green): preview-renderer unit 46 files passed + 2 skipped / 612 tests passed + 36 skipped; preview-renderer integration 59 files / 674 passed + 1 skipped; hub-client unit 127 files / 1506 passed; hub-client integration 23 files / 169 passed. Branch `import/p0-span-comments` cut; the three cherry-picks applied with no conflicts (tree grepped for markers: none).
- 2026-10-03 (T2): after the cherry-picks, all vitest suites passed with unchanged counts and `typecheck` passed in both packages, but preview-renderer `typecheck:tests` failed (baseline green): Elliot's `sourceIndex.ts` makes `SourceIndexEntry.sourcePool` required, and 3 fixtures in `richtext/EditTypeIndicator.test.tsx` lacked it. That file is outside the plan's allowed list, so this was a STOP; Gordon approved the minimal fix (`sourcePool: []` on the 3 fixtures), committed as `P0: add sourcePool to EditTypeIndicator test fixtures`. `CommentBlock.*.integration.test.tsx` needed no changes against his rewritten `CommentBlock.tsx`. After T2 and T3: preview-renderer unit unchanged; integration 60 files / 679 passed + 1 skipped (the +5 are T3's); hub-client unchanged; all typechecks green.
- 2026-10-03 (T3): `custom/CommentSpan.import.integration.test.tsx` (5 tests). Observed behavior it records: a comment in a plain span gives one bubble, the span is `.q2-commented-span`, and the span keeps the `Space` that preceded the comment, so its text is `'range '` and the paragraph reads `Before range  after` (double space). A comment+reply wrapper (characterization only) renders ONE collapsed bubble `first+1 more`, the span commented, nothing inline. A comment in a `quarto-highlight` span stays inline with no bubble, as CommentSpan.tsx's scope says.
- 2026-10-03 (T4, what removal needs):
  - Elliot's original SHAs (named by the `-x` trailers): `42622bf23ad6183f79e270bbb229da2626af7738` "initial prototype"; `67d87096eb86976e4cb83d3b978e631149eb5133` "Significantly improve how comment spans are added and highlighted"; `d0004a327ced4936b4816dc8ab70180c21545d92` "Modify commented span style". P0 base: `ddb8eec22a317a15a27e969fa2257bf0f43dd384`.
  - Cherry-picked copies at landing time (rebases will change them): `dc0628207` "initial prototype", `65e3daaec` "Significantly improve...", `8c4056ce9` "Modify commented span style". P0 commits: `34e82a695` "P0: add sourcePool to EditTypeIndicator test fixtures"; `940113d52` "P0: integration tests for the span-comment shapes Word import produces".
  - Labelling rule: Elliot's three keep his authorship and carry the `-x` trailer; every other code commit starts `P0:`; plan-only commits start `Plan P0:` and are kept. Removal drops the trailer-matched commits and the `P0:` subjects within `<P0 base>..HEAD`, not by author or subject alone.
  - Files P0 touches, all under `ts-packages/preview-renderer/src/q2-preview/`: `PreviewRoot.tsx`, `commentAnchor.tsx`, `commentPending.ts` (new), `registry.ts`, `sourceIndex.ts`, `custom/CommentBlock.tsx`, `custom/CommentSpan.tsx` (new), `custom/CommentSpan.import.integration.test.tsx` (new, T3), `inlines/Span.tsx`, `richtext/EditToolbar.tsx`, `richtext/EditTypeIndicator.test.tsx` (T2 fix), `richtext/RichTextEditor.tsx`, `richtext/astToProseMirror.ts`, `richtext/editorConfig.ts`, `richtext/schema.ts`, `richtext/serializer.ts`, `richtext/spanMarkExtension.ts` (new), `richtext/styles.ts`. Later plans must not edit these files (epic Close-out).
- 2026-10-03 (boundary): `cargo nextest run --workspace` at HEAD: 15732 run, 15732 passed (2 slow), 202 skipped, exit 0. P0 changes no Rust (`git diff --stat <base>..HEAD` lists only `ts-packages/preview-renderer/` and this plan), so the Rust counts are unchanged by construction; no separate Rust baseline was taken before the cherry-picks. Checklist reconciled against the commits and logs. `feature/hub-import` was still at the P0 base `ddb8eec22` at landing, so no rebase was needed; landed by fast-forward. Epic Progress left for the orchestrator; changelog entry none (I14).
- 2026-10-03 (landing correction): the previous entry's "no rebase was needed" was true when written but stale at landing: P1 stage 1 landed first (`feature/hub-import` moved from `ddb8eec22` to `c198585b9`). P0 was rebased onto `c198585b9` (clean, no conflicts) and its TS gates rerun with identical counts and green typechecks (the workspace nextest was not rerun: P0 is TS-only, disjoint from P1's files, and P1 ran its own at its boundary). Rebase changed the copy SHAs: Elliot's three are now `b53ac88f6`, `d563a154f`, `722cecb58` (trailers still name the original full SHAs above); `f60b36d92` "P0: add sourcePool..." and `225eaaf32` "P0: integration tests...". The P0 base for `<base>..HEAD` removal matching is now `c198585b9`.

