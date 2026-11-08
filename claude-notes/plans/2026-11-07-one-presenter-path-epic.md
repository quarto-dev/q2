---
title: 'Epic: one presenter path for every engine'
date: 2026-11-07
description: 'Every recorded cell goes through one presenter that reads what a cell supplies, not which engine made it. Julia and knitr join the recorded route on a plain render too, Julia gains the capture facts knitr has, and the --execution-records option is one shared definition across preview, provide-hub, convert and the new q2 render. Decisions of 2026-11-08 are recorded; the plan list below is a proposal for the review sub-agents, and two questions (option resolution, the metadata-key alternative) stay open.'
status: 'spikes done; decisions of 2026-11-08 recorded; plan list proposed, not yet reviewed; still open are option resolution (Q2), metadata key versus flag (R5), the plain-render fallback and opt-out, Jupyter positions, and plan order (Q6)'  # 2026-11-08
---

**Follows from:** [the RIU follow-up plan](2026-11-07-riu-follow-up-did-we-build-it.md), Item 7 (and Item 9, which
folds into plan OPR-5 here), and [the review note](../research/2026-11-07-riu-follow-up-review.md), "Design findings
after the review".
**Evidence:** [the spike results](../research/2026-11-07-one-presenter-spikes.md) (S1 to S5; every measurement below
comes from it, with its ran, read and not-done marks) and
[the decisions note](../research/2026-11-08-one-presenter-review-decisions.md) (Gordon's words, 2026-11-08).
**Intent sources:** [knitr epic](2026-10-30-knitr-execution-records-epic.md), decisions 4 ("One presenter"), 12
("The Quarto cell-option layer lives in the shared presenter"), 24 (the retained fallback) and 26 (the flip gate);
[execution-records epic](2026-10-28-execution-records-epic.md), decision 5 ("The Rust presenter is the long-term
single presenter").

> **For implementers:** tests first. Check with `target/debug/q2` built in the checkout, never the `q2` on `PATH`.
> Gates per crate: `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>`.
> Say which steps ran, which were read only, and which were not done. Nothing is pushed without Gordon's say-so.
>
> **Who runs a box.** `[unattended]` can run without Gordon. `[Gordon]` needs a decision or a person. `[either]` is
> a run that Gordon may repeat by eye.

## Goal

Every recorded cell, from every engine, goes through one presenter path. The presenter reads what a cell supplies,
not which engine produced it: knitr\'s features (interleaved output, `prompt`, numeric `echo`, message and warning
handling, unknown-option forwarding, figure labels and captions) are open to any engine that supplies the inputs
they need, and degrade cleanly, one input at a time, for one that does not.

The scope is wider than "recorded cells":

1. **Plain `q2 render` of Julia and knitr joins the recorded route.** Today a plain render asks for no record, so
   Julia is presented by the TypeScript engine\'s `toMarkdown` and knitr by R\'s finished markdown. After this epic a
   plain render builds the record in memory and presents it through the Rust presenter (decision 4 below).
2. **Julia gains the capture facts knitr has** (expression starts, output anchors, output kinds), so its cells can
   use the features that need them (decision 3).
3. **`--execution-records` is one definition**, shared by `preview`, `provide-hub`, `convert` and a new
   `q2 render --execution-records` (decision 5).

Jupyter output stays byte-identical unless a cell sets a new option (knitr epic decision 12). The goldens in
`tests/fixtures/jupyter-markdown-goldens` guard that, with one weakness noted below.

## Decisions (Gordon, 2026-11-08; not to be reopened here)

1. **One `present_cell`, a struct of optional inputs.** One function takes a struct built from the cell\'s metadata.
   Its fields are anchors, expression starts, output kinds, figure count, resolved options, known options and
   echoed code, each optional. `quarto_resolved_options` becomes one field of it and stops being the switch between
   the knitr and Jupyter entry paths. The two per-cell loops (`present_document` and the splice stage\'s
   `present_cell`) share this one step. Capability is read from which fields are present, never from the engine\'s
   name or a marker key.
2. **Who enforces `warning` and `message`.** The presenter enforces an option exactly when the record\'s hash
   definition lists it as display-only; when the option is in the hash, capture enforced it and the presenter leaves
   the output alone. One rule replaces `knitr_visibility` and `CellVisibility::resolve`. It must be tested against
   the differing golden (`warning-false-stderr`) and the knitr sweep\'s `Hashed` rows, and each engine whose hash
   lists an option must really enforce it at capture (the rule trusts the declaration).
3. **Julia is in scope for the capture facts.** The Julia engine and its runner fork supply expression starts,
   anchors and output kinds as optional fields. Jupyter is not decided: its protocol reports no per-line positions
   (not re-checked), so it waits on what the protocol can supply (open question 4).
4. **Plain render is in scope.** A plain render of a Julia or knitr document builds a record in memory and presents
   it. This makes the cost of that record the central implementation question and reopens knitr decision 24 (open
   question 2).
5. **`--execution-records` is read only, shared, and not a new option set.**
   - It names where a document\'s record is read from, or says to run the engine. Writing stays with
     `execute: record: true`; the two combine when both are needed. No further options are added.
   - Grammar: a comma list or repeated flag; entries `cache`, `q2`, `positron`, `engine` or a directory path; the
     first source that supplies a record wins; no `engine` means never run.
   - On `q2 render` the default is `engine` (today\'s behavior; the flag is opt-in). One list applies to every
     document of a project render, walked per document.
   - A document no source supplies, with `engine` not listed, warns and its cells stay inert (as preview and
     `--no-execute` do); `--strict` promotes the warning to an error; a prose-only document stays silent.
   - A stale or partly matching record counts as not supplied: the walk moves to the next source. With `engine`
     listed it runs; otherwise the warning above names staleness. Preview, which shows a stale record, differs, so
     the docs say so.
   - A supplied record does not change `want_record`: the splice already presents it through the Rust presenter.
   - `--no-execute` vetoes `engine` in the list (a fresh record is still read; the warning reports any document left
     inert). `--replay` conflicts with `--execution-records` in clap; `--replay` is permanent (EXR-17 and execution-
     records epic decision 9 keep it). The parsed-but-dropped flags (`--cache`, `--use-freezer`, the `--execute*`
     family) are left alone.
6. **Consolidation.** The grammar and its handling are defined once for every command that takes the option. The
   rename of `--records` to `--execution-records` (RIU follow-up Item 9; unreleased, no alias) is folded into that
   work. It depends on the metadata-key research (open question 1).

## What the spikes found (evidence)

Marks are those of the [spike note](../research/2026-11-07-one-presenter-spikes.md); line numbers are for `8cfec0fc1`.

- **One renderer, one reader, two front ends that are nearly the same (S1, ran).** Every cell ends in
  `render_cell_whole` (`crates/quarto-core/src/presenter/cell.rs:131`); `CellOptions::from_lookup` is the one place
  that gives each option its meaning. Over 65 option keys the Jupyter front end (`from_config`) and the knitr front
  end (`from_knitr`) built identical options for 55 and differed for 10, all knitr\'s dotted aliases (`fig.cap`,
  `out.width`, ...), which only the knitr side renames (`canonicalize_options`, `options.rs:151`). Merging the
  front ends is a rename step, not a second reader.
- **Two loops, one reader behind them (S5, read).** `present_document` (`presenter/document.rs:104`, five non-test
  callers) and the splice stage\'s `present_cell` (`stage/stages/record_splice.rs:431`) are separate
  implementations of the per-cell step. Both try `present_knitr_cell` (`presenter/knitr_inputs.rs:59`), which
  returns `Some` only when the cell carries `quarto_resolved_options`, then fall through to `render_cell_with`.
- **With every capture input empty, the knitr entry path reproduces Jupyter\'s bytes except for `warning`
  (S2, ran).** 12 of 13 shadow goldens are byte-identical; the 13th (`warning-false-stderr`) shows stderr that
  `#| warning: false` should hide, because `knitr_visibility` (`presenter/visibility.rs:113`) deliberately does not
  read `warning`. **Weakness:** none of the 13 fixtures sets a document-level `execute:` key, so the goldens do not
  test scope layering; the six `ipynb-*` goldens use another route and were not run.
- **The document scope is not layered on the knitr splice path (S3, ran), and one observation is unexplained.** On
  the splice path a document `execute: {echo|output|include: false}` hides a knitr cell only if R already recorded
  that key. Yet a real `q2 render` of a knitr document with `output: false` hid the output although the record held
  `output: null`. What applies it on that route is untraced. "Layering is a no-op" is not established, and for
  `output` it is false by construction.
- **Julia carries none of the knitr inputs (S4, ran for the dump, read for the route).** A real Julia record has
  `quarto_cell_id`, `quarto_content_hash`, `quarto_engine`, `quarto_exec_hash`, `quarto_kernel`, `quarto_kind`,
  `quarto_lang`, `quarto_label` and the author\'s own `#|` keys, and none of the anchors, expression starts, output
  kinds, resolved options, hash rule or header keys. A Julia cell always takes the Jupyter entry path.
- **A plain render bypasses the Rust presenter for Julia and knitr (read, plus one unit test; not run).**
  `core_presents` (`engine/ts_record.rs:500`) is `engine == "julia" && ctx.want_record && !ctx.units.is_empty()`;
  `want_record` is set only by `execute: record: true` or an observer that asks for one (`engine_execution.rs:548-554`).
  For knitr, `plan_capture` (`engine/knitr/target_route.rs:78`) returns `capture: false` when no record is wanted.
- **Who can enforce `warning` and `message` follows from the hash (S5, read).** For knitr, `warning`, `message`,
  `error`, `include` and `output` are in the hash (editing one re-runs the cell), so R already dropped what the
  option forbids. For Jupyter all four visibility keys are display-only. The record carries each engine\'s
  declaration (`quarto_engines[].hash_definition`).
- **Features that need no capture-side input already reach Jupyter:** `collapse`, `comment`, `results: hide` and
  `asis`, `class.*` and `attr.*`, layout, `panel`, `code-fold`, `fenced.echo`. `prompt` applies but treats the code
  as one expression. The Jupyter option sweep (`presenter/jupyter_option_sweep.rs`) fails the build if a
  display-only option changes nothing.
- **Feature by input** (the complete table is in the spike note, S5): interleaving needs anchors; `prompt` and
  numeric `echo` need expression starts; numeric `warning` and `message` need output kinds; figure captions and ids
  need the figure count; echoed code needs `quarto_echoed_code`.
- **What Julia already has (execution-records epic, EXR-8, complete 2026-11-01).** The runner returns an annotated
  nbformat notebook with unit metadata; q2 builds and aligns the record; a parity suite gated the flip, and Julia
  presents through the Rust presenter when a record is wanted (`b4d827186`). Julia\'s flat hash declaration cannot
  say what the runner hashes (recorded as "documented, no action"). Widget outputs went to the widget epic.
- **The render option, from a read-only design agent (2026-11-08; partly verified, see the decisions note).** There
  is no `--execution-records` on `render` today. `render_to_file` passes `options.captures` only to the pandoc leg,
  so a native HTML render cannot carry a record, and a project render shares one options value across documents, so
  a per-document walk needs a new seam. A supplied record is spliced before the engine stage and presented by the
  Rust presenter whatever `want_record` is.

## Open questions

1. **R5: should the source list be a metadata key instead of a flag?** Gordon: Carlos will ask why it is not a YAML
   field; `-M` already exists on `render` and `preview` (not on `convert`), and per-command defaults could come from
   environment metadata. Reasons it may not work, none verified: a new `execute:` key could count in the document
   hash (the knitr epic says the structured profile excludes no `execute:` key); `positron` and directory sources
   are machine-local and would break another machine if committed; in a hub a guest\'s document is untrusted input
   and a field naming directories could choose files the server opens; a CLI path resolves against the working
   directory and a YAML path against its declaring file; clap cannot check conflicts for a YAML value. The
   consolidation plan (OPR-5) waits for this answer.
2. **What plain render in scope does to the retained fallback (knitr decision 24).** Does R\'s finished markdown
   stay as the fallback for a chunk the Rust presenter cannot map, and does a user get an opt-out while the route is
   switched? Needs the cost and the parity evidence (OPR-0).
3. **Q2: where option resolution lives and which values arrive.** The reader is shared (S1); the question is about
   values. Options: (a) layer the document scope under the recorded options at present time (not a no-op for
   `output`; the goldens cannot catch a mistake); (b) make capture record every presentation-relevant `execute:` key
   under its own name; (c) leave it. **Trace first** what applies a document `output: false` on the live knitr route.
4. **Jupyter positions.** Whether Jupyter-family engines other than Julia can supply anchors and expression starts,
   given an `execute_request` reports no per-line positions.
5. **Q6: how many plans, in what order.** The list below is a proposal for the review to test.

## Plans (proposed; for the review sub-agents to test)

Granularity is deliberately fine so each plan has one exit and a clear dependency. Plan ids are provisional.

| Plan | What | Depends on |
|---|---|---|
| OPR-0 | Tests first, traces and measurements | nothing |
| OPR-1 | One `present_cell` over a struct of optional inputs | OPR-0 (goldens) |
| OPR-2 | Enforcement derived from the hash definition | OPR-1 |
| OPR-3 | Option resolution (document scope) | OPR-0 (the trace); OPR-1 |
| OPR-4 | Julia supplies expression starts, anchors, output kinds | OPR-0 for the runner side; OPR-1 for consumption |
| OPR-5 | One `--execution-records` definition (and the rename) | the R5 research (OPR-0) |
| OPR-6 | `q2 render --execution-records` | OPR-5 |
| OPR-7 | Plain render on the recorded route for Julia and knitr | OPR-1, OPR-2, OPR-3, OPR-0 (parity and cost) |
| OPR-8 | Docs, embedded-docs refresh, end-to-end checks | the plans it documents |

**Parallel.** After OPR-0: OPR-1, OPR-4\'s runner side and OPR-5 can run at the same time (different crates and
repositories). After OPR-1: OPR-2, OPR-3 and OPR-4\'s consuming side. OPR-6 follows OPR-5 and does not wait on
OPR-1. OPR-7 is last among the code plans.

