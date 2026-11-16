---
title: 'OPR-9a: spikes for the plain-render switch (a note)'
date: 2026-11-10
description: 'Six short measurements that settle the open decisions of the old OPR-7d before the other pieces are built: the format matrix for Julia on every target, a prototype of the presenter fix, a prototype of the interleaving normalizer and its pin, zip extraction, reserved option keys, and the browser replay path. Produces one research note; commits no prototype code.'
status: complete
depends-on: []
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Origin:** split from [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (now a tombstone) on 2026-11-10 by Gordon's decision; evidence in the
[7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) and its addendum, and the [7d plan review](../research/2026-11-10-opr-7d-plan-review.md).
**Branch:** `opr/9a-non-html-spikes` · **Lane B** (`workspace-4`) · **Blocks:** 9b, 9c, 9d, 9e.
**Needs:** Julia 1.12 with Plots, R 4.3 with knitr, `pandoc`, `typst`; a `cargo build --bin q2`; for S6 possibly `hub-client` tooling.

> **Reading text copied from OPR-7d.** Passages copied from the old 7d plan say "this plan" or "7d". Read them as this
> piece of the series 9a to 9g (see the [tombstone](2026-11-08-opr-7d-switch-plain-render.md)). Marks: (read), (ran), (relied).

## Why this plan exists

The old 7d plan held a normalizer, a pinned entry, zip comparison, five new Julia target classes, a presenter fix, a
flip and a preview chain. The review that split it found five things nobody had measured. This plan measures them
first, so the later pieces (9b to 9g) are written from facts and the decisions that need Gordon are put to him once,
with a recommendation each. It writes one research note and no production code.

## Facts already known (do not redo; marks are from the review and the first four spikes)

- Julia gate on the integration tip (ran): 11 identical, 1 allowlisted, 4 blockers (`julia/capture-facts`,
  `julia/fig-subcap`, `julia/presentation-attrs`, `julia/records-plot`), 1 both-failed (expected). `records-plot` and
  `presentation-attrs` differ by interleaving only; `fig-subcap` also differs by 7j (not landed); `capture-facts` has four
  kinds of difference (the `data-prompt="true"` attribute, the `&gt; ` prefix, numeric `echo`, stderr blocks), 51 diff lines.
- **S1 (ran, 2026-11-10).** The Julia runner (a Julia package, `render_mimetypes.jl:42-75`) gives only `docx` and `typst` an
  image-only list; every other target gets `text/html`, svg, png and pdf. The presenter picks `text/html` first on every target.
  Of 22 formats rendered, html, revealjs, dashboard and epub match (html by interleaving alone); docx and typst match except the
  figure directory name; **gfm, commonmark, markdown, djot, rst, org, native, textile and mediawiki inline a base64 image** on the
  recorded route; **pptx, odt, rtf, icml, fb2, docbook and plain lose the figure**. The plain route already names the directory
  `figure-<format>`; only the recorded route hardcodes `figure-html`. `pdf` is "not yet supported" on both routes. No runner change
  is needed.
- **S4 (ran).** docx, pptx and epub render on both routes and can be compared by member after canonicalizing; the recorded docx
  carries extra bookmarks with hashed names (a real difference; S2 found the origin, the cell-wrapper ids, and 9b fixes it). The draft document is in the results note.
- **S5 (ran, Julia only).** `quarto-unit-id` is the only reserved key that does harm (knitr aborts); the other `quarto_*` keys an
  author types reach cell metadata but are inert.
- **S2 (ran, 2026-11-10).** With the 9b prototype (7j was already in the tree, so its prototype was not applied) recorded matches plain on figures and
  directory for all 22 formats; Julia harness: gfm, typst and revealjs differ by interleaving only (docx and pptx cannot be read until 9d); `cargo nextest
  run -p quarto-core` gives 8256 passed, 0 failed, so every 9b test is new. A second hardcoded `figure-html` lives in `engine/ts_presentation.rs:83`
  (9b must fix it). Jupyter (stored notebook): html unchanged, gfm, pptx and docx gain a figure file. The docx bookmarks come from the cell-wrapper ids of 7f; Quarto 1 writes that id only for html-family, ipynb and JATS output, so 9b gates it by target (Gordon: faithful to Quarto 1), prototyped: 4 bookmarks to 0.
- **S3 (ran, 2026-11-10).** `scraper` fails trap 1; `lol_html` in two passes (mark, then splice) equalizes `presentation-attrs`, `records-plot` and
  `fig-subcap`, every applicable negative control is reported, and `capture-facts` has 24 lines left (2, 6, 4 and 12 by kind). The pin goes in a file beside the allowlist.
- **S6 (read).** The flip adds one runtime path in WASM: a replayed record is now coalesced, and a stale capture could fail.
- Flipping `EngineExecutionStage::new()` fails three tests in `quarto-core` and `quarto`: two `engine_record_stage` tests and
  `knitr_chunk_records::knitr_chunk_id_is_not_emitted_as_an_attribute` (a real regression, see 9c) (ran).
- A nextest run rebuilds `target/debug/q2` with whatever the tree held. After any experiment run `cargo build --bin q2` on
  a clean tree before you measure the plain route.

Full results: [the 9a results note](../research/2026-11-10-opr-9a-spike-results.md).

## Rules for a spike

- No prototype code is committed. Make a prototype as a patch in your session scratchpad, measure, then `git checkout -- .`
  and confirm `git status` is clean. Say which binary each measurement used and that it was built from which tree state.
- Scratch dirs live in the session scratchpad, not `/tmp`. No pushes. Use `cargo nextest run`, never `cargo test`, never
  pipe it through `tail`.
- Every claim in the note carries a mark. A spike that cannot answer its question says so; it does not guess.

## The spikes

**S1. Format matrix. Done (2026-11-10).** Results above and in the note (22 formats, both routes). Left over: the formats the
note lists as skipped (json, ms, man, texinfo, tei, the wiki families, the slide-show families); run them as part of S2 if cheap.

**S2. Prototype the presenter fix. Done (2026-11-10).** Results in the note. The original brief: *Question:* after the fix, do plain and recorded match beyond interleaving on
every format, and what else changes? *Method:* in a scratch state apply OPR-9b\'s change (a target field on `CellOptions`,
the `text/html` branch gated by `display_data_mime_type`, the figure directory named from the format) and the two-edit
OPR-7j prototype (its diff is in the 7j plan) if 7j has not landed. Rebuild, repeat the S1 matrix (22 formats, plain and `execute: record: true`, one scratch dir each; the first session\'s `run.sh` is gone with its scratchpad, so rewrite it), and run the Julia part of
the harness with gfm, typst, revealjs, docx, pptx added temporarily to the `julia/fig-subcap` and `julia/records-plot`
manifest entries (revert). Run `cargo nextest run -p quarto-core` and list every test that fails. Also run a Jupyter notebook with a Plots-like bundle (`text/html` and an image) to gfm, pptx and docx before and after, to count the Jupyter results that change; that needs the Jupyter venv. *Decides:* 9b\'s test list
and scope; whether gfm and pptx can graduate in 9e; the number and names of Jupyter results that change on non-HTML targets
(for the landing report and OPR-8\'s notes).

**S3. Normalizer prototype. Done (2026-11-10).** Results in the note. The original brief: *Question:* does the design in 9d work on real pages, and which parser? *Method:* prototype
`julia-interleaved-cell` against the four real pages (with the S2 state for `fig-subcap`) and the seven negative controls in
9d, once with `scraper` (parse and round-trip both sides first; `scraper` keeps no source spans, so splicing back is not
possible) and, if that fails trap 1, once with `lol_html` (a normal dependency that preserves untouched bytes). Produce the
post-normalizer `capture-facts` diff and say how many lines it has and how they group by kind. *Decides:* the parser; where the
pin lives (a TOML literal string, or a file beside the allowlist); whether 51 lines is signable or the entry should show a
count per kind with the full diff attached to the report.

**S4. Zip extraction. Done.** The extractor spec and the draft fixture are in the note; 9d implements the extractor and 9e commits the fixture.

**S5. Reserved option keys. Done.** One key; 9c\'s scope is one key.

**S6. Browser replay. Done (static).** One new path; 9g\'s WASM box adds one replay test. No WASM build was run.

## Deliverable

One note, `claude-notes/research/2026-11-NN-opr-9a-spike-results.md` (render it with `q2 render --strict`), with a section per
spike: question, method, what you ran, result, what it decides. End with **Decisions for Gordon**, each with your recommendation:

1. gfm and the rest of the non-HTML family: graduate if S2 shows parity, else gate. S1 shows it is about eighteen targets, so the
question is which graduate for Julia and in what order. Recommendation: docx, typst and epub first. 2. Does the Julia target gate
also apply when `want_record` is true? Gordon agreed (2026-11-10) to settle it after S2: once 9b fixes the presenter the gate
shrinks to the unverified tail of formats, and one rule is enough (asking for a record never changes the output route).
3. The post-flip rule for the gate-time entries (recommended in 9g: remove them at the flip, keep the normalizer code and tests,
assert identity with an empty allowlist). 4. The pin\'s form (S3). 5. Any Jupyter result S2 shows changing that Gordon did not
expect. 6. The docx bookmarks: decided by Gordon (2026-11-10), a presenter fix in 9b that is faithful to Quarto 1 (the auto cell id only on html-family targets).

## Checklist (every box is `[unattended]`; the decisions in the note are Gordon\'s)

- [x] S1 run; matrix table in the note.
- [x] S5 run; reserved-key list in the note (done when 9c\'s scope is one sentence).
- [x] S4 run; extractor spec and the draft fixture in the note.
- [x] S2 run; patch kept in the scratchpad (`9b-prototype.patch`), tree clean afterwards (verified, then `cargo build --bin q2` on it), the S1 matrix re-run (22 formats), failing tests listed (none: 8256 passed).
- [x] S3 run; parser verdict (`lol_html`) and the post-normalizer `capture-facts` diff (24 lines, by kind) in the note.
- [x] S6 answered (static; the WASM run is 9g\'s).
- [x] The note\'s "Decisions" is complete (five settled, three open for Gordon); rendered with `q2 render --strict` (the 46 `Q-12-13` listing errors are the same on an untouched note, so they are not this change); committed; landed with the protocol\'s rebase and fast-forward.

## Stop conditions

Stop, commit your progress, write "Notes from an earlier attempt" and tell Gordon (the protocol's "When you cannot
finish") if any of these happens:

- a spike cannot be run because a tool is missing (R, Julia, Plots, pandoc, typst): report which, do not skip it silently;
- the working tree is not clean after you revert a prototype, or a build leaves `target/debug/q2` in an unknown state you cannot
  rebuild;
- a measurement contradicts a "fact already known" above: say so first, because other pieces rely on it;
- you would need to change production code to answer a question: stop and ask.

## Handoff

End the session with the protocol's handoff ("Ending your session"). Name 9b, 9c and 9d as startable when 9a is `complete`, and say which decisions in the note Gordon must answer first.

## Exit criteria

The note is on the integration branch, every spike has an answer or an honest "could not", the decisions for Gordon are
listed with recommendations, and the working tree was clean after every prototype.

## Notes from the run

S1, S4, S5 and S6 ran in the first session; S2 and S3 in the second (2026-11-10). Surprises: 7j was already on the integration branch, so the handoff\'s 7j prototype was skipped; and the S2 prototype needed a second `figure-html` fix that 9b\'s plan does not name. Startable now: 9b (lane B), 9c (lane C) and 9d (lane D; `workspace-6` must be clean first). Nothing in the note is open for Gordon; he also sets 9e `ready` once he has read the settled graduation and `want_record` rules. Since then Gordon has settled the three: the `quarto-unit-id` diagnostic is an error, the docx bookmarks are a presenter fix (9b), and 9c covers one key on knitr chunks.
