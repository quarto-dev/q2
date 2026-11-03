---
title: 'RIU-0b spikes: findings (spike J, Julia; spike P, node-level provenance)'
date: 2026-11-03
description: 'Per-spike results for the two RIU-0b spikes: what a Julia cell is in a q2 record and in Positron, and whether spliced record output can be stamped through the SourceInfo system. Each table lists what was run, what was only read, and what stays unverified.'
status: done  # both spikes run 2026-11-03; the Positron window steps were reported by Gordon, not driven by an agent
---

**Plan:** [RIU-0b](../plans/2026-11-03-riu-0b-julia-and-provenance-spikes.md)
**Epic:** [records-in-use](../plans/2026-11-02-records-in-use-epic.md)
**Earlier findings:** [RIU-0](2026-11-03-riu-0-spikes-findings.md)

## Branches (scratch, never merged)

| Branch | Commit | Holds |
|---|---|---|
| `spike/riu-s8-julia` (q2) | `2fbd63449` | the S7 exporter ported with its keep rule and the import gate widened to the `julia` engine; example `riu_s8_check` |
| `spike/riu-s9-provenance` (q2) | `4c8fbb8e0` | `quarto-ast-reconcile/src/stamp.rs` (a walker), `merge_markdown_into_ast_with_slot`, the stamp in `record_splice.rs`, 12 tests plus a debug dump |
| `spike/q2-s8-julia` (Positron, `/Users/gordon/src/positron-spike-s3`) | `afd8c75204` (no new commit) | nothing: the S5 push command was used as it is |

Build: `target/debug/q2` (`q2 (quarto 2) 0.39.0`, dev profile) on `feature/records-in-use` at `eb9903d95`.
The `q2` on PATH was never used. Jupyter came from a `uv venv` in the session scratchpad
(`jupyter-core ipykernel matplotlib pandas nbformat nbclient`). Julia 1.12.7 (juliaup).

## Spike J: Julia

### What was run

| # | Step | Result |
|---|---|---|
| J1 | `q2 render` of a Julia document (`engine: julia`, `execute: {record: true}`), five units: a value, a `println` plus a vector, a `@printf` plus a named tuple, a caught error, an inline span | Rendered. Record read with `jq`: one engine entry, `name: "julia"`, `owned_languages: ["julia"]`, `can_freeze: true`, hash definition `id: "julia"`, granularity `document`. Every cell has `quarto_engine: "julia"`, `quarto_kernel: "julia"`, `quarto_lang: "julia"`. |
| J2 | Same document with `jupyter: julia-1.11`, then with `engine: jupyter` plus `jupyter: julia-1.11` (venv on PATH, IJulia added to a scratch project) | **Same record both times**: engine `julia`, owner `julia`. The `julia-1.11` kernelspec is never used. A `{julia}` cell goes to the `julia` engine whichever way the document asks for Jupyter. |
| J3 | Read `engine/resolution.rs` (not run beyond J2) | A julia extension engine makes a Primary claim on `julia`; only when that extension is absent does Jupyter own it, through the T4 fallback (`resolution.rs:1506-1553`, tests). In the real binary the extension is built in. |
| J4 | S7 export of the J1 record, keep rule unchanged (`jupyter` only) | `exported=0`; every unit skipped (four "not jupyter-owned", one "inline unit"). |
| J5 | Output shapes, from a second document (Julia project with Plots added offline) | `HTML(...)`: `text/html` and `text/plain`. `md"..."`: `text/html`, `text/latex`, `text/markdown`, `text/plain`. A Plots plot: `application/pdf`, `image/png`, `image/svg+xml`, `text/html` (a data-URI `img`), and **no** `text/plain`. Errors: `output_type: error`, `ename: ErrorException`, the traceback an **array of strings with ANSI**. Every `data` value was a **string**; no array-valued or JSON-valued data appeared. `display(MIME("text/html"), "...")` left the cell with **zero outputs** (an engine behavior, not investigated). |
| J6 | Scratch patch: keep rule admits `jupyter` or `julia`; export J1 and the plot document, import each export with `import_positron_cache`, ask `outputs_for` per cell | Export: 4 exported, only the inline span skipped. Import with the **original** gate: owner `"julia"` gets `None` for every cell; owner `"jupyter"` gets the outputs. With the gate widened to `jupyter` or `julia`: both owners get `Some` for every cell, output counts equal the record's. |
| J7 | Positron dev build, S5 push command, three-cell Julia document (value; `println` plus vector; HTML value), export file with 3 cells | **Reported by Gordon, not observed by an agent:** 3 matched; all three cells showed their outputs; outputs persisted across close and reopen. |
| J8 | Same, with a plot cell added after the hello cell (export has 4 cells, the plot cell about 75 KB) | **Reported by Gordon:** 4 matched; the plot cell showed **two plots**, one with a bolder font, **and the base64 text** of at least one representation. |