### OPR-0: tests first, traces and measurements

Exit: every item below has a recorded result, and the document-scope fixture is committed and green on today\'s
code.

- [ ] `[unattended]` Add a document-scope golden fixture: a Jupyter document whose front matter sets `execute:`
      keys (`echo`, `output`, `include`, `warning`), presented through both entry paths, so layering is pinned
      before anything moves. (The 13 goldens set none.)
- [ ] `[unattended]` Run the six `ipynb-*` stored-replay goldens through the knitr entry path (not done in S2).
- [ ] `[unattended]` Trace what applies a document `output: false` to a knitr cell on the live render route, and
      whether the preview and hub routes (which present from a stored record) do the same. Record file and line
      for each hop. Gates OPR-3.
- [ ] `[unattended]` Run a plain `q2 render` of a Julia and a knitr document and confirm by running that the
      engine-owned route is taken (read from code and one unit test so far).
- [ ] `[unattended]` Measure what building a record in memory adds to a plain Julia and knitr render: allocations,
      files written under `.quarto`, a second pass, wall time; and whether an in-memory build already exists.
      Gates OPR-7.
- [ ] `[unattended]` Measure parity between the engine-owned and the recorded route on real documents, for knitr
      (the corpus, the allowlist and the flip gate of knitr decisions 23 and 26) and Julia (the EXR-8 parity
      suite). Gates OPR-7.
