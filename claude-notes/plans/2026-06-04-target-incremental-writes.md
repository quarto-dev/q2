# Target Incremental Writes — Development Plan

**Date:** 2026-06-04 (rewritten from the research-plan version)
**Branch:** feature/provenance
**Status:** Implementation-ready development plan.
**Supersedes:** the original *research* version of this file (the
`preimage_in`-text-splice model). That model is abandoned — see
"Why this replaces the text-splice model" below.
**Follows:** `2026-06-04-incremental-writer-unwind.md` (complete; restored
State-A `incremental_write` + `compute_reconciliation`, removed Plan-7
soft-drop / `stampUserEdits` / baseline-AST tracking).

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
        N_u  = lookup(A_u, sourceInfo(N))     // value-equality + tiebreak
                                              // + preimage_in fallback (Generated)
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
   - `Generated` (e.g. resolved shortcode) → no equal `Original` exists in
     `A_u`; fall back to `preimage_in(node)` → token byte range → match the
     `A_u` node covering that range. (This is property #2: editing a rendered
     shortcode edits the `{{< >}}` invocation.)
   - synthetic / no preimage / non-contiguous `Concat` → not mappable →
     not editable. Surface as read-only.

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

## What already exists (post-provenance, post-unwind)

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
- [ ] Add a small QMD corpus under the reconcile/incremental tests covering:
  a single paragraph, a heading, a paragraph adjacent to a fenced div, a
  document with a resolved shortcode, and a document with **duplicate blocks**
  (two identical paragraphs) — the last guards the structural-minimality claim.
- [ ] Helper to: parse `content` → untransformed AST; pick a node; build a
  pure replacement subtree via `parse_qmd_content`.

### Phase 1 — Backend: dual-tree render
- [ ] **Test:** a render entry point returns *both* the transformed AST and the
  untransformed AST (the `qmd_to_pandoc` output captured **before**
  `AstTransformsStage`), each with its own pool; assert an unchanged paragraph
  has byte-identical `source_info` values in both.
- [ ] Capture the untransformed AST at the parse boundary and thread it to the
  render response. Define the boundary explicitly as **immediately after
  `qmd_to_pandoc`, before any transform** (the same tree `incremental_write`
  reconciles against).
- [ ] Extend the render response shape (and TS types) with `untransformedAst`.

### Phase 2 — Backend: destination-node lookup
- [ ] **Test:** `lookup(A_u, source_info)` returns the corresponding node for
  (a) a clean `Original` paragraph (exact value match), (b) a `Generated`
  shortcode node (via `preimage_in` fallback to the token), (c) returns
  `None` for synthetic/no-preimage, (d) disambiguates when a value resolves to
  multiple candidates (tiebreak on node kind + tree depth).
- [ ] Implement the lookup: a `source_info`-value-keyed traversal of `A_u`,
  with the `preimage_in` range fallback for `Generated` and a documented
  tiebreak rule. Live in `quarto-ast-reconcile` or `pampa` (whichever keeps the
  dependency direction clean — `pampa` consumes reconcile).

### Phase 3 — Backend: `apply_node_edit` entry point
- [ ] **Test (end-to-end, Rust):** `apply_node_edit(content, untransformed_ast,
  destination_source_info, modified_subtree)` →
  - edited paragraph: only that block's text changes; all other bytes verbatim;
  - duplicate-block fixture: editing one leaves the twin untouched (structural
    minimality);
  - shortcode fixture: the `{{< >}}` invocation is replaced, not the resolved
    content;
  - assert `read(result_qmd)` reflects the splice and unchanged regions are
    byte-identical to `content`.
- [ ] Implement: deserialize `A_u`; `lookup`; `splice` (replace `N_u` with the
  parsed subtree — may be 1→N blocks; reconcile handles that);
  `compute_reconciliation(A_u, A_u')`; `incremental_write(content, A_u, A_u',
  plan)`; return QMD via the `AstResponse` shape. Reuses the core verbatim.
- [ ] WASM signature (see below); add TS bindings + `.d.ts`.

### Phase 4 — Frontend: round-trip + wiring
- [ ] **Test:** the preview holds `untransformedAst` from the render and passes
  it (with `content`, resolved `source_info`, subtree) into `apply_node_edit`;
  the returned QMD drives `onContentRewrite`.
- [ ] Remove the read-only guard in `ReactPreview.tsx` (currently
  `handleSetAst` early-returns for `pipelineKindForFormat(format) ===
  'preview'`, ReactPreview.tsx:430) and route to `apply_node_edit`.
- [ ] SPA: replace `noopSetAst` (PreviewApp.tsx:355, used at :859) with the
  real handler.
- [ ] Resolve the edited node's `s` → `source_info` **value** before sending
  (independent pools — do not send a bare pool id).

### Phase 5 — Frontend: v1 edit surface (text-bearing blocks)
- [ ] **Test:** editing a paragraph's text and a heading's text produces a pure
  subtree via `parse_qmd_content(newText)` and yields the expected QMD
  end-to-end; inline markdown the user types (e.g. `*emph*`) round-trips
  correctly (because it's parsed, not JS-tokenised).
- [ ] Wire one editable text surface for `Para`/`Header`: on commit, call
  `parse_qmd_content(newText)`, take the resulting block(s) as the modified
  subtree, call `apply_node_edit`.
- [ ] Gate the edit affordance on the **backend editability gate** (node has a
  resolvable `source_info`); render non-mappable nodes read-only.

### Phase 6 — End-to-end verification (per CLAUDE.md)
- [ ] Browser e2e: in a running q2-preview, edit a paragraph; assert the QMD on
  disk changed only at that block and the preview re-rendered. Record the exact
  interaction + observed QMD diff in this plan.
- [ ] `cargo nextest run --workspace`; `cargo xtask verify` (full — WASM leg, as
  `quarto-core`/`pampa` and the wire format are touched).
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

- Predecessor: `2026-06-04-incremental-writer-unwind.md`
- Reconciler: `quarto-ast-reconcile/src/{compute,apply,hash}.rs`
- Writer: `pampa/src/writers/incremental.rs`
- `preimage_in` / `SourceInfo`: `quarto-source-map/src/source_info.rs`
- WASM entry points: `crates/wasm-quarto-hub-client/src/lib.rs`
- Read-only guard / SPA wiring: `hub-client/src/components/render/ReactPreview.tsx`,
  `q2-preview-spa/src/PreviewApp.tsx`