### Diagnosis of J8 (read from the Positron code, not run)

`quartoOutputViewZone.ts` renders **every** item of a cell output (`:2366-2384`) and drops only
`text/plain` when HTML or an image is present. `_renderOutputItem` renders `image/*` and
`text/html`, and falls back to **text for any string it has no renderer for** (`:2419`). The Julia
engine puts four representations of one plot in a single bundle, so Positron draws the HTML item (a
raster `img`), the SVG item (vector, so a different font weight) and the PNG, and prints the
`application/pdf` base64 as text. Python never showed this: matplotlib sends one PNG plus
`text/plain`. That the extra plot is the SVG and the text is the PDF is inferred, not confirmed.
The exporter therefore needs a representation-selection rule, not only the S7 filter (RIU-5).

### Answers

1. **Owner and engine.** Julia cells are owned by the `julia` engine (q2's Julia engine, a
   QuartoNotebookRunner server), not Jupyter, whatever the document says. (J1, J2)
2. **RIU-5 keep rule.** As written it skips every Julia cell (J4). It must admit the `julia` engine
   for plain-language cells, and needs the representation rule from J8.
3. **Outputs.** Strings only; multiple representations per bundle (J5). The array and JSON classes
   S7 left untested are still untested: Julia produced neither.
4. **Positron push.** Works for Julia text, vector and HTML outputs, persists (J7). A multi-MIME plot
   shows duplicates and base64 text (J8). Both Gordon-reported.
5. **Positron to record.** A Julia cell's owner is `"julia"` (the J1 record's `owned_languages` and cell `quarto_engine`; the J6 harness passed it as a literal, not through the registry's resolution); the import
   gate accepts only `"jupyter"` (`positron_import.rs:147`) and must accept `julia` too (J6).

### Go or no-go

**Go for Julia in RIU-5 and RIU-6**, with two changes: the keep rule and the import gate admit the
`julia` engine, and the exporter selects one image representation (drop `application/pdf`, and
drop `text/html` when an image is present, and keep one of the images). No decision 12 conflict.

### Not tested

- Positron **producing** a Julia cache itself (no Julia kernel was run in Positron), and so Positron
  hashing a Julia cell with option lines (`#| error: true`): the J7 and J8 documents had none.
- Which image representation Positron displays best (SVG against PNG), and the fixed rule's effect
  in the window. The rule is designed, not run.
- `display(MIME(...), ...)` outputs; Julia widgets; a Julia document with the julia extension
  absent (Jupyter would own it then).
- Windows and Linux.

## Spike P: node-level provenance for spliced output

### What was run

All steps are on `spike/riu-s9-provenance` (`4c8fbb8e0`), in `record_splice.rs` tests unless noted.

