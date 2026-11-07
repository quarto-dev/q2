---
title: 'knitr execution-records epic: paper review (2026-10-30)'
date: 2026-10-30
description: 'Findings of a paper review of the knitr execution-records epic and KNR-1 to KNR-7 before the spikes run: contradictions, claims the code does not support, and gaps, ranked, with what was verified and what was not.'
---

**For:** Gordon, to decide which findings to apply to
[the knitr execution-records epic](../plans/2026-10-30-knitr-execution-records-epic.md).

**Method.** Two Sonnet reviewers read the plans (A: composition and contracts; B: knitr,
Quarto 1 and harness design). The lead checked the top findings against the files. Each
item below says whether the lead verified it. Nothing in the epic or the plans was edited.

## Disposition (2026-10-31)

Gordon agreed to the recommendations for findings 1 to 25 and asked for the edits to be
made as rewrites of the affected text. They are in the epic, KNR-1 to KNR-7 and the capture
research note. Where the agreed recommendation left a choice, the edit took the conservative
reading and names it:
- **Positron delivery (finding 21):** the epic checks validity and a hand-copy in K-S1; it
  builds no delivery path. Epic decision 6 and KNR-5 say so.
- **Source order (finding 16):** the record carries source-piece positions; KNR-1 S2 decides
  the encoding.
- **Decided by Gordon on 2026-10-31, each as recommended:** the mixed-record document hash
  (R2, accepted; with no `execute:` key excluded by the structured profile it no longer
  arises at document level), the document scope is not layered over the record\'s options
  for now (R6; an `execute:` edit re-executes), no pattern form for `HashProfile` exclusions
  (R4), and KNR-1 may start once EXR-1 and EXR-2 are available (finding 22).
- **Still open:** the format in the record (R3, KNR-3 decision 8); the parent session was
  asked on 2026-10-31 and has not answered. The epic lists it under "Other decisions needed
  from Gordon".
- **Added:** KNR-1 spike S9 (inline spans), which KNR-4 needed.

## Read this first: two scope limits

1. **Reviewer A read the old parent epic.** The review ran against the parent as it was
   when our branch was cut (`67a6c7f69`). The parent has since gained four commits
   (decisions 24 to 40, EXR-4 largely rewritten, then hash profiles in `457229c47`), and
   our branch is now rebased onto `457229c47` with no conflicts. A\'s items marked *(old
   parent)* cite EXR-4, EXR-16 and parent-epic lines that have moved or changed; they were
   re-checked in a second round, reported in "Re-check against the parent tip" below. Items about KNR-3 to KNR-7 and knitr
   code are not affected.
2. **The knitr line numbers are from a development tree, not 1.50.** See finding 3.

## What the new parent commits change for us (lead check, not reviewer output)

- **Decision 35**: each engine declares a `HashDefinition` as data. The knitr declaration
  (EXR-11 decision 6) has EMPTY exclusion lists, so every chunk option counts. Our
  decision 6 and KNR-3 want a hash that leaves presentation-only options out. The
  override list does not name decision 35 or EXR-11 decision 6.
- The struct is an exclusion list, closed, with unknown options counting. KNR-3\'s
  "execution-relevant allowlist" has the opposite polarity and must be restated as a
  closed presentation-only exclusion list for knitr.
- **Resolved by `457229c47` (our request, accepted by the parent session):** a
  `HashDefinition` may list named `profiles`, and a unit may name one with
  `quarto_hash_rule`. A unit naming a profile the declaration lacks reads as stale, and
  the document hash drops an `execute:` key only if every profile used excludes it. No
  parent engine declares a profile. **Our epic must add the second profile for structured
  units and tag fallback units with the profile that keeps the all-options rule; KNR-3
  must say which profile each unit kind uses.** This closes the mixed-record part of
  finding 18.
- **Decision 34** now says matched outputs use the current options and scope layered
  over whatever options the record stores, with a current option winning. The knitr
  layering (header options over recorded resolved options) is left to EXR-14 and our
  epic; it is the missing reconciliation in finding 1.
- **Decisions 38 to 40**: inline ids are `{n}-{hash8}-inline` from one shared unit
  list, not `u7`. KNR-4 and the KNR-6 deletion list were written against the old
  mechanism.
