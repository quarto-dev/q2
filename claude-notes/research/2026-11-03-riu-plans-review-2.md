---
title: "Second review of the records-in-use plans (RIU-1 to RIU-6)"
---

## What was reviewed

The epic, RIU-1 to RIU-6, the RIU-0b plan and findings, at commit `3ed2df0f0`. Two read-only
sub-agents (Sonnet) reviewed them: A on provenance and the record model, B on test-first
executability, the CLI, the hub and cross-platform. I read nothing of the code in bulk; I
re-verified by grep and reading only the findings I act on or report as blocking. I ran no
build and no test. B ran `target/debug/q2 record export-positron x.qmd` (exit 2, unrecognized
subcommand) and `target/debug/q2 preview --records cache .` (exit 2); that is the only
execution anywhere.

No finding contradicts settled decisions 1 to 15. Two have incomplete premises (decision 9 and
decision 14, below).

## Verified by me

- **V1 (blocking).** `encode_capture_payload` writes a record only when
  `record_gate && !r.metadata.quarto_engines.is_empty()` (`capture_files.rs:303`); otherwise it
  writes the legacy capture list, empty for a record-only run. `describes_something` does not
  exist in the tree (grep). So an engineless imported record is published as an empty legacy list
  unless the predicate is added and the gate is on.
- **V2.** RIU-3 Exit criteria says provide-hub shows Positron outputs through the import-only path
  (`riu-03` line 335). RIU-1 keeps the provide-hub default off until the hosted hub deploys the
  reader. So that criterion is not reachable by default, and local preview needs the RIU-1
  preview flip too, yet RIU-3 Depends names only RIU-2.
- **V3.** The epic's own S2 bullet says a record-only entry "is meaningful only with the gate on",
  but decision 9 says gate off means "a legacy hub payload and nothing more". Decision 9's
  wording is the inconsistent one.
- **V4.** Nobody owns the change to `encode_capture_payload` (epic says RIU-2, RIU-3 Phase B says
  "from RIU-2", RIU-2's five-place list omits it).
- **V5 (blocking).** `trigger_auto_re_execute` calls `claim_and_spawn` directly
  (`re_execute.rs:174-180`), not `re_execute_handler`. RIU-4 gates only the POST, so under
  `preview.engine: auto` with a list lacking `engine` a staleness flip still starts the engine.
- **V6.** RIU-2 lines 42 to 43 contradict each other: a gate-on run writes no `.bin`, yet "the
  gate is not read in `cache.rs`". Which side is intended is a design choice (Decision D3).
- **V7.** `heading_id.rs:56` treats a `Generated` header as having a deliberate id. A stamped
  `Header` (stored markdown) therefore keeps its id rather than having it derived. So the
  `CodeBlock`-only exemption list is not shown to be complete.
- **V8.** `coalesce.rs:96-105` builds record metadata with `..Default::default()`, so an engine
  record has no `quarto_provenance` in its `extra`. A `.quarto` or `dir:` file can carry it.

## Fixed (factual only)

- Epic: `execute.rs` `~:528` is `:530`; the bare `preview.rs:248-255` is
  `crates/quarto/src/commands/preview.rs:248-255`.
- RIU-1: the registry override is applied at `render_to_file.rs:504-506`, not `:500`.

Other findings files cite the bare `preview.rs` path (`riu-00` line 128, RIU-0 findings lines 69,
120 and 236); I left them as they are, since they name the file unambiguously by context.

## Findings only inferred or taken from a reviewer without my re-check

Important (reviewer claims, read-only, not re-checked by me except where marked above):

- **B-3 import inputs.** No parsed document exists before the pipeline; `compute_source_inputs`
  (`preview_record.rs:286-345`) is the only place one exists, and `SourceInputs` carries no AST or
  `ConfigValue`. `resolve_engines_for_document` (`resolution.rs:439`) needs both. RIU-3 lines
  87 to 90 should say so. B's smallest design: add `ownership` and a registry override to
  `compute_source_inputs`, resolve before `mask_document`, and put the import in `quarto-core`.
- **Source-list type home.** `quarto-core` (both `quarto-preview` and `quarto-hub-provider` depend on
  it; `quarto-hub` does not). An enum cannot hold fakes, so the resolver needs a trait or closures
  for the test plans' "fake sources". `--records cache` on `provide-hub` has no cache directory; the
  plan does not say error or ignore.
- **Locator crate.** RIU-3 Phase C puts it in `quarto-preview`, which lacks `dirs` and `sha1`, and
  `provide-hub` cannot use it there. A native-only module in `quarto-core` would serve both. The
  locator must take host OS and environment as parameters to be testable on one host. A
  provide-hub run materializes a temp project, so the SHA-1 URI rule never matches there.
- **`Dir(path)` has no owner** (RIU-2 defers it to RIU-3; RIU-3 has no `dir` work), and what a
  compatible `.ipynb` is is undefined. What a source "supplies" (a Positron file with zero
  matching cells) is undefined and decides whether `engine` is ever reached.
- **Gate threading.** The hooks call `record_writing_enabled()`, which reads the environment, so a
  record-only integration test needs `set_var` (unsafe in edition 2024) or a gate parameter.
  RIU-2 says "pass `true`", which holds only for direct calls to the writers.
- **Re-execute and the list.** If `perform_re_execute` resolves the list, `positron,engine`
  returns the stale Positron record forever; the button should mean engine only. RIU-2 is silent.
- **Watcher guard.** RIU-2 Phase E says to move the lookup outside both `off` guards; at `:262`
  there is no lookup, only the staleness compare, and `config.rs:30-32` documents `off` as making
  the watcher hook a no-op.
- **RIU-4 plumbing.** `SharedContext` has no slot for preview data. Carry a `ReExecuteConfig` as
  an axum `Extension` (the `extend_with_preview` closure may capture it); `EnginePolicy` is built at
  `crates/quarto/src/commands/preview.rs:333-337` and already rides `PreviewConfig`. clap 4.5
  accepts a custom `value_parser` with `Append`; not compiled.
- **RIU-5.** No `record` group exists (confirmed by running it). Add `Commands::Record`; `main()`
  returns `anyhow::Result`, so exit codes 10 to 13 need `std::process::exit` as `render.rs:833`
  does. Whether a failing Julia cell makes `record_capture_run` return `Err` is unknown.
- **Stamp rule shapes (A1).** The spike tested only top-level cells and `text/plain` spans.
  Untested: cells in a div, callout, tabset, list item or blockquote; labeled cells; expanded
  cells; stored markdown; and spans alone in a paragraph or table cell, in a heading, link,
  caption or footnote. Reviewer inference: a paragraph consisting only of a span is replaced by
  reconcile and stamped wholesale; same-index pairing can mis-pair after a block-count change
  (`compute.rs:107-150`).
- **Consumers (A2).** Only CodeBlock-keyed consumers (`code_block_generate.rs:168-181`,
  `codeblock_shorthand.rs:677`) break outright. Changed behavior for stamped nodes: `heading_id`
  (verified, V7), `crossref_index.rs:402-465` and `:591-606` diagnostic locations (inferred), and
  scroll sync, since a `Generated` node serializes offsets 0 and 0 (`json.rs:380-398`) and loses its
  `data-loc` (inferred, no browser). The edit paths either fail loudly on `Generated`
  (`apply_node_edit.rs:25-66`) or never see the spliced AST. Rust `is_atomic_kind` has no
  production consumer; the viewer's TypeScript set is the only effect, and
  `utils/sourceInfo.test.ts:80-95` pins it.
- **Output bytes are not stamped.** Echoed code and text output are fenced code, so the node
  holding the text carries no mark; only its wrappers do. The epic Goal's "marked on the AST nodes"
  holds through ancestors.
- **J claims.** J6 and Answer 5 passed literal owner strings, not the registry's output; "julia"
  ownership rests on J1's record. J3 cites tests on mock engines (it says "tests", which is
  honest). J4 and J6's "original gate" results are not reproducible from the s8 branch, only from
  reading `positron_import.rs:147`. J8 predicts three images and Gordon saw two plus text. RIU-5's
  three drop rules do not cover an `md""` bundle (HTML, LaTeX, markdown and plain) or a Python
  `Markdown` display.
