# Hash-based tracking of edit locations ("track me") — block-editing successor

**Date:** 2026-06-13
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`)
**Successor to:** `2026-06-11-block-editing-improvements.md` (Phases 1–3). **Not yet the active plan** —
finish that plan's P3.4 ("self-heal on write") first; this layers on top. Do **not** repoint
`CURRENT.md` until P3.4 lands.
**Design master:** `claude-notes/designs/2026-06-06-block-editing-design.md` (this plan amends it — see
§"Impact on related design docs").
**Deep rationale:** the position-independent subtree hash and the "can reconciliation relocate a node"
analysis live in `claude-notes/research/2026-06-13-tree-reconciliation-node-relocation.md` — **currently
on branch `bugfix/hub-sync-fork-panic`, not here.** Cherry-pick or copy it onto this branch when this
plan goes active; its load-bearing facts are restated below so this plan stands alone.

---

## Context — why this plan exists

The Phase-2/3 self-heal keeps an open block editor anchored across an external (collaborator)
re-render by **byte offset + content slice**: `findReanchorCandidate` (`lockedTiles.ts:409-431`) takes
the **single** pool entry nearest `r[0] >= anchorR0`, then content-verifies against the frozen
`anchorSlice`. This works for top-level tiles but **drops a nested child** (documented watch-item,
predecessor plan Risks §"Nested-child self-heal DROP", l.932-948): after a concurrent insert above the
container, the *container's* shifted `r[0]` becomes the "nearest" candidate, fails the
whole-container-≠-child content check, and there is **no scan onward to the child** → the child's
in-flight edit is wrongly dropped though it still exists unchanged.

The predecessor plan named two fixes: **(a)** client-side "scan all candidates `r[0] >= anchorR0`",
and **(b)** "content-addressed relocation via a position-independent AST-subtree hash from q2's
reconciliation pipeline." **This plan implements (b)** — but in its strongest form: relocation moves
**to the parent**, which already has the source, WASM, the reconciler, and the hash. The iframe
postMessages **"track this node"**; the parent computes where it now is on every fresh render and ships
the location back **inside the same `UPDATE_AST`** (same generation as the content it indexes, by
construction). The structural hash never leaves Rust; only a small `{trackId → [r0,r1] | null}` answer
crosses. `null` ⇒ the node is gone ⇒ drop.

This both fixes the nested-child drop and turns relocation into a small **general service** (one active
tracked location now; the data model admits many — presence cursors, comment pins, a move's destination
projection — as future consumers).

## Settled decisions (from design discussion, 2026-06-13)

- **Scope = KEEP-open self-heal only.** Just keep the open editor anchored across an external
  re-render. Commit-rebasing and destination-projection (both reuse the same primitive) are
  **designed-for but out of scope** here.
- **Primitive = structural hash** (`compute_block_hash_fresh`). Cursors evaluated and **deferred** —
  see §"Automerge cursors — also considered."
- **Version handshake = a monotonic `generation` counter** stamped on `UPDATE_AST`, echoed by the
  iframe on TRACK/commit. Host-agnostic (works for the file-based SPA too); simpler than
  `getHeads`.
- **Duplicate tie-break = nearest-to-last-known-position** (the re-anchored `lastPos`, not the frozen
  original), measured in **absolute** distance (both directions), with the **`anchorSlice` content
  check as the final arbiter** (hash recall → slice precision) and a **distance cap** (an implausibly
  far sole match ⇒ DROP, not teleport).
- **Multiple tracked locations allowed in the data model; exactly one used now** (the active editor).

---

## Load-bearing code facts (verified this session; line numbers drift — re-grep)

- **Parent does NOT full-reparse on a local edit.** `handleSetAst` (`ReactPreview.tsx:508-561`) runs
  `applyNodeEdit` on the **cached** AST (`rendered` state, `ReactPreview.tsx:325-329`). A *fresh*
  `untransformedAstJson`+`renderedContent` is produced only on the **`doRender` path**
  (`ReactPreview.tsx:171-282`), which fires when `content` (Automerge text) changes — **the
  collaborator-edit path.** ⇒ track-me relocation hooks the `UPDATE_AST` *assembly*; the nested-drop
  fix flows through `doRender`.
- **No version stamp on `UPDATE_AST`.** Fields (`Q2PreviewIframe.tsx:223-251`): `astJson,
  currentFilePath, assetManifest, projectFilePaths, pendingAnchor, pendingAnchorEpoch, renderedContent,
  untransformedAstJson, currentActor, editingDisabled, unlockNestingCursor, nestedEditBuffers`. We add
  `generation` and `trackedLocations`.
- **`renderedContent` is byte-identical to the Automerge `['text']`** at render time
  (`ReactPreview.tsx:441-445`; SPA snapshots VFS at `PreviewApp.tsx:1023-1024,1060`). So the iframe's
  `r0/r1` index the same string the parent hashes.
- **Self-heal effect** = `useLayoutEffect` on `[astJson, renderedContent, untransformedAstJson]`
  (`PreviewRoot.tsx:244-285`) calling `findReanchorCandidate`. **This is the integration point.**
- **Hosts duplicate the handler:** `handleSetAst` in both `ReactPreview.tsx` and `PreviewApp.tsx`
  (SPA ~no-op for file mode today). UPDATE_AST assembly is per-host ⇒ track-me parent logic must be a
  **shared helper** consumed by both (mirror `computeNestedEditBuffers`).
- **The hash:** `quarto_ast_reconcile::compute_block_hash_fresh` (`crates/quarto-ast-reconcile/src/
  hash.rs:102`) — content-only, **excludes all source location**, per-subtree, deterministic
  `FxHasher`, collision-guarded in reconcile by `structural_eq_block`. Already a dep of
  `wasm-quarto-hub-client` (`Cargo.toml:14`). `apply_node_edit.rs` is the model for "deserialize
  `untransformed_ast_json` → walk → act" + a WASM export (`wasm-quarto-hub-client/src/lib.rs:2938`).
- **Editing/identity uses Original pool entries only** (`t==0,d==0`); a block and its first inline can
  share `r[0]`, so the **full `[r0,r1]`** disambiguates (predecessor plan caveat B).

---

## Implementation phases (TDD — tests precede implementation in each)

> **Process.** Touches hub-client (`ReactPreview`/`ReactRenderer`) + the SPA (`PreviewApp`) ⇒ the
> two-commit `hub-client/changelog.md` workflow. Touches the WASM leg ⇒ **full `cargo xtask verify`**
> (not `--skip-hub-build`) and the `build:wasm → build-q2-preview-spa → build --bin q2` chain before any
> live `q2 preview` check. Fail-on-revert is mandatory on every behavior test (predecessor convention).

### Phase A — Generation handshake (enabling infra; no behavior change)

- [ ] **Test:** the parent stamps a strictly-increasing `generation` on each assembled `UPDATE_AST`;
  a re-render bumps it; the iframe stores it and echoes it on `SET_AST` (commit). (RTL on both hosts +
  iframe.)
- [ ] **Test:** the parent retains the last **2** `{generation → {untransformedAstJson,
  renderedContent}}`; older entries evicted. (Unit on the shared retainer.)
- [ ] Add `generation: number` to the `UPDATE_AST` payload (`Q2PreviewIframe.tsx`) and to
  `PreviewNodeEditPayload` (commit) + the new `TRACK_NODE` message.
- [ ] Both hosts increment a per-session counter when assembling `UPDATE_AST` (`ReactPreview.tsx`,
  `PreviewApp.tsx`). **Counter, not `getHeads`** — uniform across hosts.
- [ ] Iframe stores `currentGeneration` in `PreviewContext`; threads it into `SET_AST` and `TRACK_NODE`.
- [ ] Parent keeps a 2-slot ring of prior `{untransformedAstJson, renderedContent}` keyed by generation
  (in the shared tracker of Phase C).

### Phase B — Rust/WASM relocation primitive (native-testable)

New module `crates/pampa/src/node_tracking.rs` (mirror `apply_node_edit.rs`'s deserialize-and-walk),
reusing `quarto_ast_reconcile::compute_block_hash_fresh`. Rust returns **hash recall + nearest**; the
**slice-verify, distance-cap, and drop policy live in TS** (Phase C), where they're easy to tune.

- [ ] **Test (integration, `crates/pampa/tests/integration/node_tracking.rs`):**
  - `subtree_hash_at`: returns the hash of the Original block whose source range `== [r0,r1]`; `None`
    for a range that matches no Original block; disambiguates a container from its first inline child
    that shares `r[0]` (full range).
  - `locate_subtrees` — the matrix:
    - **shifted** (insert above) → relocates to the new range;
    - **deleted** → `None`;
    - **nested-child-after-insert-above** (THE bug) → relocates to the child, not the container;
    - **duplicate subtrees** → returns the match **nearest** `|r0 − hint|`;
    - **trivial-content node** (empty paragraph / `HorizontalRule`, huge hash class) → nearest-by-hint
      is the sole disambiguator (documented);
    - **non-local parse change** (an unclosed ``` fence typed above swallows the node) → the node's
      subtree hash changes → `None` (conservative drop).
