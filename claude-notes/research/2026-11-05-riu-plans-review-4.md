---
title: 'RIU plans review 4: first-session implementer and end-to-end safety views'
date: 2026-11-05
description: 'Fourth review of RIU-1 to RIU-6, after the third review answers landed (5584723d2, f1b571676). Two read-only reviewers on branch feature/records-in-use; findings, fixes, and decisions for Gordon.'
---

**Reviewed:** `feature/records-in-use` after `f1b571676`: the epic, RIU-1 to RIU-6, the three earlier
reviews, and the code the plans touch. The `knr-5-positron-interop` branch was read with `git show` and
`git grep`, never checked out.
**Method:** a3 (this note) ran two read-only sonnet reviewers: E (can a fresh engineer start RIU-2 and
RIU-3a test-first) and F (does the demo route hold together and fail safely). Nothing was built or run.
"Verified" means I read it in the tree myself. Reviewer statements I did not re-read are marked
"inferred". **No settled decision is contradicted in a way I resolved;** findings 4 and 12 touch
decisions and are listed for Gordon.

## Blocking

1. **RIU-3 specifies the staleness guard two incompatible ways.** Verified. The "Third-review decisions"
   bullet and epic decision 5 (2026-11-05 list) say the guard skips only the engine-hash comparison and a
   per-unit mismatch still flags the page. Phase A (test) and Phase B (guard bullet) still said return
   `Ok(false)` before reading the source, so the first failing test an engineer would write forbids the
   behavior the decision wants. **Fixed in the text** (see Fixed). The semantics below stay open.
2. **Nothing carries per-unit staleness into `CaptureRef.staleness`, and the obvious reading is wrong.**
   Verified by E and consistent with the facts already recorded: `SpliceReport` is produced only inside
   the render stage, `capture_driver.rs` never renders, `record_one` hard-codes `Some(false)`
   (`capture_driver.rs:206`) and `recompute_staleness` is the only other writer. Three consequences:
   - An imported record published at boot is always fresh; a Positron cache older than the document shows
     no overlay until the first file-change event (`record_one` is skipped when a ref exists). No checklist
     covers the boot case.
   - `UnitStaleness::Stale` means no record cell matches by either hash (`record/matching.rs:74-79`
     per E, inferred), so every unit the Positron cache does not cover (an `{r}` chunk, a unit with no
     cached output) is `Stale`. "Any Stale unit flags the page" would raise the overlay on every partly
     covered page, contradicting the test "an unedited imported page stays quiet".
   - Getting the report to the driver is a choice: a render-time hook that writes the sidecar, or
     `recompute_staleness` redoing `scan_and_fill_ids`, `author_texts` and `match_units`. The stale RIU-3
     sentence "Phase B adds the report if it does not expose one" is wrong (it exposes one) but I left
     it, since the replacement is a design choice.
3. **No checklist item wires the Positron or directory source into the preview resolver.** Verified in
   the plan text: RIU-2 says `Positron` and `Dir` "resolve to nothing until RIU-3 supplies them"; RIU-3
   Phases B to D build conversion, import, locator and a provider method, and never the resolver arm
   or its caller. RIU-3a exit criterion 1 (preview shows Positron output) has no implementing item.
4. **The resolver trait has no signature and RIU-2 contradicted itself on where it lives.** Verified:
   RIU-2's "Third-review decisions" bullet said a trait in `quarto-core`; Phase B2 said the resolution
   function lives in `quarto-preview`. **Wording fixed** to agree with epic decision 1. Still unstated: a
   method name, arguments and return type; who implements it for the provider (RIU-3 Phase D uses a
   separate import method and the provider has no cache); that `SourceInputs` gains `ownership` in RIU-3
   Phase B, a change to a type RIU-2's trait already takes; and that the cache key derives from
   `SourceInputs.input_qmd`. RIU-4 says only "the trait comes from RIU-2".