- **Decision 36**: the stage computes author-text hashes. **Decision 39**: Positron ids
  share the format and content hash but may differ in label and index, and Positron\'s
  cache lookup consumes a file on one matching hash (relevant to spike K-S1).
- Numbers of parent decisions 6, 9, 10, 21 and 22 are unchanged.

## Findings, ranked

### Blocks the plan

**1. Decision 11 (presenter reads resolved options from the record) contradicts KNR-5 and
KNR-7.** *Lead-verified.*
- KNR-7 line 59 expects an edit to a presentation-only option to leave the unit fresh
  and show the new option. A fresh unit is not re-run, and the presenter would read the
  old resolved options from the record, so the edit does not show.
- KNR-5 line 60 says imported outputs take options "from the current document source, not
  from the record". That is the opposite rule for the same unit kind, and a
  Positron-imported unit has no R run, so a setup chunk\'s `opts_chunk$set(echo=FALSE)`
  is invisible to it.
- No plan says how a fresh record\'s stale options are overridden by a document edit.

**2. The capture seam of decision 3 does not exist where the plans put it.** *B items 15
and 17, A item 5; lead-verified in `block.R`.*
- "Right after `evaluate()`" has no hook except `knit_hooks$get('evaluate')` (block.R:303)
  or a patched copy of about 190 lines of `eng_r`. Neither is named in the plans (grep:
  no hits). Wrapping `sew` sees only filtered pieces.
- An `evaluate` hook is bypassed for `eval=FALSE`, blank code, and cache hits
  (block.R:308 to 311). The plans say a capture "sees" the pieces on a level-1 or
  level-2 hit (KNR-1:135, KNR-3:45).
- A level-2 hit holds `remove_plot(res, ...)` (block.R:339, comment: "when cache=2, we do
  not really need plots"), so it has no renderable plots. Not mentioned.

**3. Every knitr line reference is from knitr 1.52.5 (development, NEWS headed 1.53),
not the 1.50 the plans say they checked.** *Lead-verified* (`DESCRIPTION` Version,
`git describe` v1.52-69).
- `evaluate(` is at block.R:261 in v1.44, :262 in v1.50, :313 at HEAD.
- Behavior differs across versions in ways no plan names: numeric `eval` echoes masked
  lines with a `##` prefix in 1.50 but not at HEAD; the level-1 cache-hit path in
  1.44 to 1.46 goes through `fix_evaluate`; `dev.before`, `log.echo`, `fig.alt` merging
  and engine-modified options for the chunk hook are all post-1.50.
- "Verified on 2026-10-30 (knitr 1.50)" (KNR-1:37, KNR-2:15) is therefore false for the
  line numbers.

**4. Capture independent of `fig.keep` is impossible for `fig.keep='none'`, and the
default `'high'` merges plots.** *B items 10 and 11; B ran R, lead read block.R.*
- block.R:259 opens the device with `keep != 'none'`, so no recorded plot exists to
  capture. `fig.keep` is in knitr\'s own `cache2.opts` (block.R:170), so it is
  execution-relevant. KNR-3 line 73 lists it as presentation-only.
- Default `fig.keep='high'` merges low-level plot changes (three plot calls gave two
  files). A record of one image per recorded plot cannot reproduce knitr\'s grouping.
  Grep for `merge_low` and `low-level` in the plans: none.

**5. Forcing `stop_on_error` and `keep_*` is not a free choice.** *A item 13, B items 13
and 14; lead-verified block.R:315 to 319.*
- `stop_on_error` is derived from `error && include`, so `include` is execution-relevant,
  and KNR-3 line 73 lists it as presentation-only (KNR-3 lines 82 to 84 half-acknowledge
  this: "unless the capture forces them").
- B reports that `include=FALSE, error=TRUE` with a failing chunk aborts the whole knit,
  and that a constant "force 0" would let code after an error run. KNR-1 S1 lists only
  `error=TRUE`.
- B reports that logical `warning` and `message` are enforced by `evaluate(keep_*)`, not
  by the filters, so forcing TRUE and then running knitr\'s normal path would start
  showing warnings. That would break `capture_does_not_change_live_output` (KNR-1:181).
  *Reported by B from code and docs; not run.*

**6. KNR-1\'s byte-identity exit criterion cannot be met as written.** *Lead-verified.*
KNR-1 lines 181 to 182 require live output byte-identical with capture on and off, while
the epic (lines 207 to 209) says preserve ids differ within one document. B adds that
`htmlwidget-<hex>` ids and `plotly` `cur_data` ids differ between runs.

### Wrong claim

**7. "q2 already patches knitr\'s namespace, so capture needs no upstream change"
omits that `patch.R` already replaces `sew`.** *Lead-verified.* `patch.R` lines 142 to 149
override `add_html_caption`; lines 150 to 213 replace `sew` with a manual dispatcher;
`wrap_asis_output` (lines 99 to 139) does the caption, `{=html}` fencing and the `asis`
rule; line 292 overrides `valid_path`. A grep of all plans and the research note for
`wrap_asis_output`, `add_html_caption` and `valid_path` finds no hits. Consequences:
"wrap `sew`" means wrapping q2\'s version; KNR-1:189 collides with an existing override;
the knit_asis and htmlwidget presentation layer lives here and in no plan inventory
(KNR-2 inventories `hooks.R` only).

**8. "The list of rules is in this plan" (KNR-1:171 to 173) is false.** *Lead-verified.*
The plan has the test name and no list. The epic\'s stated list (preserve ids, temp
paths, figure directories) is incomplete (B measured `htmlwidget-` ids and `cur_data`
ids). Under the classifier (KNR-1:168), a missed normalizer rule shows as a blocker, not
noise.

