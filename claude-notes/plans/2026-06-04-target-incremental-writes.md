# Target Incremental Writes — Development Plan

**Date:** 2026-06-04 (rewritten from the research-plan version)
**Branch:** feature/provenance
**Status:** Implementation-ready development plan.
**Supersedes:** the original *research* version of this file (the
`preimage_in`-text-splice model). That model is abandoned — see
"Why this replaces the text-splice model" below.
**Baseline:** the earlier Plan-7 write-back model (soft-drop reconciliation,
`stampUserEdits`, baseline-AST tracking in the WASM bridge) was reverted before
this plan began. The writer core is back to State A: `incremental_write` +
`compute_reconciliation` with no soft-drop and no provenance-stamping of edits.
That revert is history (git); this plan builds on the State-A baseline.

---

## Overview

Lift q2-preview's read-only guard and make rendered nodes editable, by
treating a user edit as a **modified pure (untransformed) AST subtree**
for a single node and writing it back through the **existing**
reconcile + incremental-write core — with no soft-drop, no provenance
stamping of edits, and no transformed-vs-source impedance mismatch.

The edit round-trips entirely through pre-pipeline ASTs:

1. The frontend identifies the edited node by its **`source_info`** (carried
   on every rendered node) and supplies a **pure** replacement subtree.
2. The backend holds the **untransformed AST** (round-tripped from the
   render — not re-parsed), finds the destination node in it by
   `source_info` **value**, splices the pure subtree in, runs
   `compute_reconciliation` against the original untransformed AST, and
   `incremental_write` produces the new QMD.
3. The new QMD flows back through the existing content→render reactivity,
   which re-renders and hands the frontend fresh transformed + untransformed
   trees for the next edit.

### Three properties this buys

1. **Both reconcile inputs are pre-pipeline ASTs**, so there is no
   transformed-vs-source mismatch — the failure mode the read-only guard
   exists to prevent (ReactPreview.tsx:420 "the post-pipeline AST diverges
   from source enough that a naive `incrementalWriteQmd` would corrupt the
   qmd"). No soft-drop, no `Generated{by: user_edit}`, no `stampUserEdits`.
2. **`new_ast` is the untransformed AST with exactly one node replaced**, so
   `compute_reconciliation` yields `KeepBefore` for every other node — "no
   extra changes" is a **structural guarantee**, not a heuristic. The
   duplicate-block / text-reflow hazards of a splice-and-reparse approach
   don't arise, because we replace a tree node, not bytes.
3. **One write-back algorithm.** The core stays `compute_reconciliation`
   (`quarto-ast-reconcile/src/compute.rs:30`) + `incremental_write`
   (`pampa/src/writers/incremental.rs:81`), unchanged. `apply_node_edit` is
   a new entry point that *constructs* `new_ast` backend-side and delegates
   to that core. No second write-back path; the demos
   (`q2-demos/*/useSyncedAst.ts`) already prove the core works on pure AST.

### Why this replaces the text-splice model

The original research plan proposed: `preimage_in(node) → byte range → splice
raw text into the source QMD → re-parse → reconcile`. We abandoned it for the
**AST-splice** model above because:

- The edit contract settled on "setAst hands back a modified **AST**" (matching
  the existing `setLocalAst`/`setAst` infrastructure and the demos), not raw
  text.
- Splicing into the untransformed AST and reconciling against the *same* tree
  makes minimality structural (property 2). Splicing text + reparsing risks
  re-flow and duplicate-block cross-matching.
- `preimage_in` is therefore **not** the frontend-facing edit-position bridge.
  It retreats to (a) its existing internal role in `incremental_write`'s
  `InlineSplice` boundary math, and (b) a fallback in the node-lookup for
  `Generated` nodes (below). The frontend never calls it.

---

## Architecture & data flow

```
RENDER (per content change)
  content ──parse──▶ untransformed AST ──pipeline──▶ transformed AST
                          │                                │
                          └──────────── both serialized ───┘
                                         │
                                         ▼
                   frontend holds { transformedAst, untransformedAst }
                   (renders transformedAst; retains untransformedAst)

EDIT (user edits node N in the rendered view)
  1. frontend: newText ──parse_qmd_content──▶ pure subtree S
  2. frontend: setAst → apply_node_edit(
        content,
        untransformedAst,
        sourceInfo(N),          // resolved VALUE, not pool id
        S)
  3. backend:
        A_u  = deserialize(untransformedAst)
        N_u  = lookup(A_u, sourceInfo(N))     // value-equality only (Plan 2b:
                                              // preimage_in fallback retired)
        A_u' = splice(A_u, N_u → S)
        plan = compute_reconciliation(A_u, A_u')   // KeepBefore everywhere but N_u
        qmd  = incremental_write(content, A_u, A_u', plan)
        return qmd
  4. frontend: onContentRewrite(qmd) → content updates → RENDER fires again
```

`apply_node_edit` returns **QMD only**. The preview refresh and the
production of the next render's `untransformedAst` are handled by the
existing content→render loop (Phase 1 makes that render dual-tree). This
deliberately decouples write from render and reuses existing reactivity.

### Retention decision (round-trip to frontend)

The untransformed AST is **retained by round-tripping it to the frontend**,
not re-parsed at edit time. Rationale:

- `source_info` is only a valid cross-tree key within a single parse lineage.
  A re-parse mints a fresh `SourceInfoSerializer` pool (json.rs:308 — interning
  allocates a fresh entry per call; it does **not** dedupe by value), so its
  ids/values are not guaranteed to correspond to the transformed tree the user
  is editing. Round-tripping the render's *own* untransformed tree makes the
  correspondence exact by construction.
- It pins a **render generation**: an edit is only valid against the
  untransformed tree it was rendered from. If `content` moved underneath
  (future collaborative case), re-render before editing rather than splicing
  against a stale tree.

We match by `source_info` **value**, not pool id — so **no shared-pool
serializer change is required**. (A shared-pool / id-equality optimization,
giving O(1) lookup, is possible later but is *not* a prerequisite; it would
require the serializer to dedupe by value.)

Cost: the render response carries two full ASTs instead of one. Accepted for
v1; revisit if payload size bites large documents.

---

## Editability — two independent gates

A rendered node is editable in v1 iff **both** hold. Neither is "node type."

1. **Backend gate — is the node source-mappable?**
   - clean `Original`/`Substring` `source_info` → exact value match in `A_u`.
   - `Generated` (e.g. resolved shortcode) → `decode_compact_source_info`
     rejects non-`t=0` SourceInfo before `lookup_block` is called, so
     `Generated` targets never reach the lookup. Property #2 (editing a
     rendered shortcode edits the invocation via `preimage_in`) is retired
     in Plan 2b — a miss now returns the original content unchanged (no-op).
   - synthetic / non-contiguous `Concat` → rejected by type check →
     no-op. Surface as read-only.