- [ ] `[unattended]` Research R5: hash effect of a new `execute:` key, what a document-level or project-level key
      could make the hub provider read, path resolution for a key versus a flag, interaction with
      `preview.engine`. Gates OPR-5.
- [ ] `[Gordon]` Read the parity and cost results and the R5 research; decide open questions 1 and 2.

### OPR-1: one `present_cell` over a struct of optional inputs

Exit: one per-cell function used by `present_document` and the splice stage; no engine name and no marker key
selects a path; all 13 shadow goldens, the six `ipynb-*` goldens and the OPR-0 fixture are byte-identical (the one
known golden difference moves to OPR-2).

- [ ] `[unattended]` Tests first: the struct\'s fields, each independently absent, give today\'s output; a Julia
      cell with one field supplied gets that one feature and no other.
- [ ] `[unattended]` Add the struct, built from cell metadata; make `quarto_resolved_options` one field.
- [ ] `[unattended]` Move the dotted-alias rename into the shared front end (the ten aliases of S1) without
      changing any Jupyter result.
- [ ] `[unattended]` Replace the two loops\' per-cell step with the one function; delete the dead path. Gates:
      `quarto-core` clippy and nextest; the callers\' crates (the five `present_document` callers are all in
      `quarto-core`; confirm for others).
- [ ] `[unattended]` Cross-check the Windows build for `quarto-core` (`presenter/figures.rs`, `record_splice.rs`,
      `document.rs` build file paths and figure names).