**9. The epic says KNR-6 deletes EXR-11\'s chunk-hook capture, but KNR-6 and KNR-3 do not
agree with it or with each other.** *Lead-verified.* Epic lines 61 to 63 say KNR-6
deletes it. KNR-6 lines 132 to 134 say "fallback units keep theirs". KNR-3 line 140
replaces EXR-11\'s recorder already in Phase 3B. It is unclear which recorder writes the
record between KNR-3 and KNR-6.

**10. The pass-through presenter for `text/x-quarto-markdown` has no owner.** *Lead
checked the wording; A\'s grep not repeated for all parent plans.* Epic line 151 says
fallback units "already need" it; that is a need, not an existing thing. A reports no
parent plan builds it (EXR-11 only places stored markdown). Decisions 8 and 14 and KNR-3
depend on it.

**11. Decision 10 (legacy degrade) is named as "re-decided in KNR-3" but KNR-3 only
mentions it in passing** (KNR-3 line 23, "degrades to"). *Lead-verified; narrower than
A\'s claim.*

**12. The 1.44 "threshold" citations are not compatibility floors.** *B item 5.* `rmd.R:8`
is a `stop()` for `.R` spin input; `execute.R:774` picks `has_crop_tools`. The real
version branches are `patch.R` lines 6, 152 and 300 and `hooks.R` lines 332, 793, 842.

### Gap

**13. Hash polarity and the flat-key invariant.** *A items 2 and 3 (old parent).* The
polarity part is now settled (see "What the new parent commits change": restate KNR-3 as a
closed exclusion list, and use profiles). The flat-key invariant and key spelling
(`fig-width` against `fig.width`) still need a re-check against the new EXR-4.

**14. A hash computed before R runs cannot see setup-chunk state.** *A item 3 (old
parent).* Allowlisted options (`fig.width`, `dev`, `dpi`) can be set by a setup chunk, so
editing it makes only the setup unit stale. KNR-3 risk 2 does not address this.

**15. `eval.after` options (`fig.cap`, `fig.scap`, `fig.alt`) resolve after the chunk
runs** (block.R:343 to 345, B item 16), so "resolved options" are not available at the
capture point decision 3 names. Plot numbering (`fig.num`, `plot_counter`) is recomputed
from the filtered list. Grep: no hits in the plans.

**16. Source and output interleaving.** *B item 12.* Under default `collapse=FALSE`, knitr
emits alternating source and output fences; an nbformat cell has one source then
outputs, and the S2 mapping drops source positions. The plans say "execution order"
without saying where source pieces go.