- [ ] `pub fn subtree_hash_at(untransformed_ast_json: &str, r0: usize, r1: usize) -> Option<u64>`.
- [ ] `pub fn locate_subtrees(untransformed_ast_json: &str, targets: &[(u64 /*hash*/, usize /*hint_r0*/)])
  -> Vec<Option<(usize,usize)>>` — single AST walk, per target collect hash-matches, pick min
  `|r0 − hint|`, **early-out** when a target's match set can't improve. Block-level only (Phase 3 edits
  blocks; inline-span editing is out of scope).
- [ ] WASM exports in `crates/wasm-quarto-hub-client/src/lib.rs` (mirror `apply_node_edit`), u64 as
  **string** across the JS boundary.
- [ ] JS wrappers `subtreeHashAt` / `locateSubtrees` in `ts-packages/preview-runtime/src/wasmRenderer.ts`.

### Phase C — Shared parent tracking registry + sidecar

A pure, host-agnostic `NodeTracker` (plus a thin `useNodeTracking` adapter if convenient) in
`ts-packages/preview-runtime/` — consumed by **both** `ReactPreview` and `PreviewApp`. Mirrors the
`computeNestedEditBuffers` shared-helper precedent.

- [ ] **Test (unit, mocked WASM):** `register(trackId, r0, r1, generation)` computes+stores
  `{hash, anchorSlice, lastPos, generation}` against the **retained** AST of that generation (not
  necessarily the current one — handshake correctness); `untrack` removes it; `relocate(newAst,
  newContent)` returns `{trackId → [r0,r1] | null}` applying: hash recall (`locateSubtrees`) →
  **slice-verify** vs stored `anchorSlice` → **distance cap** → write back `lastPos` on KEEP.