### OPR-2: enforcement derived from the hash definition

Exit: one rule; `warning-false-stderr` passes through the knitr-shaped entry path; the knitr sweep\'s `Hashed` rows
and the Jupyter sweep\'s display-only rows still agree with the rule.

- [ ] `[unattended]` Tests first: for each engine, each visibility option is enforced exactly once (by capture or
      by the presenter), driven by the declared hash definition; an engine whose declaration and capture disagree
      fails a test.
- [ ] `[unattended]` Audit each engine: for every option its hash lists, capture enforces it (Jupyter, Julia,
      knitr, marimo). Report any mismatch to Gordon; do not paper over it.
- [ ] `[unattended]` Implement the rule; delete `knitr_visibility` and `CellVisibility::resolve`.
- [ ] `[unattended]` Check the user docs sentences about `warning` and `message` per engine.

### OPR-3: option resolution (document scope)

Exit: decided by Gordon after the OPR-0 trace, then implemented with the OPR-0 fixture proving the result.

- [ ] `[Gordon]` Choose (a), (b) or (c) of open question 3, with the trace in hand.
- [ ] `[unattended]` Tests first for the chosen option, including a document `output: false` on a knitr cell.
- [ ] `[unattended]` Implement. Check hash coupling: moving where an option is resolved must not change which
      options re-run a cell (`hash_rule.rs`, `jupyter_hash_definition()`, `record/ids.rs`).