**17. `knit_meta` rules depend on filters.** *B item 18.* `results='hide'` drops
`knit_asis` before `sew`, so its `knit_meta` is not added; `include=FALSE` still runs
`sew`. A record-derived dependency set would differ from the live one.

**18. Fallback unit rules disagree across plans.** *A item 9.* Decision 8, risk 3 and
KNR-3:128 list `child=`, level-3 hits and overridden hooks; decision 13 and KNR-2 add
LaTeX targets; KNR-3 and KNR-7 do not mention LaTeX. Other targets (gfm, epub, pptx) are
unassigned. The mixed-record hash declaration is resolved by hash profiles (see the
new-parent section above); KNR-3 must now use them.

**19. Cache level 3 and the skeleton.** *A item 7.* On a level-3 hit knitr returns the
cached string before any hook runs (block.R:100 to 101). Caches written before the flip
hold full markdown, not skeleton markers. KNR-1 S7 does not test that case.

**20. Inline plans disagree.** *A item 10.* KNR-4 assumes Rust-side substitution; KNR-6
leaves open whether it is R-side or markers. KNR-4 is "blocked on S1", but S1 covers only
block chunks, and no spike covers value formatting. KNR-4 line 72 keys the NULL rule on
"the recorded spelling"; the new parent records no spelling field (EXR-12 text was
rewritten, so re-check).

**21. The record is not delivered to Positron.** *A item 12 (old parent for the parent
line citations).* A q2 record lives under `.quarto/` and is never shared; the only path in
the plans is a hand copy in spike K-S1. KNR-5 exit criteria (lines 132 to 137) are
"written findings" with no pass or fail, so the plan is done even if Positron discards
the record. Figures: KNR-1:193 and KNR-5:60 carry file paths; parent decision 3 says
inline base64 (old parent; re-check). A Positron-written R record lacks resolved options,
`knit_meta` dependencies, figure paths and the format tag.

**22. Dependency table.** *A item 15 (partly old parent).* KNR-1 lists only EXR-1 but
needs the scanner and EXR-10\'s id hand-off (lines 183 to 193); "can start once EXR-1 is
available" (epic 264) conflicts with decision 1. KNR-5 lists EXR-16 as soft, but its
importer and validator are the base it modifies. KNR-6 line 92 says "resolve with the
parent", which is stale under Option A.

**23. Corpus coverage.** *B items 22 and 23.* No target-format axis (`kable`, `df_print`
and `prefer-html` change pieces by format); numeric `eval`, `warning`, `message`, `error`
and `fig.keep` not in 1D; setup-chunk options only in S8; `cache=TRUE` only;
`include_graphics`, `rgl`, `html_screenshot`, handled-language fences and Quarto cell
options (`code-fold`, `panel`, `column`) absent. Installed locally: `htmlwidgets`,
`plotly`, `ggplot2`, `kableExtra`; not installed: `DT`, `rgl`, `formatR`, `pagedtable`.

**24. Quarto 1\'s R files are byte-identical to q2\'s, so "Quarto 1 informs the oracle" adds
nothing on the R side.** *Lead-verified* (`diff -q` of `hooks.R`, `execute.R`, `patch.R`,
`rmd.R`, `ojs.R`: no differences). Any Q1 difference comes from Rust-side inputs and the TS
post-processing. B also names things the presenter must reproduce that no plan names:
`wrap_asis_output`; message formatting in `sew` (`Warning in <call>:` prefix,
`xfun::str_wrap` at the R `width` option, `strip.white`); `lastYamlCode`; the global setup
in `execute.R` 290 to 361; and the existing TS cell-option layer in
`ts-packages/quarto-api/src/jupyter/to-markdown.ts`, which is a better oracle for decision
12 than `hooks.R` and is not mentioned in KNR-2.

**25. Exit criteria.** *A.* Measurable: KNR-1 (except finding 6), KNR-4, KNR-6, KNR-7.
Weak: KNR-3 (no nbformat validator gate; the risk 2 test is not in the phases; fallback
rate has no threshold) and KNR-5 (see 21). KNR-2 is local-only by decision 17, so the bar
is an unrecorded run unless the report states counts.