- [ ] **Test:** a hash collision (mock two structurally-different nodes to same hash) is caught by the
  slice-verify → `null`, not a wrong KEEP.
- [ ] **Test:** the sidecar is **omitted entirely when nothing is tracked** (zero-cost default path,
  referential-stable empty — mirror `nestedEditBuffers`/`EMPTY`).
- [ ] Add iframe→parent messages `TRACK_NODE {trackId, r0, r1, generation}` and `UNTRACK_NODE {trackId}`
  to the message union + dispatch (`iframeMessageDispatch.ts`), handled in both hosts.
- [ ] Wire `NodeTracker` into both hosts' `UPDATE_AST` assembly: after producing a fresh
  `untransformed_ast_json`+`renderedContent`, call `relocate(...)` and attach
  `trackedLocations?: Record<string,[number,number]|null>`.
- [ ] Slice-verify, distance-cap threshold, and drop policy live here (TS). Distance cap = a multiple of
  the inter-generation content-length delta (tunable; default generous).

### Phase D — Iframe consumption (the self-heal swap)

- [ ] **Test (real `PreviewRoot`, `p2-3b-real.integration.test.tsx`; fail-on-revert):** the headline —
  a **nested blockquote/list child** edit **survives** a concurrent insert above the container
  (`trackedLocations` re-anchors it; draft preserved). Reverting the track-me branch → falls back to
  `findReanchorCandidate` → child DROPs → test fails. *(This is jsdom-testable: pure pool/content/hash,
  no layout.)*
