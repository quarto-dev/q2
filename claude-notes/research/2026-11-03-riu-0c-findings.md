---
title: "RIU-0c findings: four spikes before the third plan review"
---

## What this is

Four small spikes that the second plan review left open because reading could not answer them.
The spike code is scratch on branches in the worktree `.worktrees/riu-spikes` and is never merged. Each
entry says what was run, what was observed and what was only inferred. Plan changes that follow from
these results are listed at the end.

| Spike | Branch | Commit | Question |
|---|---|---|---|
| 1 | `spike/riu-s10-viewer-edit` | `5e5cecff2` | Can echoed code inside a cell be edited in the viewer today? |
| 2 | `spike/riu-s11-stamp-shapes` (off `spike/riu-s9-provenance`) | `4f2c08402` | What does the stamp do on the shapes spike P never tried? |
| 3 | `spike/riu-s12-failing-cell` (off spike 2, which holds the same `record_capture_run`) | `a4bdc954f` | Does a failing cell make `record_capture_run` return an error? |
| 4 | `spike/riu-s13-typecheck` (off `feature/records-in-use`) | `a1a8b5bfd` | Do the plans' riskiest signatures compile? |

## Spike 1: echoed code in a cell is not editable today, and an atomic cell Div does not change that

**Answer: echoed code is not editable today; an atomic cell Div would not break anything that works.**

What decides editability. A `CodeBlock` takes no `setLocalAst` (`q2-preview/blocks/CodeBlock.tsx:166`
destructures only `node`). Its edit affordance is the attribute `data-block-pool-id`, set only when
`resolveSource(node)` finds the node's source info in an index built from the **untransformed** AST,
which holds only entries with type 0 and file 0 (`q2-preview/PreviewRoot.tsx:1509-1527`,
`q2-preview/sourceIndex.ts:62-70`). The atomic gate in `dispatch.tsx:413-419` only replaces a node's
`setLocalAst` with a no-op. The callers of `setLocalAst` outside `framework/` rewrite composite nodes
(`Figure.tsx`, `LineBlock.tsx`, `DefinitionList.tsx`, `Cite.tsx`, `CrossrefResolvedRef.tsx`,
`utils.tsx:232`), never a code block.

Run: a vitest integration test mounting `PreviewRoot` (`spike-riu-s10-edit.integration.test.tsx`,
`npx vitest run --config vitest.integration.config.ts <file>`), four cases, all passing:

1. An author-owned code block (file 0) in an unstamped Div has the affordance.
2. Echoed code resolving into the reparse slot (file 1), which is what the splice produces: **no
   affordance**.