| # | Question | Step | Result |
|---|---|---|---|
| P1 | Q1: identify spliced nodes after the reparse | Compute each unit's presented-markdown range in the final (unmasked) text; give `merge_markdown_into_ast` a sibling that also returns the `FileId` slot the reparse is registered under; walk every node source info with a copy of `remap.rs` (`quarto-ast-reconcile/src/stamp.rs`); stamp a node when it resolves into that slot and its range lies inside a unit span | Works. Reconcile **keeps the document's own nodes** (they carry the document file id) and takes new content from the reparse (new slot), so the file id separates author nodes from spliced ones for free; the range then says which unit. Nothing needs to ride through the reparse. |
| P2 | Q2: constructor, shape, atomicity | Shape test `p_shape_of_a_stamp`: `Generated`, `from` empty, `data.source` and `data.unit` present. Two variants: a new `record-output` kind built with a struct literal, and the kind that **already exists** in `quarto-source-map` 0.3.1, `By::jupyter_output()` | `By::is_atomic_kind()` is false for the new kind and **true for `jupyter-output`**. See "What the catalog now holds" below. |
| P3 | Q3: wire format | `p_the_wire_format_carries_and_reads_back_the_stamp`: write the spliced AST with the preview JSON config, read it back with the **strict** reader (rejects a node with no `s:`), count stamps | Passes; kind and source are in the pool; the count survives the round trip. |
| P4 | Q3: prose edit through the writer | `p_prose_edit_next_to_a_stamped_output_round_trips`: the real flow, `apply_node_edit` on the **untransformed** source AST with the destination taken from the prose paragraph in the **spliced** AST | Result equals the source with only that paragraph changed; the cell fences and the other prose are byte-identical. `p_prose_after_a_stamped_cell_still_resolves_to_the_authors_bytes`: prose before and after a stamped cell keeps its original range in the spliced AST. |
| P5 | Q3: tiling | `p_tiling_auditor_on_the_untransformed_and_the_stamped_ast`: the auditor on the untransformed parse of the same text, and on the stamped AST against the final text | Untransformed: no findings. Stamped: 9 `GeneratedNoInvocation` (a census tally, the same finding any `from: []` kind gives) and **no** `SiblingOverlap`, `ContainmentViolation` or `ScatteredConcat`, which are the three kinds the CI gate asserts. The writer never receives the spliced AST, so the tiling precondition is not at stake. |
| P6 | Q3: idempotence | `p_stamping_changes_no_structural_hash`: two splices, and a splice with `positron` against one with `engine`, compared with `compute_blocks_hash_fresh` | Equal hashes (source info is excluded from the hash by design); the stamps themselves differ. `crates/quarto-core/tests/integration/idempotence` ran in the integration suite and passed; it has no engine cells, so it cannot exercise a record. |
| P7 | Q4: which units | `p_every_matched_cell_is_stamped_and_prose_is_not`, `p_a_stale_cell_is_not_stamped`, `p_spans_stamp_the_value_only`, `p_mixed_document_stamps_only_what_the_record_supplied`, `p_no_edit_means_no_stamp`, `p_a_q2_record_is_stamped_with_its_own_source` | Matched cells: stamped (the cell Div, the output Div, the non-code blocks inside). Stale cells and unmatched units: not stamped (never edited by the splice). Inline spans: only the value `Str`. Mixed document (a Positron record covering Python, an `{r}` chunk it does not cover): only the Python unit stamped; the R chunk stays a source cell. A q2 record is stamped with `source: "engine"`. |
| P8 | Full suites on the spike branch | `cargo nextest run -p quarto-core -p quarto-preview` | 7606 passed, 37 skipped. |

### Two failures that shaped the design

- **A stamped `CodeBlock` broke an existing test.** With every node stamped,
  `record_preview_splice::record_splice_matches_legacy_splice_on_the_jupyter_fixtures` failed: the
  record page lost the `code-copy-outer-scaffold` wrapper and copy button on output blocks.
  `CodeBlockDecorationKey::from_source_info` returns a key only for an `Original`
  (`code_block_generate.rs:168-181`), and the copy-button and filename decoration are keyed on it.
  A `CodeBlock` must keep its `Original`. With `CodeBlock` exempt the parity test and the full suites
  pass. Other transforms that key on an `Original` range were not audited beyond what the suites
  exercise.
- **A value merges with its neighbors in a `Str`.** The reparse of `A v0 and v1.` makes `Str("v1.")`,
  whose range runs one byte past the unit's span, so a containment rule missed the second span.
  A `Str` that overlaps a span at all is stamped (it contains output bytes); every other node needs
  containment.

### What the catalog now holds (the contract is out of date)

- `By::jupyter_output()` exists in `quarto-source-map` 0.3.1 (kind `"jupyter-output"`, atomic,
  documented as content synthesized from execution-engine output). `By::citeproc()` and
  `By::callout()` exist too. The contract's catalog (`provenance-contract.md` section 2) lists none
  of them, and nothing in this tree calls `jupyter_output()`.