- [ ] **Test:** `trackedLocations[id] === null` (collaborator deleted or content-changed the node) →
  editor **drops**, draft discarded, drop-focus best-effort (existing semantics).
- [ ] **Test:** duplicate-subtree → re-anchors to the **nearest** instance (parent tie-break); the
  far-twin-after-own-deletion case is documented as the residual (see §"Design problems").
- [ ] **Test:** with the field **absent** (host not wired / no tracking), the effect falls back to the
  existing `findReanchorCandidate` — no regression to the predecessor suite.
- [ ] Iframe sends `TRACK_NODE` at edit-open (in `activate`/`captureEditTarget`), `UNTRACK_NODE` on
  close/commit, and **re-tracks on a nesting-cursor in/out move** (new cursor node). One active `trackId`
  now; the registry is a `Map` for future multi-track.
- [ ] `PreviewRoot.tsx` self-heal effect: **prefer `props.trackedLocations?.[activeTrackId]`** (present →
  re-anchor / `null` → drop); **else** fall back to `findReanchorCandidate`. Keep the write-back of the
  re-anchored offset (feeds the next render's `lastPos`).

### Phase E — End-to-end + verification

- [ ] **Playwright (real `q2 preview` / hub):** open an editor on a nested blockquote child, simulate a
  concurrent insert above, assert the editor stays open on the child with the draft intact; delete the
  child remotely → drops.
- [ ] Full `cargo xtask verify` (WASM leg) + `npm run build:all` + preview-renderer/hub-client suites.
- [ ] Live `q2 preview` smoke per CLAUDE.md's rebuild chain.

---

## Design problems (must be respected by the implementation)

1. **Duplicates — the core limit.** Identical subtrees hash-equal; nearest-`lastPos` tie-break + slice
   verify resolves the common case. **Irreducible residual:** *your* instance is deleted **and** an
   identical twin survives elsewhere → nearest picks the twin, slice matches → wrong KEEP (a later
   commit edits the twin). Bounded (needs dupes **and** deletion-of-your-instance mid-edit). Mitigation:
   the **distance cap** converts an implausibly-far sole match into a safe DROP. Only **position
   identity** (Automerge cursors) fully closes it — the reason cursors stay on the table.
2. **Non-local parse dependency.** A node's hash is of its *parsed* AST, which depends on context. A
   distant edit (unclosed fence above, list tight/loose flip, blockquote lazy-continuation) can change
   how the node parses → hash miss → DROP from a "far" edit. **Conservative-correct** (the node's
   meaning genuinely changed) but can surprise. The byte-slice approach has the same exposure — not a
   regression. (qmd's no-reference-link rule removes one classic markdown instance.)
3. **`FxHasher` is non-cryptographic.** Keep the `anchorSlice` content-verify as the collision backstop
   and final arbiter — hash for recall, slice for precision.
4. **The hint must be re-anchored each render** (write-back) and the search must be **bidirectional**
   (`|r0 − hint|`), unlike today's `r0 >= anchorR0` (which silently misses a delete-above). Fixing that
   directional bias is part of this work.
5. **Trivial-content nodes** (empty paragraph, `HorizontalRule`) have enormous hash classes and lean
   entirely on the position tie-break. Rare edit targets; acknowledged.
6. **Keep the iframe dumb.** All of hash-find + nearest + slice-verify + distance-cap + drop policy is
   **parent-side**; the iframe consumes one `[r0,r1] | null`. The parent stores `{hash, anchorSlice,
   lastPos, generation}` per tracked id.
7. **Perf.** One early-out hash-walk per external render **while tracking only**; zero on the default
   path.

---

## Automerge cursors — also considered (and deferred)

Cursors are the predecessor plan's deferred "exact position tracking" (Deferred §Automerge cursors).
They genuinely close design-problem #1 (position identity distinguishes "moved" from
"deleted-with-twin"). Quick rundown of why we **defer**, not adopt, for this plan:

- **Precedent is clean** (`usePresence.ts:251-261,349-358`): `A.getCursor(doc,['text'],offset)` /
  `A.getCursorPosition(...)`, mapped to line/col via Monaco's `getOffsetAt`/`getPositionAt`.
- **But the coordinate space differs from the AST.** Cursors index by **JS character position**
  (`automergeCursor.probe.test.ts:20-27`); source-info `r0/r1` are **UTF-8 byte offsets**. So an
  AST-node cursor needs a **byte↔char bridge** on every track/resolve — the exact offset-domain hazard
  this codebase keeps hitting (the earlier "cursors are UTF-8, no bridge" claim was wrong). Tractable
  (pure encoding conversion over the known-identical `renderedContent`), but real.
- **The preview parent has no doc handle.** `ReactPreview` receives only `content: string`
  (`ReactPreview.tsx:284-301`); cursors need `A.getCursor(doc,…)`, so we'd plumb the Automerge doc (or a
  cursor service) into the deliberately-pure preview path. The hash needs **none** of this.