### Nit

26. NAMESPACE lines 11 to 20 in the research note omit that `source` and `warning` are at
    21 to 22. The group loop in `output.R` is 346 to 370, not 349 to 362.
27. "About 25 call sites" of `engine_output` counts the definition and doc examples;
    19 lines contain `engine_output(options`.
28. Decision 17\'s skip pattern (`knitr_label_reinject.rs:37 to 58`) checks that knitr is
    installed, not a version; there is no version gate to copy.
29. Decision 12 says the cell-option layer is "inert unless a cell sets them", but a
    Jupyter document that sets `code-fold` or `layout-ncol` changes output (A item 16).
30. The 1.44 corpus runs of S1 will not have the same line numbers as those cited.

## Re-check against the parent tip (second round, `457229c47`)

Reviewer A re-read the parent text; the lead verified the items marked *lead-verified*.
The knitr epic and KNR-1 to KNR-7 are unchanged since the rebase, so their line
references in this note still hold.

**R1. Profiles settle the mixed-record hash, but KNR-3 does not use them.**
*Lead-verified* (grep for `profile` and `hash_rule` in all KNR plans and the epic: no
hits). KNR-3 lines 33, 63, 121 and 134 still speak of `hash_definition {id, version}` and
a "new id and version". The profile mechanism needs the same `knitr` id (the id prefixes
the hash, EXR-4 lines 327 to 329), a bumped version, a structured profile, and fallback
units tagged with the profile that keeps the old rule. A bumped version also restarts
every fallback unit\'s hash, which the plan does not say.

**R2. Mixed records defeat the structured profile at document level.** *A, new; the rule
is lead-verified (EXR-4 lines 337 to 340).* The document hash drops an `execute:` key
only if every profile used in the record excludes it. One fallback unit (all-options
profile) in a document therefore makes every presentation key in `execute:` count, so an
`execute: echo` edit re-executes a mixed record. This undercuts the point of the
structured profile for any document with a single fallback chunk. KNR-3 must decide.

**R3. The format tag contradicts parent decision 27 (undeclared override).**
*Lead-verified.* Decision 27 says the record carries no presenter inputs and stays
format-neutral; the output format is pipeline context at present time. KNR-3 line 30
tags units with `quarto_format`, and KNR-3 lines 96 to 97, KNR-6 lines 92 to 95 and
KNR-2 risk 3 assume the record carries the format. The override list does not name
decision 27. Separately, the format is not in any hash, so a record built for HTML reads
as fresh for a Typst render.

**R4. Closed-list polarity is sharper than before.** *Lead-verified.* `HashProfile` holds
only name lists (`Vec<String>`) and EXR-4 has no glob or wildcard language. Forwarded
options such as `class.*`, `attr.*` and `layout*` (KNR-2 lines 46 to 50) and unknown
attributes cannot be excluded by name, so they would count in the hash and editing
`class.output` would re-execute. KNR-3 must produce a closed presentation-only list that
covers every knitr option, or ask the parent for a pattern form.

**R5. The flat-key invariant fails for structured units.** *Lead-verified.* EXR-4 lines
259 to 262 compute flat keys as the cell\'s options minus the engine\'s default
`presentation_cell_options` (empty for knitr), so a structured knitr unit would record
every option, and the invariant with the hash breaks. EXR-4 does not say which list
applies when a unit names a profile. Key spelling (`fig-width` against `fig.width`) is
still unaddressed; grep for `fig-width` in KNR-1 to KNR-7 finds nothing.

**R6. Layering order of setup-chunk options against the `execute:` scope.** *A, gap;
could not verify.* Decision 34 layers the current document scope over the record\'s
options. In knitr a setup chunk\'s `opts_chunk$set` outranks the `execute:` defaults, so
layering scope over resolved options would reverse that. EXR-14 lines 221 to 223 leave
the knitr layering to the follow-on epic, and no knitr plan states an order. I did not
find where `execute.R` sets the defaults, so q2\'s actual order is unconfirmed.