2. **Frontend gate — can we produce a pure subtree for the edit?**
   A difficulty gradient, the only place type matters:
   - paragraph / heading text edit → `parse_qmd_content(newText)` → pure
     block(s). **v1 scope.**
   - list / table / structural edits → harder; deferred.
   - editing *inside* a resolved callout/shortcode → the rendered form isn't
     the pure form; deferred ("construct from scratch").

The backend (`apply_node_edit`) is **node-type-agnostic**; v1 product scope =
however many frontend pure-subtree producers we build (gate 2). v1 ships
text-bearing blocks (paragraph/heading). More types are **frontend-only,
additive, zero backend change**.

---

## What already exists (post-provenance, State-A writer)

- `SourceInfo` with `Generated { by, from }`, `AnchorRole`, `preimage_in`
  (`quarto-source-map/src/source_info.rs:435`), `is_atomic_kind` (:781).
- `ATOMIC_CUSTOM_NODES` / `is_atomic_custom_node`
  (`quarto-pandoc-types/src/atomic_custom_nodes.rs`).
- `compute_reconciliation` (compute.rs:30) — source-info-blind hash/structural
  matching; `apply_reconciliation` (apply.rs:24) — zero-copy merge of
  unchanged subtrees.
- State-A `incremental_write(original_qmd, original_ast, new_ast, plan)`
  (incremental.rs:81); `incremental_write_qmd` 2-arg (lib.rs:2789).
- `parse_qmd_content` = `qmd_to_pandoc`, pure pre-pipeline (lib.rs:2647).
- `render_qmd_content` (lib.rs:993), `render_page_for_preview` (lib.rs:1166),
  with `ReplayEngine` available in the WASM preview path (lib.rs:1159).
- `setLocalAst` / `setAst` framework infra (`preview-renderer/src/framework/`).
- source-info pool wire format (`s` id per node, pool `p`).

---

## Work — phases (TDD: tests before implementation)

### Phase 0 — Fixtures & shared test helpers
- [x] Add a small QMD corpus under the reconcile/incremental tests covering:
  a single paragraph, a heading, a paragraph adjacent to a fenced div, a
  document with a resolved shortcode, and a document with **duplicate blocks**
  (two identical paragraphs) — the last guards the structural-minimality claim.
  Lives in `crates/pampa/tests/integration/node_edit_tests.rs`.
- [x] Helper to: parse `content` → untransformed AST; pick a node; build a
  pure replacement subtree via `parse_qmd_content`.