5. **A mis-paired expanded-cell Div cannot be stamped by unit span as things stand (decision 7).**
   Verified by F at `engine_execution.rs:884-897`, `record_splice.rs:247-248` and
   `quarto-ast-reconcile/src/types.rs:91-123`: `merge_markdown_into_ast` returns a `ReconciliationPlan` and
   the splice discards it (`_plan`). The plan holds only index alignments (`KeepBefore`, `UseAfter`,
   `RecurseIntoContainer{before_idx, after_idx}`), no unit ids, byte ranges or handles, and the merged
   Div keeps the author's source info. So the plan does not identify the paired blocks by unit. Decision
   7's own fallback applies unless the splice is changed to retain a pre-merge range table. Also (inferred
   by F): stamping the surviving container would make the author's real Div atomic, a choice to confirm.
   Go to Gordon.

## Important

6. **Guard ordering gap.** Verified from the plans. RIU-3a adds the `--records` flag, but the
   `claim_and_spawn` guard lands in RIU-4. Between them `--records positron` advertises no execution
   while the HTTP re-execute and `auto` still start kernels. RIU-3b's browser check and the RIU-6 demo
   could run in that window. `claim_and_spawn` has exactly two callers (`re_execute.rs:144`, `:180`),
   verified. Options: move the guard (and its `ReExecuteConfig` field) to RIU-2 or RIU-3a, or label the
   flag "does not yet prevent execution".
7. **The 3a and 3b split is prose only.** Verified. "Estimated sessions: 2 to 3" and the Exit criteria are
   unchanged and mix both halves; Phase G (verify, clippy, nextest, browser) is the only verify phase and
   is in 3b; the minimal `--records` flag has no checklist item or test; 3a exit criterion 1 needs a
   browser that only Phase G has. RIU-4 asks each plan to name the flag code it owns; RIU-3 does not.
8. **"No code runs" is overstated.** Verified: `run_boot_pre_render_scripts` runs project pre-render
   scripts at boot (`quarto-preview/src/lib.rs:445-447`) with no check of `engine_policy` or the source
   list. The epic Goal ("no code runs"), decision 5 and RIU-4 "Never execute" should say "no cell or
   engine code" or the scripts should be gated. A choice. Inferred: the pre-engine pipeline also runs Lua
   filters and shortcodes.
9. **The demo route uses the editor UI, not the viewer overlay.** Verified by F at `PreviewStatusBar.tsx`
   and `StaleCaptureOverlay.tsx:59`: the editor status bar runs cells through ephemeral `exec/request`;
   only the viewer SPA overlay calls `/api/preview/re-execute`. So decision 5's "Re-execute overlay" and
   RIU-3's test "editing a cell on an imported page shows the overlay" are viewer-only, while RIU-4's
   guest-Re-execute-refused test is reachable only by curl or the viewer. The editor also offers "Clear
   results" to every collaborator. Which UI the browser check and the refusal test target must be said.
10. **A missing `Extension<ReExecuteConfig>` should fail closed.** Inferred from RIU-4's "no-argument form
    for the two test callers" and the handler's temp-dir fallback when `CACHE_DIR` is unset
    (`re_execute.rs:101-109`, as E and F read it). Unstated what the handler does without the
    extension. Also unresolved which plan deletes `CACHE_DIR`, `set_cache_dir` (`lib.rs:361`) and the
    `re_execute_with_registry_and_cache` seam: RIU-2 says the `Extension` moves there, RIU-4 says the
    static "is removed".
11. **`provide-hub --records q2,engine` is a no-op with the gate off.** Verified in the plans (the `q2`
    record has no captures, so decision 9 refuses it). **Text fixed** in RIU-4 (the epic decision 4 line
    was left as is; it is covered by RIU-4 and the decision itself is not wrong).
12. **The fall-through warning has no channel, and "no source supplies" can omit its cause.** Inferred
    from F's read of `capture_driver.rs:139-147`: the page diagnostic sink carries errors; `provide-hub`
    has no sink. A mid-list refusal followed by sources that supply nothing and no engine shows nothing
    unless the "no source supplies" diagnostic names the refusal. Decision 6's wording "a default
    preview is unaffected" is also false with the default list and `preview.engine: off`. Choices.
13. **The diagnostic is boot-only and fires on prose-only documents unless guarded.** `record_one` returns
    `Ok(false)` both for "nothing to record" and, after RIU-2, "no source supplied". It is not emitted for
    a `.qmd` added later, a path that already has a ref, or a record cleared in the editor. The
    standalone case (`ctx.project_files()` is `None`, so the driver returns `Ok(0)`;
    `capture_driver.rs:78-80`, verified) means `--no-project --records positron` shows nothing, and RIU-4
    lists `--join` and `--static` conflicts but not `--no-project`.
