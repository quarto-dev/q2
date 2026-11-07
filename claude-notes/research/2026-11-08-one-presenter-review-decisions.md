---
title: 'One presenter path: decisions from the review discussion (step 1)'
date: 2026-11-08
description: 'What Gordon decided in the first half of the review session for the one-presenter-path epic and for the new q2 render --execution-records option, in his words where they were typed, before any review sub-agent was started. Also what the briefs of the two review sub-agents will change because of those answers.'
---

**Epic:** [one presenter path](../plans/2026-11-07-one-presenter-path-epic.md).
**Evidence:** [spike results](2026-11-07-one-presenter-spikes.md). Nothing was implemented, and the two
review sub-agents have **not** been started. One read-only design agent was run, for the render option (below).
This note is the record of step 1 of the review; the findings note and the rewritten epic come after the sub-agent
reports.

## Decided

Quotes are Gordon's words from the session; the rest is the option he chose.

| Question | Decision |
|---|---|
| Epic Q1: what one `present_cell` takes | Yes: one `present_cell` takes a struct of optional inputs, and `quarto_resolved_options` becomes one field of it, not the switch. "That is just what i had in mind, one path, not switching on an arbitrary field. ... i affirm it." |
| Epic Q3: who enforces `warning` and `message` | The presenter enforces an option exactly when the record's hash definition lists it as display-only; when the option is in the hash, capture enforced it and the presenter leaves the output alone. One rule replaces `knitr_visibility` and `CellVisibility::resolve`. Needs tests against the `warning-false-stderr` golden and the knitr `Hashed` rows, and a check that every engine whose hash includes an option enforces it at capture. |
| Epic Q4: anchors and expression starts for other engines | "I think we should adapt julia." Julia is in scope. Jupyter is not decided; it waits for what the protocol can supply (brief A3). |
| Epic Q5: what Julia carries | Answered by the spikes (nothing of the knitr inputs). |
| Epic Q7: the plain-render route of Julia and knitr | **In scope.** A plain render of Julia and knitr joins the recorded route. This makes the cost of building a record in memory the central implementation question (brief B6) and reopens knitr decision 24 (the retained fallback); both are listed as open below. |
| Epic Q2 and Q6 | Not discussed yet; they wait for the sub-agent reports, as planned. |

### `q2 render --execution-records`

Gordon's requirement: "we are going to need to support `q2 render --execution-records`". A read-only design agent
(model Sonnet; it edited nothing, built nothing, ran no tests) reported on how it would fit; the facts below that
I checked myself are marked.

| Question | Decision |
|---|---|
| R1: default list | `engine`, which is today's behavior; the flag is opt-in. |
| R2: read only, or also write | Read only. "`--execution-records` should only read (or execute engine), but you can combine it with `execute: record: true` when you need that. ... i'd rather not add more options." |
| R3: no source supplies a record and `engine` is not listed | Warn and leave the cells inert, as preview and `--no-execute` do. `--strict` promotes the warning to an error. A prose-only document stays silent. |
| R4: a stale or partly matching record | Treat it as not supplied: the walk moves to the next source. With `engine` listed it runs; otherwise R3 applies and the warning names staleness. This differs from preview, which shows a stale record, so the docs say so. |
| R5: one list for a project render, or a metadata key instead of a flag | **Open, needs research.** Gordon: "i know that carlos is going to ask why execution-records is not just a yaml field, so we'd better evaluate if there is any reason for it not to be. if we went that way, we would not need --execution-records because all commands already have -M (except convert, hmm)". The reasons it might not work are listed under "Open". |
| R6: does a supplied record change `want_record` | No. The splice already presents a supplied record through the Rust presenter. |
| R7: conflicts with other flags | `--no-execute` plus `engine` in the list: `--no-execute` vetoes running (it drops `engine`), and R3's warning reports any document left inert. `--replay` plus `--execution-records` is a clap conflict. `--replay` is permanent (EXR-17 keeps it, line 147; epic decision 9), so the pair stays. The dropped flags (`--cache`, `--use-freezer`, the `--execute*` family) are left alone. |
| Consolidation | "If there is an opportunity to consolidate at least the grammar and handling, now that we have many commands with the same option, we should do that in this epic." One shared definition of the grammar and handling for `preview`, `provide-hub`, `convert` and `render`. |
| Item 9 (the rename) | Fold into the consolidation plan, which names the flag `--execution-records` from the start. "But also note that it depends on the research about whether we could just use metadata attribute." If R5 chooses metadata, the consolidation changes shape (see Open). |

## What I checked of the render agent's claims (read)

- Verified: `render_to_file.rs` passes `options.captures` only to the pandoc leg (`render_qmd_to_pandoc_with_captures`,
  lines 539-544), and `core_presents` is `engine == "julia" && ctx.want_record && !ctx.units.is_empty()`
  (`engine/ts_record.rs:500-502`). `-M` exists on `render` and `preview` and sits above the document's metadata
  (`main.rs:96-103`, `:426-438`). The three `records` fields are at `main.rs:63`, `:352`, `:809`.
- Relied on without checking: that the capture splice runs before the engine stage and presents a supplied record
  through the Rust presenter by construction; the staleness rules in `capture_driver.rs`; the list of dropped
  flags; whether an engine stage still starts a kernel after the splice has replaced the cells (the agent marked it
  NOT CHECKED; it is a brief item for B).

## Open after step 1

1. **R5:** metadata key or flag. Reasons it might not work, not verified: a new `execute:` key could count in the
   document hash (the knitr epic says the structured profile excludes no `execute:` key; not confirmed in code);
   `positron` and directory sources are machine-local, so a committed value would break another machine; in a hub
   a guest's document is untrusted input and a field naming directories could choose files the server opens; a CLI
   path resolves against the working directory and a YAML path against its declaring file; `convert` has no `-M`;
   clap cannot check conflicts (R7) for a value that arrives through YAML.
2. **What Q7 in scope reopens:** whether knitr's retained fallback (decision 24, removal plan KNR-8) survives for
   plain render, and whether users get an opt-out while the route is switched.
3. **Jupyter** under Q4.
4. **Epic Q2 and Q6**, after the reports.
5. The two decisions in the RIU follow-up (now the RFU epic) that this epic does not own: the Phase A verdict and command 2
   (progress, cancel and the dirty-document wording).

## Changes to the review sub-agent briefs (not started)

Brief A (design and intent): A1 adds knitr decisions 24 and 26 and the open question on layering the document scope;
A2 adds a fifth situation, a render given a record, and what a plain render would route through if it built a record
in memory; A3 widens to "Julia is in scope" (where in the runner fork per-expression positions and output order could
come from, how knitr's capture produces them, which optional wire fields would carry them); A4 adds the safety check
for Q3 (does capture enforce each option its hash declaration lists); A5 adds the doc sentences that Q7, the Julia
work and the Q4 degradation would make false; A6 is unchanged but now central; **new A7**, the parity evidence
between record-presented and engine-owned output for knitr (the corpus, the allowlist, the flip gate) and Julia
(the EXR-8 parity suite).

Brief B (what the tree says): B3 adds the hash effect of a new `execute:` key; B4 adds the render route's callers;
B5 adds the hub-trust question for R5; B6 becomes the central question (cost of an in-memory record on a plain Julia
and knitr render, with timing runs, the writer gate, and whether an in-memory build already exists); B7 and B8 are
unchanged; **new B9**, the consolidation inventory (every site that declares, parses, validates or words an error for
the source list); **new B10**, the render agent's unverified claims (does an engine stage still start after the
splice, and does native HTML render drop a supplied record).