- `quarto-source-map` is an **external crate** (crates.io 0.3.1, repo `posit-dev/quarto-source-map`).
  The contract's step "add a constructor to `crates/quarto-source-map/src/source_info.rs`" cannot be
  done in this tree; a new first-party constructor means a release of that crate. `By` has public
  fields, so a kind can be built in `quarto-core` (as `enrich_or_create` does), but then it is not in
  `is_atomic_kind` (which matches builtin kinds only).
- The TypeScript `ATOMIC_KINDS` set (`ts-packages/preview-renderer/src/utils/sourceInfo.ts:54`)
  holds only `filter`, `shortcode`, `title-block`, `tree-sitter-postprocess`. It has **neither
  `citeproc` nor `jupyter-output`**, so it already disagrees with the Rust function. The viewer
  turns an atomic node's `setLocalAst` into a no-op (`dispatch.tsx:415-419`).

### Answers

1. **Identify spliced nodes: yes**, by file id and range, with a post-walk after the merge. The splice
   must return the slot (one new function) and compute each unit's span through the unmask.
   Nothing has to be carried through the reparse.
2. **Kind and atomicity.** Reuse the upstream `jupyter-output` kind, atomic, with
   `data = {"source": ..., "unit": ...}`. The reason for atomic: output is regenerated or imported,
   never edited in place (the contract's section 7 rule). The alternative, a new `record-output` kind,
   is non-atomic unless `quarto-source-map` is released. The TypeScript set must gain
   `jupyter-output` for the viewer to honor it. Gordon decides (listed in the epic).
3. **Wire format, writer, tiling, idempotence: all hold** (P3 to P6).
4. **Units:** matched cells and span values; not stale cells, not unmatched cells, not prose, **not
   `CodeBlock` nodes** (P7 and the first failure).
5. **Record-level `quarto_provenance`.** Engine provenance (`quarto_engines[i].provenance`, the owned
   languages) is per engine run, and a Positron record has no engine entry. Mapping the marker onto
   it would need an invented entry, which would then claim languages and break the content-hash-only
   match S4 proved. The marker stays, at record and cell level. A future freeze reader sees
   `quarto_engines: []` (nothing can freeze) and `quarto_provenance: "positron"` (why). The splice
   reads the record-level marker to choose `data.source`; a record without it is `"engine"`.

### Not tested

- Any viewer reading the stamp (out of scope), and the atomic gate in a browser.
- Other transforms keyed on an `Original` range (only the suites above exercise them).
- Stamping through `render_qmd_to_preview_ast` itself: the tests call `splice_record` and the JSON
  writer with the same configuration instead. The preview entry point was read, not run.
- Spliced output from a **legacy** capture (the legacy splice is not stamped; EXR-17 deletes it).
- An inline value that merges with prose in a way other than `Str` (a `Code` or `Math` span).
- The unmask mapping for a span inside a document with masked display blocks beyond
  `a_nested_fence_in_a_display_block_is_not_a_unit`.

## Register

| # | Unknown | Answer |
|---|---|---|
| U21 | Which engine owns a Julia cell, and does it use the kernelspec | The `julia` engine; no kernel. (J1, J2) |
| U22 | Does RIU-5 keep or skip Julia | Skips all of it as written; must admit `julia` (J4, J6). |
| U23 | What Julia outputs look like | Strings; several MIME types per bundle; no array or JSON values (J5). |
| U24 | Does Positron show an exported Julia record | Yes for text, vector and HTML, persisted (Gordon, J7); a multi-MIME plot shows duplicates and base64 text (Gordon, J8). |
| U25 | Owner for `record_from_positron` and Julia | `"julia"`, read from the J1 record (J6 passed it as a literal; RIU-3 tests it through the real registry); the gate must accept it. |
| U26 | Can spliced nodes be identified | Yes (P1). |
| U27 | Constructor, shape, atomicity | Existing atomic `jupyter-output`; a new kind needs an upstream release (P2). |
| U28 | Wire, writer, tiling, idempotence | All hold (P3 to P6). |
| U29 | Record-level provenance | Stays; does not map onto engine provenance (Answer 5). |

## Not run

- Nothing in RIU-1 to RIU-6 was implemented.
- The Positron steps J7 and J8 were done by Gordon; the agent did not drive the window.
- No push, no SSH, nothing under `/Users/gordon/src/positron`.