3. An atomic cell Div (`jupyter-output` added to the TypeScript set, the plan's change) around an
   author-owned code block: affordance **unchanged**.
4. An atomic cell Div around slot-owned code: still none.

Confirmed in the real pipeline by spike 2: every echoed `CodeBlock` in a spliced cell carries
`Original(file 1)`.

Not run: a browser check, and a click-and-commit edit. The affordance attribute is exactly the gate
the hover editor uses, so the unit test is judged conclusive, but it is a jsdom test. A stale or
unmatched author cell is never stamped and stays editable.

## Spike 2: the stamp on the other shapes

Run: `cargo nextest run -p quarto-core -E 'binary(integration) & test(s11_)' --no-capture` on the spike
branch. The tests build a record with a stub Jupyter engine (`Mode::Stream`, `Empty` or `Value`), splice
it through `render_qmd_to_preview_ast`, and classify every node in the AST JSON by its source info.
They also render the full page twice, with the stamp on and with it off (an environment toggle in the
spike), and diff the HTML and the diagnostics. Nodes below are `Gen` (stamped), `Orig(f0)` (the
author's file) or `Orig(f1)` (the reparse slot).

| Shape | Result |
|---|---|
| Cell in a div, callout, tabset, list item, blockquote | Stamped correctly. The cell Div is `Gen`; the container stays `Orig(f0)` (a tabset container is `Gen(tabset)`). |
| Labeled and captioned cell (a float) | Stamped correctly: the cell Div inside the author's Figure is `Gen`. |
| Cell with no outputs | The cell Div around the echoed code is still stamped. |
| Expanded cell, then a cell in a div, then a plain cell | HTML is correct, but **mis-paired**: the second expansion's `.cell` Div carries the author's `.note` Div source info (`Orig(f0)`) because reconcile pairs by index and the block count shifted. It is unstamped and attributed to author bytes. The `.note` Div itself appears as `Orig(f1)`. Reconcile does this with or without the stamp. |
| Stored markdown cell (heading, paragraph, float, table) | Every non-code node is stamped (`Header`, `Para`, `Div`, `Figure`, `Table`, `Plain`, `Str`). |
| Inline value beside prose | Only the value `Str` is stamped. |
| Inline value alone in a paragraph | The `Str` is stamped; the `Para` is `Orig(f1)` (replaced by reconcile, unstamped). Not over-stamped. |
| Inline value alone in a table cell | The cell `Plain` **is** stamped (over-stamped: it is the author's cell). |
| Inline value in a heading, link, image caption or footnote | Only the value `Str` is stamped; containers keep `Orig(f0)` or `Orig(f1)`. |
| Copy-button scaffold around every `CodeBlock` | The scaffold `Div` and its `RawBlock` carry `Orig(f1)`: the code-block transform derives them from the unstamped `CodeBlock`, so they are never marked (under-stamped, by construction). |

Does stamping change a render? For the stored-markdown cell, the HTML is identical with and without the
stamp, including heading ids: an explicit `{#sec-stored}` id and two implicit `## Results` headings
(`results`, `results-1`) come out the same both ways. The duplicate-id diagnostic Q-15-1 differs in its
raw location (`Generated(jupyter-output)` stamped, `Original(file 1, 4949..4981)` unstamped) but renders
the same text with no snippet either way. **No kind other than `CodeBlock` was found to change a render
when stamped.** In particular the suspected `Header` effect (`heading_id.rs:56`) was not reproduced; the
`Header` exemption in decision 14 is therefore precautionary, not evidence-based (a decision for Gordon).

Not run: Windows, a `Table` with the value in a column header, expansions in a list or blockquote, a
labeled expanded cell, and the stamp through the live preview. The fixtures use the stub engine, not
real Jupyter.

## Spike 3: a throwing cell is an error unless `error: true`

Run: an example binary on the spike branch (`crates/quarto-preview/examples/riu_s12_fail.rs`) that calls
`record_capture_run` with the default registry, on documents with a cell that raises (Python) or calls
`error("boom")` (Julia), with a Jupyter venv and Julia 1.12.7.

| Document | Result |
|---|---|
| Python, a cell raises, default options | `Err`: `Stage 'engine-execution' failed: Execution failed in jupyter: code cell at doc.qmd:10:2 raised ValueError: boom`, with a hint to use `#| error: true` or `execute: error: true`. |
| Python, `execute: error: true` | `Ok`, record present, the failing cell has one `error` output, the cells around it keep their `stream` outputs. |
| Julia, a cell throws, default options | `Err`: `Execution failed in julia: Julia server returned error after receiving "run" command` followed by the stack trace. |
| Julia, `execute: error: true` | `Ok`, record present, the failing cell has an `error` output. |
| Both languages, no error | `Ok`, record present. |

So exit class 13 of the export (RIU-5) covers a cell that raised without `error: true`; with it, the
export proceeds and the error is an output of the record. Not run: whether Positron displays a Julia
error output (Python error output was shown correctly in S7).

## Spike 4: the plans' signatures compile, with these changes

Run: `cargo check -p quarto-core -p quarto-hub -p quarto-hub-provider -p quarto-preview -p quarto
--all-targets` after sketching each signature on the branch; a clap parse test through `cargo nextest
run -p quarto`. Production code of all five crates compiled. Test code was not repaired; the compiler
error counts below are the churn. Not run: `npm run build:wasm`, the Windows cross-check, and any
behavior (bodies are minimal or absent).

| Plan item | Result |
|---|---|
| RIU-2: the writer gate | The environment is read in exactly two places: `write_capture_doc` in `capture_driver.rs` and `run_and_store` in `execute.rs`. `write_capture_doc_gated` is already the test seam, so tests of the encoder, the refusal and the publish function need no new seam. Passing the gate through `record_eager_captures` and `record_one` compiles but touches 20 call sites in `quarto-preview` (2 production, 18 in tests). |
| RIU-2/4: carrying the gate and policy | Compiles: a `ReExecuteConfig { policy, sources, record_gate, cache_dir }` replaces the `(policy, cache_dir)` parameters of `record_eager_captures` and `recompute_staleness`. Compiler errors in tests: 36 in the `quarto-preview` lib tests, 19 in its integration tests (18 are `PreviewConfig` literals missing the new `record_sources` field). |
| RIU-2: the publish split | Compiles with **no new dependency edge**. The shared part is only document creation plus `repo.create` (`publish_payload_doc` in `quarto-hub`, about 10 lines). **`set_capture` is not part of either `write_capture_doc`**; each caller sets its own sidecar, so the plan's wording was wrong. The refusal is a new `quarto_core::engine::capture_files::PublishError` (`Io` or `RecordWriterOff`); `RecordError` gains a `Refused` variant and the provider maps it to `ProviderError::Protocol`. |
| RIU-2: `describes_something` | Compiles in `record/types.rs` and in `encode_capture_payload` as a one-line change. |
| RIU-2: the source-list type | Compiles as a target-neutral module, `quarto_core::record::sources` (an enum with `PathBuf`, the two defaults, `contains_engine`, and the entry classifier of RIU-4). WASM not built. |
| RIU-2: the resolver over a trait | Compiles with `#[async_trait(?Send)]`, but `quarto-preview` does not depend on `async-trait` today: **one new dependency line** (`async-trait.workspace = true`, already a workspace dependency). A `dyn` trait object needs it. |
| RIU-3: `SourceInputs.ownership` | Compiles as described: `resolve_engines_for_document(&doc_ast.ast.meta, &doc_ast.ast, &ctx.registry, ctx.claimed_engine_name.as_deref(), Some(DOCUMENT_OWN_FILE_ID))` inside `compute_source_inputs` before `mask_document`. The registry-override parameter changes **5 callers** (3 in `src`, 2 in tests). |
| RIU-3: `scan_and_fill_ids` | `pub(crate)` is enough because the import step lives in `quarto-core`: no visibility change. |
| RIU-4: the `Extension` | Compiles: `Some(move |r| extend_with_preview(r).layer(axum::Extension(cfg)))` satisfies `F: FnOnce + Send` of `run_server_with`; the handler extracts `Extension<Arc<ReExecuteConfig>>`; the static `CACHE_DIR` and `set_cache_dir` are gone with no other reader. The watcher callback is an `Fn` closure, so it must clone the `Arc` inside. |
| RIU-4: the guard | Compiles in `claim_and_spawn` with a new `ClaimOutcome::Refused`, which both callers (the handler and `trigger_auto_re_execute`) must match. |
| RIU-4: the clap option | `value_delimiter = ','` with a per-entry `value_parser` and `ArgAction::Append` yields a flat `Vec<RecordSource>`. Test `spike_records_option_parses` passes: `--records cache,./a/b,positron --records engine`, a Windows drive path `C:\notes`, and `postiron` rejected. |
| Other crates | A grep of `crates/wasm-*`, `ts-packages` and `hub-client/src` found no use of any changed function. |

## What changed in the plans

Rewrites made from these results are listed in the commit message and recorded in the plans themselves.
They fall in three groups. **Applied from a determinate result:** decision 14's condition is resolved by
spike 1 (the cell Div is stamped, atomic, as planned); the shape table replaces the untested-shapes
bullet in RIU-3 Phase E; RIU-5's exit class 13 is stated precisely; RIU-3's `scan_and_fill_ids`
visibility sentence is removed. **Changed by a1, for Gordon to veto:** RIU-2's publish wording (no
`set_capture`, a `PublishError`, the `async-trait` dependency), and carrying the gate in a
`ReExecuteConfig` that RIU-2 introduces and RIU-4 extends, instead of a gate argument threaded through
the hooks. **Left as it was, for Gordon:** see below.

## Decisions for Gordon

- **Keep the `Header` exemption?** Spike 2 stamped `Header` in two fixtures with no render difference.
  Decision 14 exempts it as a precaution; dropping it makes the stamp uniform (every non-code node), and
  keeping it leaves stored-markdown headings unmarked. The plans keep the exemption until you decide.
- **Mis-paired expansions.** An expanded cell shifts the block count and reconcile pairs the author's
  next Div with a spliced `.cell` Div, which then carries author bytes unstamped. The stamp cannot fix
  it (it is reconcile's pairing). Options: leave it and record it as a known limit; or have the splice
  stamp such a Div by the unit span even when its source is `Orig(f0)`; or pair by something sturdier
  than index. This is a design choice, so no plan was changed.
- **The table-cell `Plain`.** An inline value alone in an author's table cell stamps the cell's `Plain`.
  Accept it (the cell holds only output), or stamp the `Str` only.
- **Copy-button scaffold nodes carry the slot range, not a stamp.** Accept (they sit under a stamped
  cell Div), or stamp them in the code-block transform. No plan changed.