### Phase 1 — Backend: dual-tree render
- [x] **Test:** a render entry point returns *both* the transformed AST and the
  untransformed AST (the `qmd_to_pandoc` output captured **before**
  `AstTransformsStage`), each with its own pool; assert an unchanged paragraph
  has byte-identical `source_info` values in both.
  (`pipeline::tests::render_qmd_to_preview_ast_returns_dual_ast`)
- [x] Capture the untransformed AST at the parse boundary and thread it to the
  render response. Implemented as `capture_untransformed_ast_json` in
  `quarto-core/src/pipeline.rs`, called at the start of
  `render_qmd_to_preview_ast` before the main pipeline runs.
- [x] Extend the render response shape (and TS types) with `untransformed_ast_json`.
  Added to `PreviewAstOutput`, `RenderResponse` (Rust), and
  `RenderResponse` interface in `ts-packages/preview-renderer/src/types/diagnostic.ts`.

### Phase 2 — Backend: destination-node lookup
- [x] **Test:** `lookup(A_u, source_info)` returns the corresponding node for
  (a) a clean `Original` paragraph (exact value match), (b) ~~a `Generated`
  shortcode node (via `preimage_in` fallback to the token)~~ — property #2
  retired in Plan 2b; test `lookup_finds_block_via_generated_preimage_fallback`
  deleted, (c) returns `None` for synthetic/no-preimage, (d) disambiguates when
  a value resolves to multiple candidates (tiebreak on first occurrence).
  (4 tests in `node_edit_tests::lookup_*`)
- [x] Implement the lookup: exact `source_info`-value match only. Lives in
  `crates/pampa/src/node_lookup.rs` as `lookup_block`. Plan 2b removed the
  `preimage_in` covering fallback — a miss is now a no-op at the `apply_node_edit`
  level. v1 scope: top-level blocks only; tiebreak = first (smallest-index).

### Phase 3 — Backend: `apply_node_edit` entry point
- [x] **Test (end-to-end, Rust):** `apply_node_edit(content, untransformed_ast,
  destination_source_info, modified_subtree)` —
  5 tests: single-para edit, heading edit, duplicate-block minimality (edit
  block 0, edit block 1), synthetic-target/include-target → no-op + return
  original (Plan 2b: `DestinationNotFound` removed; `ApplyNodeEditError` no
  longer has that variant). Lives in `node_edit_tests::apply_node_edit_*`.
- [x] Implement: `crates/pampa/src/apply_node_edit.rs` — deserialize A_u;
  lookup_block; on miss: `eprintln!` + `return Ok(content.to_string())`; on
  hit: splice; compute_reconciliation; incremental_write; returns `Ok(new_qmd)`
  or `Err(ApplyNodeEditError)` (deserialization / write errors only).
- [x] WASM entry point in `wasm-quarto-hub-client/src/lib.rs`; TS declaration
  added to `hub-client/src/types/wasm-quarto-hub-client.d.ts`.

### Phase 4 — Frontend: round-trip + wiring
- [x] **Test:** `hub-client/src/services/applyNodeEdit.wasm.test.ts` — 3 WASM
  tests covering the render→untransformedAst→apply_node_edit round-trip.
  **Require `npm run build:wasm` before they pass** (WASM binary predates
  Phase 1–3 Rust changes; tests correctly fail on the stale binary).
- [x] Removed the read-only guard in `ReactPreview.tsx` (`handleSetAst` now
  routes `PreviewNodeEditPayload` through `applyNodeEdit` for preview format).
  `untransformedAst` state captured from render results.
- [x] SPA (`PreviewApp.tsx`): `noopSetAst` replaced by `handleSetAst` that
  reads content from VFS, calls `applyNodeEdit`, writes new QMD back to VFS,
  bumps `contentTick`. Full Automerge write-back deferred to Phase 5.
- [x] `PreviewNodeEditPayload` type defined in shared
  `ts-packages/preview-renderer/src/types/diagnostic.ts`.
- [x] `applyNodeEdit` wrapper added to `preview-runtime/src/wasmRenderer.ts`;
  `apply_node_edit` declared in `WasmModuleExtended` interface.
- [x] `untransformed_ast_json` field captured from render results and stored in
  `ReactPreview` state and `PreviewAppState`.

### Phase 5 — Frontend: v1 edit surface (text-bearing blocks)
- [x] **Test (Rust):** 2 tests in `node_edit_tests` — inline markdown
  round-trip (`*emph*` → parse → write preserves stars) and heading edit
  leaves adjacent paragraph verbatim.