14. **RIU-6 puts K-S2 cases in the demo fixture, but K-S2 is the Ark R document and R is out.** Verified:
    RIU-6 line 96 ("R through Ark and R through knitr are out") against RIU-6 line 104 and RIU-3 line
    145; `knr-05` lines 13 and 153 name K-S2 as the Ark R run. Epic decision 11 is not resolved by me.
    Either the cases are Python and Julia equivalents or R is in the demo. Go to Gordon.
15. **Deferred provide-hub flip gaps.** Verified against the epic and the deployment `docs/operations.md`
    by F: the order is right. Missing: tell anyone running `provide-hub` against quarto-hub.com before
    the flip (RIU-1 Phase E has it); the rollback hazard in the four steps; and that the flip changes a
    default that `--server` or `$QUARTO_HUB_SERVER` can aim at any hub, whose version is not visible. The
    step-3 phrase "falls back as designed" contradicted S3 (see Fixed).
16. **Session sizing is stale after the split.** Verified for the headers (epic total unchanged at 9 to 12
    and 16 to 20; RIU-3 still "2 to 3"). Estimate, inferred: 3a is heavy for one session (conversion, the
    `SourceInputs` change with 5 callers, the three-tier locator, provider path, guard, resolver arm,
    staleness reporting, flag); 3b about 1 to 2; RIU-2 at the top of 2 to 3 or over; RIU-4 about 1 to 1.5.
17. **Docs not named by any plan.** Verified: `docs/guides/projects/preview.qmd` "Sharing" does not say
    whether a guest can re-run code, and the share banner (`share.rs:109-115`) says guests can "RE-RUN"
    the project's code, which is false when `engine` is absent from `--records`. Not named: the preview
    "Code execution" and `preview.engine: off` text, a user page for `q2 record export-positron`, and the
    page for the persisted `--data-dir` text.
18. **Items not testable as written (E1).** Verified: the mis-paired Div bullet is a research step with
    a conditional (finding 5); the language-gate test "flips once KNR-5 merges" cannot be written failing
    first; the three Positron-side limits have no expected values; the K-S2 cases have no fixture source;
    the "no source supplies" diagnostic has no unit test. RIU-2 Phase A is well ordered, and RIU-4
    Phase A has explicit "write it first and watch it fail" items.

## Minor

- The runbook "Engine-record writer gate" (`release-runbook.md:362-378`) still carries a two-release rule;
  RIU-1 Phase B owns the rewrite. Keep the sentence that a hub-bound writer must not outrun its hub, and
  re-scope "the hub it writes to" for the preview.
- Under `--share` the `positron` source and the content-hash fallback scan serve the host's Positron
  outputs to guests, and the fallback can pick another document's cache (inferred); no plan says so.
- A guest with `--allow-edit` can make a page stale, and under `auto` with `engine` in the list the host
  then runs guest-authored code with no consent prompt. The demo should not use `auto` unattended.
- Loose citations, all verified: `RIU-2` cites `capture_files.rs:303` without its directory
  (`crates/quarto-core/src/engine/`); the `author_texts` function is `record/author_text.rs:34`.
- KNR-5's gate wording matches the plans, but nothing says who merges KNR-5 first.

## Fixed

First pass (verified factual errors only):

- RIU-3 Phase A test and Phase B guard bullet aligned with the narrowed guard (epic decision 5).
- RIU-2 Phase B2 resolver location aligned with epic decision 1.
- RIU-3 Phase E ANSI item: the mechanism is `format_error_text` and `strip_ansi_codes` in
  `presenter/cell.rs` (`:280` here; on `knr-5-positron-interop` it also strips `evalue` and handles an empty
  `ename`; CSI only). The check against a real cache stays open.
- RIU-4: `q2,engine` for `provide-hub` needs `QUARTO_ENGINE_RECORD_WRITE=1` with the gate off.
- Epic provide-hub step 3: S3 observed a full-page Render Error, not a fallback.

Second pass, after Gordon answered the decisions below (rewritten in place, not appended):

