# In-progress braid strand audit (bd-a0eyjshu)

**Date:** 2026-07-27
**Strand:** bd-a0eyjshu — Audit and clean up stale in-progress braid strands

## Overview

At the start of this audit, **55 strands** were marked `in_progress`. The
working hypothesis (from Carlos) is that essentially none of them represent
live work — they were left `in_progress` when a session ended and never
transitioned. The goal is to drive that count down to only the strands with
actual repo evidence of live work.

For each strand the verdict is one of:

- **CLOSE** — repo evidence shows the work landed (merged PR, commit on
  `main`, or shipped release).
- **OPEN** — real work remains, but nobody is on it right now. Returns to
  `open` so it shows up in `braid ready`.
- **KEEP in_progress** — repo evidence of genuinely live work (an open PR
  being iterated on, or a checkout currently sitting on the branch).

### Evidence sources

- `git log --grep=<id>` on `main` and on all refs
- `gh pr view <n>` merge state for every PR named in a strand comment
- `braid dep tree` for epics (an epic with open children cannot close)
- Local branches, sibling room checkouts (`~/rooms/room-{1,2,3}/q2`) and
  worktrees, to find unmerged/unpushed work

### Method caveat

Evidence gathering ran from `~/rooms/room-2/q2` on `main` (78d55deb). Work
done on *other machines* is invisible here — the only two strands touched
in the last 24h (bd-53501yf7, bd-9fwn1504) were cross-checked individually.

## Results

| Verdict | Count |
| --- | --- |
| CLOSE | 29 |
| OPEN | 24 |
| KEEP in_progress | 2 |
| **Total** | **55** |

**Executed 2026-07-27.** `braid list --status in_progress` now returns 3
strands: the two live ones below, plus this audit strand itself.

## Checklist

### Phase 1 — CLOSE (work landed)

- [x] bd-81cfshmw — q2 mcp launcher. PR #277 MERGED 2026-06-12.
- [x] bd-6rczoll3 — xtask ts-packages build step. Merge `34f7090d` on `main`.
- [x] bd-5706gcrq — rich Markdown in titles. PR #291 MERGED 2026-06-16.
- [x] bd-y259zb57 — reveal theme parity. Bug 1 (`5b46b50e`) + Bug 2 Level 2 (`650cbddc`, `83122ad8`) all on `main`; branch `beads/bd-y259zb57-reveal-theme-parity` is 0 ahead of main.
- [x] bd-w0c6d38k — revealjs crossrefs. PR #316 MERGED 2026-06-19. (Part 3 = WASM preview lives on separate strand bd-zecehtnc.)
- [x] bd-vwp4y5ku — hub-client reveal theme. PR #320 MERGED 2026-06-22. (Follow-up bd-ktuojk26 is its own strand.)
- [x] bd-sfet3264 — remote execution provider. PR #357 MERGED 2026-07-06.
- [x] bd-bm0vaetl — provide-hub Text-valued file ids. Fix `ac1457d4` on `main`.
- [x] bd-uy4uygha — format:html capture display. `449f93bc..deee0edb` on `main`.
- [x] bd-9lgiulr4 — provide-hub consent gate. `572e698e` on `main`.
- [x] bd-jzqswvh0 — image drag-drop path. PR #395 MERGED 2026-07-15.
- [x] bd-llhlzd7p — localization/i18n. PR #398 MERGED 2026-07-17; no children.
- [x] bd-en2hvrwn — raw-json format. PR #400 MERGED; shipped in v0.10.0.
- [x] bd-z1smhvuo — embed-example iframes. PR #271 MERGED 2026-06-10 (Phases 1+2).
- [x] bd-kjrpya2d — preview VFS iframe resolution. PR #271 MERGED; Part 2 browser-verified.
- [x] bd-k2h1x7bu — em/en-dash canonicalization. PR #290 MERGED 2026-06-16.
- [x] bd-0nm0beab — braid-viewer release bundles. Shipped in braid v0.6.0 (2026-07-07), all 5 platforms attached.
- [x] bd-aeyss6p5 — list-item block attrs. PR #314 MERGED 2026-06-18.
- [x] bd-m4slev7a — hub MCP share-URL acceptance. PR #324 MERGED 2026-06-22.
- [x] bd-sjb4pzx8 — tiptap rich-text spike. PR #335 MERGED; `512907f7` (rich text default in q2 preview) on `main`. Spike verdict GREEN and shipped; later phases are their own strands.
- [x] bd-9x3zbuj8 — preview hierarchy nav + overlap. PR #345 MERGED 2026-06-25.
- [x] bd-pvcnea83 — edit-chrome top crop. PR #346 MERGED 2026-06-26.
- [x] bd-yai4w8ly — merged preview status line. `0b13dbcb` + `c8251701` on `main`.
- [x] bd-qyjsncfx — `q2 --version` string. PR #282 MERGED 2026-06-13.
- [x] bd-yjh1y117 — beads/ → braid/ branch prefix. PR #302 MERGED 2026-06-17.
- [x] bd-65u3hil5 — Windows HOME panic. PR #358 MERGED 2026-07-02 (test deleted).
- [x] bd-5t6wvu7m — jupyter image outputs. PR #412 MERGED 2026-07-24.
- [x] bd-eiku4ymo — capture audit metadata + hub admin tools. PR #415 MERGED 2026-07-27.