- [x] Wire `Para`/`Header` editable surfaces: `contentEditable` on click;
  `onBlur`/Enter commits; `commitEdit(poolId, newText)` resolves the
  source_info from the pool and sends `PreviewNodeEditPayload` via `setAst`.
- [x] `PreviewContext` extended with `pool` and `commitEdit` (provided by
  `entry.tsx`'s `PreviewRoot`).
- [x] Backend editability gate: blocks with no `s` (pool id) render read-only
  (the `isEditable` guard in Para.tsx / Header.tsx).
- [x] `parseQmdContentSync` wrapper added to `wasmRenderer.ts`; parent frame
  calls it before `applyNodeEdit` — no WASM in iframe.
- [x] `PreviewNodeEditPayload` updated to use `newText` (raw text) instead of
  pre-parsed `modifiedSubtreeJson`; parent does all parsing.

### Phase 6 — End-to-end verification (per CLAUDE.md)
- [x] Browser e2e: manually confirmed in hub-client dev server
  (`npm run build:wasm && npm run dev:fresh`). Editing a paragraph and a heading
  each produced the correct QMD change in the Automerge document and the preview
  re-rendered with the new text. Three integration bugs surfaced and fixed:
    - pool at `astContext.p` not `raw.p` (was always empty object)
    - project render path hardcoded `untransformed_ast_json: None`
    - compact source_info format `{"t","r","d"}` not accepted by `apply_node_edit`
- [x] WASM e2e tests: 7 tests in `hub-client/src/services/applyNodeEdit.wasm.test.ts`
  cover all three regressions plus full round-trip (para replace, inline markdown).
  All pass with the current built WASM.
- [ ] `cargo nextest run` on changed crates; `cargo xtask verify` full.
- [ ] hub-client changelog entry (two-commit workflow).

---

## WASM API

```rust
// New. Constructs new_ast backend-side, delegates to the existing core.
pub fn apply_node_edit(
    content: &str,                  // original QMD (source of verbatim bytes)
    untransformed_ast_json: &str,   // the render's own untransformed AST (round-tripped)
    destination_source_info_json: &str, // resolved SourceInfo VALUE of edited node
    modified_subtree_json: &str,    // pure replacement block(s)
) -> String;                        // AstResponse { success, qmd, error, diagnostics }
```

Render entry point gains an `untransformedAst` field in its response (Phase 1).
`incremental_write_qmd` is left intact (demos still use it); `apply_node_edit`
does not replace it — it shares the same core.

---

## Out of scope / deferred (not blocking v1)

- General pure-subtree production for arbitrary node types (v1 = text-bearing
  blocks via `parse_qmd_content`).
- Multi-block **structural** edits (drag-reorder, split/merge across blocks).
- Editing inside resolved CustomNodes / shortcode *bodies*.
- Collaborative concurrency: render-generation pinning / conflict handling when
  `content` drifts between render and edit-commit (v1 assumes single-user).
- Shared-pool / id-equality lookup optimization (value-matching suffices).
- Hardening the "subtree must be pure" invariant with a type/runtime guard
  (v1 knowingly accepts that a wrongly-transformed subtree misbehaves).

## Open questions

- **Tiebreak rule** when a `source_info` value resolves to multiple `A_u`
  nodes (interning dedupes equal values; a block and a contained inline can
  share a range). Proposed: filter by node kind, then nearest enclosing block;
  finalize in Phase 2.
- **1→N block splice**: if `parse_qmd_content(newText)` yields multiple blocks,
  do we accept the expansion (reconcile handles it) or constrain v1 to 1→1?
  Lean: accept; assert behavior in Phase 3 tests.
- **Payload size**: round-tripping two ASTs per render. Measure on a large doc;
  if it bites, reconsider backend retention (module state) vs the shared-pool
  compaction.

## Risks

- **Wire-format growth** (two ASTs) — measured in Phase 1; mitigations above.
- **Lookup ambiguity** for unusual `source_info` shapes (`Concat`,
  nested `Substring`) — covered by Phase 2 tests; `None` (read-only) is the
  safe default.
- **`incremental_write` `InlineSplice` prefix/suffix-verbatim assumption**
  (incremental.rs:565) — edits that change block-level syntax fall back to
  `Rewrite`; ensure Phase 3 covers a heading-level change.

## References

- Reconciler: `quarto-ast-reconcile/src/{compute,apply,hash}.rs`
- Writer: `pampa/src/writers/incremental.rs`
- `preimage_in` / `SourceInfo`: `quarto-source-map/src/source_info.rs`
- WASM entry points: `crates/wasm-quarto-hub-client/src/lib.rs`
- Read-only guard / SPA wiring: `hub-client/src/components/render/ReactPreview.tsx`,
  `q2-preview-spa/src/PreviewApp.tsx`