**R7. Finding 1 is resolved for fresh records, not for the framing.** Decision 34 now
anticipates the exception, and KNR-7 line 59 is consistent with it. Epic decision 11 and
KNR-6 lines 63 to 69 still describe it as an exception to a rule the parent has
softened, and omit that a current header option wins. The setup-chunk staleness gap
(finding 14) and the imported-record blind spot remain.

**R8. Dependency table.** *A.* The parent table (now at lines 428 to 453) holds for the
edges the knitr table shows. Still open: KNR-1 needs the EXR-2 scanner but lists only
EXR-1; KNR-5 treats EXR-16 as soft though it modifies its importer; KNR-5 lacks a KNR-2
edge. Stale "open parent decision" phrases (all nine are settled): KNR-7 lines 28 and 50,
KNR-6 lines 92 to 95 (decision 27 contradicts its premise), KNR-2 line 206.

**R9. Override list.** The epic does not name decision 27, decision 35 with EXR-4\'s knitr
declaration, decision 14 (the importer gate widened by KNR-5 for token `r`), or decisions
36, 38 and 39 (the mechanisms that now answer KNR-3 item 7 and KNR-4 item 1; neither plan
cites them).

**R10. Smaller.** (a) Inline spelling: still no spelling field in the parent; KNR-4 lines
71 to 72 has nothing recorded to key on, and decision 38\'s document-wide span counter is
not mentioned. (b) KNR-3 does not say whether the knitr exec hash input is the stage\'s
canonical text or the text R receives (decision 36 and the label re-injection of EXR-10).
(c) Profile names are not in the hash prefix (id and version only), so two profiles with
equal lists hash alike. (d) KNR-5 risk 1 names structural surprise but not Positron\'s
delete-on-match, so any record copied for spike K-S1 is consumed on the first matching
cell. (e) The KNR-5 hash agreement test does not cover nested chunks or the label
divergence of decision 39.

## Which claims are predictions, not verifications

Beyond the epic\'s reviewer guide (B item 29): KNR-1\'s "4 to 6 sessions" (eight spikes
plus a harness); the epic\'s "20 to 30 sessions" (the sum of the per-plan ranges); "capture
before the filters is workable across 1.44 to latest" (the guide flags it, but findings
2, 4 and 5 are specific blockers); "the Jupyter presenter matches knitr\'s output divs"
(parity is tested only for block shape on simple cells); "Q1 gives Typst and docx no
special knitr handling" (true of `rmd/*.R`, but knitr branches on the output format
itself).

## Verified true

Block.R references at HEAD: 100 to 101, 214, 303, 308 to 324, 335, 337 to 340, 352 to 371,
392, 396, 425. `output.R` 295, 535 (`sew`), 576, 657. `hooks.R` is 1211 lines; the hook
ranges cited in KNR-2 are right. `mod.rs:157`, `types.rs:97`, `text_execute.rs:562`,
`knitr_label_reinject.rs:37 to 58`. EXR-11 decision 5 treats the options digest as
provenance, and decision 11 supersedes it consistently. The epic overrides parent
decisions 6, 9 and 21 correctly where it names them.

## Could not verify

Behavior on knitr 1.44 to 1.49 (history read, nothing run); forced `message=NA` and
`warning=NA` under `options(warn=2)`; whether Positron presents `text/markdown` or
`include=FALSE` outputs; whether Ark sends htmlwidget dependencies; whether the record
passes `nbformat.validate`; the research note\'s Experiment 1 (its fixture is not
committed); reticulate, SQL, shell, `html_screenshot`; the session estimates.

## Verdict: what can be reviewed further now

- **Can be reviewed further now:** KNR-1 (after fixing findings 3, 6 and 8, and adding
  the seam options from finding 2, which S1 depends on), and KNR-2\'s inventory (after
  adding `patch.R` and the TS cell-option layer).
- **Should wait for the spikes:** KNR-3, KNR-4 and KNR-6. They rest on the capture seam,
  the forced-option behavior (findings 2, 4 and 5) and the cache policy.
- **Needs a parent re-check first:** KNR-3\'s hash (decision 35), KNR-4\'s ids
  (decisions 38 to 40), KNR-5\'s record contract.