- **Hub-client only.** The SPA has no Automerge; the hash works on both hosts with zero new plumbing.
- **Cursors still need a content check** for deletion (a deleted anchor collapses to a neighbor,
  `probe.test.ts:53-59`) — so they don't even remove the slice-verify.

**Decision:** ship the hash now (universal, self-contained, fixes the bug); keep cursors as the
hub-client **exactness upgrade** that closes the one residual, behind the *same* iframe-facing
byte-range protocol (the iframe never learns which primitive the parent used).

---

## Impact on related design docs

- **`claude-notes/designs/2026-06-06-block-editing-design.md` — amend three places:**
  - **§"Identity and concurrency" (l.87-147):** relocation becomes **parent-authoritative** when
    `trackedLocations` is present; iframe-side `findReanchorCandidate` (single-nearest) becomes the
    **fallback**. The identity triple (`anchorR0/anchorR1/anchorSlice`) is **unchanged** — only *who
    computes the new anchor* moves to the parent (which can scan structurally, not just the single
    nearest). Add the duplicate residual + distance-cap note.
  - **§"Key facts" (l.351-352):** the "postMessage is one-way … no request/response channel" statement
    needs nuance — there is now a host→iframe **location sidecar** (`trackedLocations` on `UPDATE_AST`)
    and iframe→host **`TRACK_NODE`/`UNTRACK_NODE`** registration. It is **still not** a request/response
    channel: locations ride the *next* `UPDATE_AST`, paired by `generation`. Reword so the doc isn't
    self-contradicted; add the `generation` field to the payload description.
  - **§"Known limitations (v1) and risks" (l.357):** mark the nested-child concurrency limitation
    **resolved by this successor**; record the new residual (deleted-instance + surviving-twin →
    cursors).
  - Note that the Phase-3 **nesting-cursor** self-heal uses the same parent-authoritative relocation (the
    "identical to Phase 2" claim still holds — the *mechanism* upgrades for both).
- **`claude-notes/plans/2026-06-11-block-editing-improvements.md` — cross-reference (no behavior change):**
  - Risks §"Nested-child self-heal DROP" (l.932-948): mark **resolved by** this plan (this is candidate
    fix **(b)**); link here.
  - Deferred §"Automerge cursors" (l.1002-1007): note cursors were re-evaluated in the successor and
    deferred, with the specific residual they would close.