### Phase 2 — RETURN TO OPEN (work remains, nobody on it)

Design/plan written, implementation not started:

- [x] k-zvzm — config merging design for pampa. Dec 2025; no commits reference it. Four plan docs exist.
- [x] bd-3lsb — AST-level sync client API. Feb 2026; no commits reference it.
- [x] bd-19nc56ao — ipynb surface syntax. Design doc written 2026-07-20; no implementation.
- [x] bd-21q16 — contenteditable spike for q2-preview. No commits, no comments; never started. (Note: superseded in practice by the tiptap work under bd-sjb4pzx8 — worth a triage pass.)
- [x] bd-m1jeqhhz — `q2 call engine`. Plan 9 authored; explicitly "ready for execution pending Gordon's go".
- [x] bd-4qflzhwh — checkInstallation / `q2 check`. Plan 10 authored; "ready for execution".
- [x] bd-1d6io — annotated-qmd source-tracking off-by-one. Investigation complete with verdict "ready"; **fix not implemented**.

Partially landed / follow-ups remain:

- [x] bd-3nzyd — hub-client E2E smoke-all. Tier-2 gate landed (PR #249, CI green), but the strand's own notes say **"RESIDUAL (keep this bead open)"**: 14 tests still pass-on-retry. Real remaining work, no live session.
- [x] bd-10bdjmjb — browser sync offline-fallback race family. Both spun-off strands (bd-vm5e5u10, bd-10deu8h4) are closed and PR #277 shipped sync reliability fixes, but the parent's D1/D2 durability items were never explicitly signed off. Needs a triage read of `claude-notes/plans/2026-06-12-sync-client-offline-race.md` before closing.
- [x] bd-tvtknbhx — interactive task-list checkboxes. PR #407 MERGED, but the strand comment lists 3 items "remaining before close": hub-client live verification, loose/Para-leading item glyph, rich-text raw-glyph display.
- [x] bd-grkrb9nj — Lua API Pandoc parity (epic). PRs #393 + #404 merged; **1 open child** (bd-62lppjuy, `doc:normalize()`), so the epic cannot close yet.
- [x] bd-4doe9lvt — `_quarto-rules.scss` parity (epic). PR #408 merged; **6 open children** (bd-18410csp, bd-8oyd9dg4, bd-9fz5fweg, bd-l1rx9yzh, bd-q36vnfdp, bd-sehm2rha).
- [x] bd-v053sk3s — Phase 1P q2 preview revealjs (GA landing gate). PR #266 MERGED 2026-06-09 and the core is browser-verified — but `braid close` **refused**: 2 open children (bd-qn8yi1su golden parity test, bd-vv8jft5n config-option/section-id parity). Reclassified from CLOSE to OPEN; closes once those two land.
- [x] bd-c3dtpe36 — mermaid render component for q2-preview. Phases 0–2 verified live on quarto-hub.com 2026-07-17. The related pivot strand bd-5m4ga0s1 ("mermaid as a regular rendering feature") is already **closed**, so this experiment strand's remaining scope should be re-triaged rather than assumed live.

⚠️ **Unmerged work sitting on local-only branches** (at risk — these are not
pushed anywhere; returning them to `open` does not preserve the code):

- [x] bd-g4uw7d8g — q2 preview eager sync. 3 commits on local branch `beads/bd-g4uw7d8g-q2-preview-eager-sync`, not on `main`, no PR. Comment: "Awaiting Carlos's manual e2e before push."
- [x] bd-hcp8m3ve — float/layout class taxonomy. 4 commits on local branch `braid/bd-hcp8m3ve-float-taxonomy`, not on `main` (`quarto-float-caption` absent from `main`), no PR. P3/lst-fixture/preview-WASM pending.
- [x] bd-e3lv7eg3 — tree-shake unused deps. `7f36c285` on `remotes/origin/chore/bd-e3lv7eg3-tree-shake-unused-deps`, **no PR was ever opened**.
- [x] bd-7zxvdn0y — Monaco parse diagnostics. Comment describes 6 commits on `braid/bd-7zxvdn0y-monaco-parse-diagnostics`. **Followed up 2026-07-27 — verdict: unmerged, and held only on the strand author's machine.** Not a merged-then-deleted branch:
  - None of the described symbols exist on `main` — `refreshParseDiagnostics`, `useParseDiagnostics`, `getDiagnosticsForContent` appear *only* inside `.braid/snapshot.jsonl` (i.e. the strand's own comment text), nowhere in `hub-client/` or `crates/`.
  - No PR ever existed: `gh pr list --state all --search "bd-7zxvdn0y"` is empty, and no closed/merged PR title mentions parse diagnostics or squiggles. The nearby merged Monaco PRs (#296, #305, #338) are highlighting and backtick-autoclose, unrelated.
  - The branch is absent from origin (`gh api repos/quarto-dev/q2/branches`), from this checkout, and from `~/rooms/room-1` and `~/rooms/room-3`.
  - `git fsck --dangling` has no unreachable commits from 2026-06-23; the one `error squiggles` commit in history (`22d9fd75`) is Carlos's, from Dec 2025, and is already on `main`.
  - The plan the strand cites, `claude-notes/plans/2026-06-22-monaco-parse-error-diagnostics.md`, was never committed to any ref here either.

  **Owner:** the strand was created and both comments written by **shikokuchuo** — the same developer whose bd-53501yf7 work (implemented today) is likewise not present in any local checkout. The consistent explanation is that shikokuchuo works from their own machine and this branch was never pushed. **Action: ask shikokuchuo to push `braid/bd-7zxvdn0y-monaco-parse-diagnostics` (plus the plan doc) to origin.** Nothing to recover on our side.

Blocked on / owned elsewhere:

- [x] bd-5oyk1xce — preview drops engine include-in-header. Fixes (`a30015c0`, `2b3009ef`) live on `feature/ts-engine-extensions` (PR #416, still OPEN — epic integration branch). Lands when that epic lands.
- [x] bd-l9jhy5u0 — julia-engine worker leak. Fix implemented in the **external** repo `~/src/quarto-julia-engine` (branch `q2-close-busy-fix`, 3 clean commits); ready to push a fork PR to PumasAI/quarto-julia-engine. Nothing left to do in q2.
- [x] bd-cxara — "Phase 9 — end-to-end verification" for hub-mcp **device flow**. Parent bd-cmp48 (device-flow auth) is still `open`, but device flow was **replaced by loopback+PKCE on 2026-06-10** and shipped via bd-81cfshmw/PR #277. Very likely obsolete — flag for Carlos: close both, or rewrite bd-cxara against the loopback flow.
- [x] bd-kik3s1vt — posit-assistant transcripts experiment. Phase 2b done (unpushed commit `da279fdc`); has an OPEN bug (preview shows "Document automerge:&lt;id&gt; is unavailable") with a debugging handoff doc.

Old citeproc-era strands (Nov–Dec 2025, `k-` ids, no recent activity):

- [x] k-422 — quarto-citeproc: citation processing engine
- [x] k-444 — multi-pass rendering architecture for quarto-citeproc
- [x] k-449 — CSL disambiguation re-rendering loop

*(These three predate the beads→braid migration and have no commits
referencing their ids. `quarto-citeproc` exists and works, so they are
probably partially or fully done — but there is no mechanical evidence
either way. Returned to `open` rather than closed on a guess; they deserve
a dedicated triage session.)*

### Phase 3 — KEEP in_progress (genuinely live)

- [x] **bd-9fwn1504** — quarto-ast-reconcile proptest counterexample.
  PR #422 **OPEN** (commit `804a1b38`), and `~/rooms/room-3/q2` is
  *currently checked out* on `braid/bd-9fwn1504-quarto-ast-reconcile-proptest`.
  Design settled with Carlos today. Close on merge.
- [x] **bd-53501yf7** — hub-client connection indicator shows Offline first.
  Implemented **today at 18:42Z by shikokuchuo** ("Remaining: live local-prod
  browser E2E + changelog"). The code is not in any local checkout, so it is
  on another machine — active work by another developer.

## Open questions for Carlos

1. **bd-cxara / bd-cmp48** — device flow was superseded by loopback+PKCE.
   Close both as obsolete, or re-scope bd-cxara?
2. **Four local-only branches** (bd-g4uw7d8g, bd-hcp8m3ve, bd-e3lv7eg3,
   bd-7zxvdn0y) carry unpushed/unmerged work. bd-7zxvdn0y's branch appears to
   be **gone entirely**. Should these be pushed to origin so the work survives?
3. **k-422 / k-444 / k-449** — citeproc strands from Nov 2025 with no
   traceable commits. Worth a dedicated triage, or close as stale?
4. **bd-21q16** (contenteditable spike) looks superseded by bd-sjb4pzx8's
   tiptap work. Close as superseded?