- **Decision hedges.** Decision 10 is settled and also under Decisions for review, and RIU-1 Phase
  B says it may invert. Decision 14's kind, R through Ark and the hosted `q2_ref` are open by
  design. "Decisions 1 to 9 of the plan review" (epic) uses a different numbering from the
  settled list.
- **Minor.** The epic's "Decisions 1 to 9" numbering clash; `data.source` is copied unvalidated
  from the record; RIU-4's Windows line omits `quarto-hub-provider`; RIU-6 line 102 says a vitest
  "only calls" the private `positron_loader`; the Recipe uses `$SCRATCH` without defining it;
  RIU-3 at 2 to 3 sessions looks light (judgment).

## Rejected

- Reviewer B's blocking item 1 is partly rejected: a record-only entry arising only if `store_run`
  stops writing `<key>.bin` is a design consequence the plan already states, not a defect. The
  verified part is the gate-threading and the V6 contradiction.

## Not run

Every browser claim (scroll sync, click-to-source), every test of the spike branches, the
7606-test claim, the uv install, Plots, Positron's rendering of Julia bundles, Windows URI rules,
the hosted `q2_ref`.

## Decisions for Gordon

- **D1.** The gate on a record-only entry. Either decision 9 is reworded (gate off means an empty
  page for a record-only run) or the cache refuses a record-only write with the gate off. RIU-3's
  provide-hub criterion and its Depends on RIU-1 depend on it.
- **D2.** Whether `describes_something` and the `encode_capture_payload` change belong to RIU-2
  (the epic) or RIU-3 (a plan).
- **D3.** Which side of RIU-2 lines 42 to 43 is meant (is the legacy file omitted when a record is
  written?).
- **D4.** Whether auto re-execute and the Re-execute button honor the source list, and whether the
  guard sits in `claim_and_spawn`.
- **D5.** Where the source-list type, the locator and the import function live (reviewers suggest
  `quarto-core`), and what `dir:` is.
- **D6.** Whether to widen the stamp exemption (`Header`) or audit stored markdown, and whether
  the atomic kind stays `jupyter-output`; evidence for both is in reviewer A's A3: the upstream
  doc comment and contract section 7 favor reuse, the name for knitr, Julia and Positron favors a
  new non-atomic kind, and an atomic Div drops edits for all its children.
- **D7.** Whether the staleness guard keys on the marker (settled) or also requires an empty
  engine list, since a `.quarto` file with the key and engines would never flag document-level
  staleness.

## Outcome (Gordon's decisions, applied)

D1: the gate stays; the publish seam refuses a record with no legacy form when the gate is off.
D2: RIU-2 owns `describes_something` and the encoder change. D3: the cache stores what the run has and
never reads the gate. D4: one guard in `claim_and_spawn`; re-execute runs the engine only; a source
supplies a record only if at least one unit matches. D5: the shared pieces live in `quarto-core`; a
directory source is a bare path read by the Positron importer (this revises decision 4: no `dir:`
prefix, no custom parser). D6: `Header` joins `CodeBlock` as unstamped, with an audit test, and the
atomic `jupyter-output` kind is reused with the viewer-edit condition (resolved afterward by RIU-0c spike 1: the cell Div is stamped and atomic). D7: the staleness guard requires
the marker and an empty engine list.