### OPR-4: Julia supplies the capture facts

Exit: a Julia cell with several expressions interleaves output, honors numeric `echo` and `prompt`, and a
multi-expression plot cell gets figure ids and captions, all through the OPR-1 path; with the new fields absent the
output is unchanged.

- [ ] `[unattended]` Read what the runner fork and the engine extension expose today for per-expression positions
      and output order, and how knitr\'s capture produces the same facts; write it up before designing.
- [ ] `[Gordon]` Approve the design (which optional wire fields in `ts_protocol.rs`, versioned).
- [ ] `[unattended]` Tests first in the runner (`test/testsets/`) and the engine (`tests/unit/`), then q2 record
      building and alignment.
- [ ] `[unattended]` Extend the runner fork and the engine; vendor and pin them as EXR-8 did; end-to-end Julia
      render with Julia installed.
- [ ] `[unattended]` Record Jupyter as a follow-up in the epic\'s reconciliation, with what the protocol can supply.

### OPR-5: one `--execution-records` definition

Exit: the flag\'s clap declaration, default, allowed values, validation and error text exist once; `preview`,
`provide-hub` and `convert` use it; the old spelling is rejected as an unknown flag; no `--records` remains outside
`claude-notes/`. **Shape depends on open question 1**: if the source list moves to metadata, this plan changes.