- **RIU-2:** a decisions section (resolver in `quarto-core` with the trait, per-source outcomes,
  fall-through, never cached, staleness at publish); the walk and its tests; new Phase B3 (the execution
  guard, the `Extension` move, fail-closed on a missing config, the handler-test limit); sessions 3 to 4.
- **RIU-3:** split into 3a and 3b with an exit criterion each; the staleness rule (at least one orphaned
  cell) and its function; the wiring of the Positron and directory arms and the minimal flag; the provider
  path rewritten around the shared walk; the interim `.cell` rule with `interim_` tests; Python and Julia
  acceptance cases in place of K-S2; browser checks split into 3a (viewer) and 3b (editor UI and live
  share); documentation pages named; sessions 4 to 5.
- **RIU-4:** the guard moved out (RIU-2); the provider's trait implementation, constructor parameter,
  `cache` start-up error owner, the `q2` staleness check, the outcome diagnostic, the `--no-project`
  conflict, the persisted-store and "Clear results" limits, the pre-render script clarification, the docs
  pages and share banner text; sessions 1 to 1.5.
- **RIU-6:** the demo is the default viewer in Positron's preview pane (how the Quarto extension does it is
  recorded); a new Phase 0 spike for the viewer inside the pane, which can run first; K-S2 and R removed
  from the demo.
- **Epic:** Goal wording, decision 5, the 2026-11-05 list (items 1, 5, 6, 7, 8, 10, 11 revised in place),
  the plans table, session totals (about 12 to 16 for the remaining plans), the ordering diagram, the
  deferred provide-hub steps (notify users, rollback hazard, `--server` open question) and coordination.

## Rejected or discounted

- E reported `re_execute.rs:307` and `execute.rs:338` as wrong in RIU-2 lines 52 and 53. They are the
  `captures.is_empty()` guards the plan means to change; the cites are correct. Dropped.
- E reported `capture_files.rs:303` wrong; it is the engines filter the plan means. Only the missing
  directory is real (Minor).
- F said `presenter/cell.rs` strips ANSI at `:277-282` "on this branch" and E reported RIU-3 line 84's
  `author_texts` cite wrong; I re-read the first (correct, at `:280`) and could not confirm the second
  from line 84, so it is dropped.
- F's claim that the editor UI runs cells only through `exec/request` I did not re-read (inferred);
  finding 9 stands on that reading and needs a browser check.
- Not verifiable by me: Linux and Windows Positron cache paths, VS Code URI encoding, the two-binary
  `--share` case, the hosted hub's deployed version.

## Decisions (answered by Gordon, 2026-11-05)

1. **Per-unit staleness for an imported record:** stale means at least one orphaned cached cell; uncovered
   units do not count; one function in `quarto-core` serves publish and file change. Agreed.
2. **Resolver trait shape:** `quarto-core` owns the list, walk and outcomes; the trait has `lookup` and
   `run_engine`; Positron and Dir are shared code; the provider implements it in RIU-4. Agreed.
3. **Mis-paired Div:** option A, the edit affordance refuses a `.cell` Div, as an interim rule and not a
   contract, because engines will later return provenance data. Agreed.
4. **Wiring and guard placement:** Positron arms are shared code; the execution guard moves to RIU-2. Agreed.
5. **Product versus demo:** the product includes the default viewer and `--ui editor` with live share; the
   demo is the default viewer opened in Positron's preview pane, with an early spike of the viewer inside
   the pane. Agreed.
6. **Pre-render scripts:** reword to "no cell or engine code"; scripts still run. Agreed.
7. **K-S2 and R:** Python and Julia equivalents; the R cases stay KNR-5's. Agreed.
8. **Fall-through channel and the "no source supplies" text:** per-source outcomes from the walk; page
   warning in the preview, stderr in `provide-hub`; `--no-project` conflict. Agreed.

Still open for later: the `--server` question on the day of the `provide-hub` flip.

## What was run

Only greps, file reads and `git show` or `git grep` against `knr-5-positron-interop`; two read-only
reviewers. For the preview-pane research, the bundled Quarto extension in `~/.positron-dev` and the
`positron-spike-s3` worktree were read (never `/Users/gordon/src/positron`). No builds, tests, browser or WASM
chain. The `q2 render --strict` of the changed files is the only command that ran code.