- **`claude-notes/research/2026-06-13-tree-reconciliation-node-relocation.md`:** the deep rationale;
  **bring it onto this branch** (currently on `bugfix/hub-sync-fork-panic`). It already concludes "verdict
  (b): a usable position-independent hash exists; here is the smallest mechanism to expose it" — this
  plan is that mechanism, in its parent-authoritative form.
- **No impact** on `document-profile-contract.md`, `provenance-contract.md`, the attribution/source-info
  wire-format docs: track-me adds a payload field + messages, not a new AST/source-info shape, and reuses
  the existing untransformed pool. (Confirm during Phase C that `trackedLocations` is purely additive on
  the wire.)

---

## Verification (per phase)

- **A:** generation increments + echo + 2-slot retention (RTL/unit, both hosts).
- **B:** `cargo nextest run -p pampa` over the `node_tracking` matrix; `cargo xtask verify` (WASM).
- **C:** shared `NodeTracker` unit tests (mocked WASM): register/relocate/untrack, collision→null,
  zero-cost-when-empty.
- **D:** real-`PreviewRoot` integration — nested-child KEEP (fail-on-revert), deletion DROP, duplicate
  nearest, absent-field fallback.
- **E:** Playwright on real `q2 preview`/hub; full `cargo xtask verify` + `npm run build:all`.

---

## Appendix — design rationale / consequence ledger (condensed)

The full `/loop` enumeration that produced this plan (waves 1–3, consequences C1–C41 classified
✅ obvious / ❓ design-question / ⚠️ risk) is preserved in git history of this file (prior revision).
The load-bearing conclusions, all folded into the phases above:

- **C1/C20** the iframe-facing protocol is **byte-range in/out**; the parent's primitive is private
  (enables the cursor upgrade with no iframe change).
- **C4** `trackedLocations` rides `UPDATE_AST` ⇒ location is same-generation as content **by
  construction** — no separate async channel, no request/response.
- **C5/C6/C7** hash gives clean deletion (`null`), clean content-change drop, and **sibling-edit
  immunity** (per-subtree) — the latter is the nested-child fix.
- **C13/C33** duplicates → nearest-`lastPos` + slice-verify + distance-cap; residual = deleted-twin
  (cursors' domain).
- **C15** the generation handshake is the correctness floor (TRACK offsets are version-relative).
- **C17/C18** commit-rebase and destination-projection reuse this exact primitive — **deliberately
  out of scope** here, designed-for.
- **C21/C22** shared `NodeTracker` + one WASM export; integration is a one-line swap in the
  `PreviewRoot` self-heal effect with the client scan as fallback.
- **C25** orthogonal to (and sequenced after) the predecessor's in-flight P3.4 "self-heal on write"
  (a React-lifecycle fix, not an identity fix).

## References

- Predecessor: `2026-06-11-block-editing-improvements.md`; design `2026-06-06-block-editing-design.md`.
- Research: `2026-06-13-tree-reconciliation-node-relocation.md` (branch `bugfix/hub-sync-fork-panic`).
- Rust: `crates/quarto-ast-reconcile/src/hash.rs` (`compute_block_hash_fresh:102`),
  `crates/pampa/src/apply_node_edit.rs` (deserialize-walk model, `:169/:174`),
  `crates/wasm-quarto-hub-client/src/lib.rs` (`apply_node_edit:2938`).
- TS: `ts-packages/preview-renderer/src/q2-preview/{lockedTiles.ts,PreviewRoot.tsx}`,
  `iframe/Q2PreviewIframe.tsx`, `iframeMessageDispatch.ts`;
  `ts-packages/preview-runtime/src/wasmRenderer.ts`;
  `hub-client/src/components/render/{ReactPreview,ReactRenderer}.tsx`,
  `hub-client/src/hooks/usePresence.ts`, `services/presenceService.ts`,
  `services/automergeCursor.probe.test.ts`; `q2-preview-spa/src/PreviewApp.tsx`.