- [ ] `[unattended]` Inventory every site that declares, parses, validates or words an error for the list (three
      clap fields at `main.rs:63`, `:352`, `:809`; per-command resolve functions; messages in `diagnostics.rs`,
      `record/sources.rs`, `execute.rs`, `re_execute.rs`; the preview and hub docs; the e2e files; the fork\'s
      extension).
- [ ] `[unattended]` Tests first through the real argument parser, per command: allowed values and default.
- [ ] `[unattended]` Implement the shared definition and the rename; update docs and e2e; then the embedded-docs
      refresh once.
- [ ] `[unattended]` Update the fork\'s extension and its vitests (RIU follow-up Item 9).

### OPR-6: `q2 render --execution-records`

Exit: the decisions in section 5 hold, shown through the real binary on a Jupyter, a Julia and a knitr document,
including a stale record, no source, `--no-execute` and `--replay`.

- [ ] `[unattended]` Tests first through the binary (model: `no_execute_cli.rs`), for each decision in 5.
- [ ] `[unattended]` Add the per-document seam to `RenderToFileOptions` and the render lookup (moving the `Q2` arm of
      `PreviewLookup` into `quarto-core`); carry a supplied record into native HTML render.
- [ ] `[unattended]` Check, by reading and running, that no engine starts after the splice has replaced the cells.
- [ ] `[unattended]` Document it: `docs/guides/authoring/execution-records.qmd` (which now says render only writes
      and never reads), `docs/guides/projects/preview.qmd`, the render reference.

### OPR-7: plain render on the recorded route

Exit: a plain render of a Julia and a knitr document is presented by the Rust presenter; parity with the old route
meets the OPR-0 gate; the decision on the fallback and the opt-out is implemented.

- [ ] `[Gordon]` Decide the fallback and opt-out (open question 2) and the parity gate, from OPR-0.
- [ ] `[unattended]` Tests first: plain render with a Julia and a knitr document equals the engine-owned route
      within the accepted divergences.
- [ ] `[unattended]` Switch the route; keep the control Gordon chose.
- [ ] `[either]` Run a real preview and a real render in a browser for one document of each engine.

### OPR-8: docs, embedded docs, end-to-end

Exit: user-facing pages are true; the embedded docs are refreshed once; end-to-end results are recorded here.

- [ ] `[unattended]` Pages that describe per-engine cell options (which sentences become false is a review item).
- [ ] `[unattended]` `cargo xtask build-agents-docs`, then `cargo build --bin q2`, once after the last docs edit.
- [ ] `[unattended]` `q2 preview` and WASM checks for the changed presenter (the WASM chain in `AGENTS.md`).

## Constraints that hold across the plans

- Jupyter output is byte-identical unless a cell sets a new option; the goldens are a weaker guard than they look
  until OPR-0\'s fixture exists.
- The `.cell` wrapper contract holds: one `Div.cell` per executed cell, the empty string when echo and every output
  are suppressed, exactly one wrapper per cell for the preview splice.
- No DOM postprocessor: any Quarto 1 DOM behavior is ported as an AST transform.
- Presenter code compiles for `wasm32` (the hub client); a signature change touches every crate that calls it.
- Every claim of "does not exist" in a plan is backed by what was searched and where.

## Out of scope

- Supplying positions for Jupyter (open question 4); a follow-up, not part of the merge.
- Fixing the parsed-but-dropped render flags.
- Writing records from the new flag (decision 5).
