# Hand-off: block-editing execution (Phase 2 → P2.5 + Phase 3)

**Written:** 2026-06-13, mid-execution, to let a fresh context resume without carrying the
whole conversation. **Plan being executed:** `claude-notes/plans/2026-06-11-block-editing-improvements.md`
(symlinked as `CURRENT.md`). Read that plan first; this file is the *execution* companion —
how we're working, what's done, and the curated facts so you don't re-derive them.

---

## 0. FIRST: confirm the current state (a P2.4d fix may have been in-flight)

Before doing anything, orient:

```bash
cd /Users/gordon/src/q2/.worktrees/block-editing
git log --oneline -20
cd ts-packages/preview-renderer && npm run test && npm run test:integration && npm run typecheck
```

- Working tree should be clean, branch `feature/block-editing-improvements`.
- The last in-flight task was **P2.4d click-switch** and a follow-up to make its tests drive
  the *real* production path (the original P2.4d tests were "test theater" — a 350-line harness
  that reimplemented the logic and passed even with production reverted). Confirm via git log
  that a commit like `test(block-editing): drive P2.4d click-switch through real production path`
  exists AND that `p2-4d.integration.test.tsx` mounts the real `<Ast .../>` tree (not a
  reimplementation harness). If that fix did NOT land or the tests still don't fail-on-revert,
  finish it first (see §4 "P2.4d test-reality debt").

---

## 1. How we work (the process — keep this rigor)

**Skill:** `superpowers:subagent-driven-development`. Per plan task:
1. **Implementer** — dispatch a fresh `Agent` (subagent_type `general-purpose`, **model `sonnet`**)
   with a *fully self-contained* prompt. Do NOT make the subagent read the plan; paste the curated
   task + context + file:line refs + commands. TDD: failing test first, see it fail, implement,
   see it pass.
2. **Spec-compliance reviewer** (sonnet) — independent; "do not trust the report", read the diff,
   run the tests. For small leaf tasks you may combine spec+quality into one review pass.
3. **Code-quality reviewer** (sonnet) — only after spec passes.
4. Implementer (re-dispatch) fixes issues; re-review until clean.
5. Check off the plan checkbox(es) + `git commit` (per task). **NEVER push** (user pushes).

**Models:** user wants **sonnet for implementation and review**. You (the orchestrator) are opus;
stay the coordinator. Escalate a subagent to opus only if it reports BLOCKED on reasoning.

**Tracking surface:** the plan file checkboxes. `CURRENT.md` is a symlink — the Edit tool refuses
to write through it; edit the real file `2026-06-11-block-editing-improvements.md`. Mark items
`[x]` (done) and add a short `(P2.x; <what's RTL vs deferred-to-Playwright>)` note. Commit plan
edits separately (`docs(block-editing): check off …`).

**Commits:** conventional (`feat`/`fix`/`test`/`style`/`refactor`/`docs`), scope `block-editing`.
`git add -A` then commit. Do NOT push. (`git push` is denied by policy; user approves pushes.)

**Tests / verification:**
- From `ts-packages/preview-renderer`: `npm run test` (unit `*.test.ts`), `npm run test:integration`
  (`*.integration.test.tsx`), `npm run typecheck`. **Run directly — never pipe through tail/head
  (it hangs).** `npm install` only from repo root if `node_modules` missing (npm workspace).
- **Phases 1–2 are Rust-free** → no cargo needed; `cargo xtask verify --skip-hub-build` only if you
  touch Rust.
- **Phase 3 touches the WASM leg** (`crates/pampa`, `crates/wasm-quarto-hub-client`) → run
  `cargo nextest run -p pampa` + full `cargo xtask verify` (NOT `--skip-hub-build`), and for a live
  `q2 preview` check the chain: `cd hub-client && npm run build:wasm` → `cargo xtask build-q2-preview-spa`
  → `cargo build --bin q2` (a plain build does NOT rebuild WASM — preview runs stale otherwise).
- hub-client changes require a `hub-client/changelog.md` entry (two-commit workflow: code commit,
  then changelog commit referencing the hash). Phase 3 §3a/3b touch hub-client.

**Reviews earn their keep — examples of real bugs the gates caught this session (so keep verifying
tests FAIL on pre-change code for every bug-fix/behavioral task):**
- P2.3a silently dropped "empty draft → cancel" (would have committed an empty/deleted block).
- P2.3b hidden-drop used the wrong predicate (nearest-fallback masked a hidden re-anchored tile);
  fixed with an `exactOnly` mode.
- P2.4b opened move-destination editors at `contentHeight:0`/`boxStyle:{}` (invisible textarea).
- P2.4d tests reimplemented the logic instead of exercising production (test theater).
Always ask the reviewer to confirm (by actual revert/checkout) that a bug-fix test fails on the
old code. Beware harness-reimplementation tests that pass regardless of production state.

---

## 2. State — DONE (with commits; all green at each step)

Phase 1 + Phase 2.1–2.4c are complete and committed. Representative commits (run `git log` for
exact hashes; they're all on `feature/block-editing-improvements`):

- **Phase 1** active-region bug fix — `10775de0` (+ test/style follow-ups). `onPointerUp` now
  suppresses cross-surface activation when the click is inside the active editor (caret-move), via
  a shared `ctx.activeEditRegionRef` on the editor's inner wrapper div.
- **P2.1** `byteLineMap.ts` (UTF-8 byte↔0-based-line map) + `r[0]`-uniqueness regression test over a
  real pampa fixture (`__fixtures__/r0-uniqueness.{qmd,ast.json}`).
- **P2.2** `lockedTiles.ts` — locked resolution (prefixing-atomic dominates, else coincidence climb),
  `enumerateLockedTiles`, `isVisibleTile`, `rectsCoincide`; `activate` resolves to the locked tile;
  roving-tabindex navigates locked tiles.
- **P2.3a** byte-offset identity: `editTarget = {anchorR0, anchorR1, anchorSlice, contentHeight,
  boxStyle}`; controlled textarea (`EditTextarea`) via root `ctx.editDraftRef` + local state
  (survives remount); dirty guard `normalizeLineEndings(draft).trimEnd() !== anchorSlice`;
  empty→cancel; IME composition guard; match by `anchorR0 === resolved.sourceEntry.r[0]`.
- **P2.3b** self-heal: a `useLayoutEffect` in `PreviewRoot` (keyed on render inputs, reads editTarget
  via ref) re-anchors the open editor across an external re-render (content-verified) or drops it
  (mismatch / no candidate / now-hidden) + drop-focus. Helpers `tileForAnchorR0(host,pool,r0,{exactOnly})`
  and `findReanchorCandidate(pool,content,r0,slice)`.
- **P2.4a** `caretGeometry.ts` — mirror-div `isOnFirst/LastVisualLine` (geometry; Playwright-verified
  later) + pure `getLogicalColumn`/`placeCaretAtColumn`.
- **P2.4b** arrow-nav move machine: `requestMove`, `pendingLanding` (discriminated union),
  `executeLanding`, reland `useLayoutEffect` (render-input-keyed) + `setTimeout(250)` fallback for
  byte-identical commits; destLine projection (down=L0+n, up=L0); `captureEditTarget`/`measureTileBox`
  shared with `activate`; caret-on-arrival via `pendingCaretRef`; bare-arrow-only guard.
- **P2.4c** plain-commit focus-restoration: `intent:'focus'` landing; Esc/Cmd-Enter/blur (no move)
  stash a focus landing → `executeLanding` focuses the edited tile by `anchorR0` (don't-steal guard
  if a new edit started); Esc uses the timeout fallback (no content re-render).
- **P2.4d** click-switch: classify in `onPointerDown` (active-region→caret-move; different tile→switch;
  empty→plain close); dirty source → commit + projected reland on B; unmodified → existing
  activate path; `clickSwitchRef`/`dirtySwitchHandledRef` coordinate pointerdown→blur→pointerup.
  *(Confirm the real-test fix landed — see §0/§4.)*

JS suite was ~349 integration + ~303 unit passing after P2.4d. Run the suite to get the current count.

---

## 3. Curated technical facts (so you don't re-derive them)

**Coordinate systems (critical):**
- Source offsets are **UTF-8 byte offsets**. `content` (= `props.renderedContent`) is a UTF-16 JS
  string. `sliceBytes(content,r0,r1)` (`src/utils/sliceSource.ts`) slices by bytes. `byteLineMap`
  works in byte space. The textarea draft / caret column work in JS-string space (a *separate*
  concern — don't conflate visual-line geometry, logical-line column, and byte offsets).
- `anchorR0` lives in the pool `r` space, which **equals** the untransformed `sourceEntry.r` by
  value-keyed correspondence: `resolveSource` returns `sourceEntry = pool[node.s]`, so
  `pool[tile.s].r[0] === resolved.sourceEntry.r[0]`. Capture at click time from `ctx.pool[s].r`;
  match at render time against `resolved.sourceEntry.r[0]`.
- `data-block-pool-id` holds the pool **index `s`** (a positional ordinal, reassigned every render),
  NOT `r[0]`. To get `r[0]` from a tile element: `pool[Number(el.dataset.blockPoolId)].r[0]`.
- Assumption B: block/block `r[0]` are distinct; a block and its first *inline* child CAN share
  `r[0]` (the dispatcher only matches block nodes, so fine; self-heal's `find` returns the block
  first since it's interned before its inlines, and the `anchorSlice` content-check is the arbiter).
- CRLF: pampa parses CRLF natively (offsets include `\r`); the textarea LF-normalizes. So normalize
  **sliced strings** (`\r\n`→`\n`, via `normalizeLineEndings`), NEVER the `content` buffer.

**Module map (`ts-packages/preview-renderer/src/q2-preview/`):**
- `lockedTiles.ts` — `resolveLockedTile`, `enumerateLockedTiles`, `isVisibleTile`, `rectsCoincide`,
  `tileForAnchorR0(host,pool,r0,{exactOnly?})`, `findReanchorCandidate`, `captureEditTarget(tileEl,pool,content)`,
  `measureTileBox(tileEl)`. The shared geometry/identity primitives. Reuse these in Phase 3.
- `byteLineMap.ts` — `buildByteLineMap(content)` → `{lineOf, lineStart, lineCount}` (0-based, byte space).
- `caretGeometry.ts` — `isOnFirst/LastVisualLine` (mirror-div; mock in jsdom), `getLogicalColumn`,
  `placeCaretAtColumn`.
- `useBlockEditHover.tsx` — delegated host handler; `activate(el)` resolves locked tile + captures
  identity + measures box + seeds `editDraftRef` + `setEditTarget`; onPointerDown click-switch
  classification; onPointerUp active-region guard + suppression.
- `dispatchers.tsx` — `Block`/`CustomBlock`, `isBlockEditTarget` (matches anchorR0), `EditTextarea`
  (controlled, dirty guard, IME, commit, onKeyDown move trigger, mount caret placement, blur
  click-switch/focus-restore).
- `entry.tsx` `PreviewRoot` — owns `editTarget` state, `editDraftRef`, the self-heal effect, the
  reland effect + `pendingLandingRef`/`pendingCaretRef`/`fallbackTimerRef`, `requestMove`/
  `requestFocusRestore`/`requestClickSwitch`/`cancelPendingLand`, `previewHostRef` (display:contents
  wrapper for scoping queries), and provides `PreviewContext`.
- `PreviewContext.tsx` — the context interface (editTarget, editDraftRef, the request* methods,
  resolveSource, pool, content, editingDisabled, …).
- `sourceIndex.ts` — `buildSourceIndex`, `serializeSourceEntry` (`"t:r0-r1:d"`), `ResolvedSource`.

**Controlled-value architecture (P2.3a):** the draft lives in a root **ref** (`editDraftRef`,
seeded ONLY at fresh-open in `activate`/reland, NOT in `setEditTarget` — so a self-heal re-anchor via
`setEditTarget` preserves the in-flight draft). `EditTextarea` mirrors it in local `useState` for the
controlled value and survives remounts (index-keyed children, `utils.tsx`). This avoids a whole-doc
re-render per keystroke without memoizing the document. Do NOT put the draft in `PreviewContext`
(widely consumed → per-keystroke re-render of every block).

**pendingLanding machine (P2.4b/c/d):** one reland mechanism (`executeLanding` + a render-input-keyed
`useLayoutEffect` + a 250ms timeout fallback for byte-identical commits) handles three intents:
`activate` (arrow move / click-switch → open destination by projected destLine) and `focus`
(plain commit/Esc → focus the edited tile by anchorR0, no open). Self-heal is a *sibling* effect that
runs only when the editor is still open (collaborator re-render); reland runs only when it's closed
(your own move/commit). `fromFile` cancels a stale landing on file switch.

---

## 4. Remaining work

### P2.4d test-reality debt (if not already fixed — see §0)
The original P2.4d tests reimplemented the logic. The fix: mount the real `<Ast astJson=… setAst={spy} …/>`
tree, open an editor by firing a real click on a tile, type, fire pointerdown(B)→blur→pointerup(B),
assert `setAst` called, re-render with the committed doc, assert B's editor opens (projected anchorR0).
**Verify the dirty test FAILS when production is reverted** (`git checkout <pre-P2.4d> -- entry.tsx
dispatchers.tsx useBlockEditHover.tsx PreviewContext.tsx`, keep the test, run it). Minor cleanups:
`direction` const (always 'down'), move `ClickSwitchRecord` to module scope.

### P2.5 — Update existing test corpus + Playwright e2e (last Phase-2 task)
Plan items still unchecked: **"Update existing editing tests"**, **"Click-switch" RTL** (done by P2.4d
if real), **Playwright `q2-preview-inline-edit.spec.ts`**, and the Playwright legs noted on several
P2.2/P2.3b/P2.4 items (real-browser coincidence epsilon, collapsed-callout visibility, soft-wrap
last-visual-line + caret-on-arrival).
- **Audit (important):** verify the broader P2.4 move/reland/focus machinery in `entry.tsx` is
  covered by tests that drive the **real** `PreviewRoot` (not harness reimplementations like the
  P2.4d theater). Several P2.4b/c tests wire spy `requestMove`/`requestFocusRestore` (they prove
  onKeyDown/onBlur *call* the right method) but may not exercise the real `executeLanding`/reland.
  Add real-tree integration tests for: arrow move (unmodified sync + modified reland), plain-commit
  focus-restoration, and confirm fail-on-revert for at least the modified-move reland.
- **Corpus migration:** the default click target changed (leaf → locked tile) and the textarea went
  uncontrolled → controlled. Audit/fix tests that assert the old behavior: `useEditableBlock`,
  `q2-preview`, `useBlockEditHover` integration, and e2e `q2-preview-inline-edit`,
  `q2-preview-columns-layout`, `q2-preview-render-components-*`, `edit-cell-sizing`. (Prior tasks
  flagged the tests they migrated; grep for `flag for P2.5` rationale in commit messages.)
- **Playwright:** these need the real browser (jsdom can't do geometry). Find the existing Playwright
  setup (`hub-client` e2e or `q2-preview-spa`); see how `q2-preview-inline-edit.spec.ts` is run.
  Tune the coincidence epsilon (P2.2 used `rectsCoincide` eps=0.5 in jsdom; verify against real
  Bootstrap subpixel layout — assumption A measured true coincidence at exactly 0px, nearest
  deciding edge ≥~12px, so an eps in ~0.5–2px is fine, but confirm a 1px-border resolves to leaf and
  a true chrome-less div coincides). **Per CLAUDE.md, exercise the real binary** — for q2-preview,
  `cargo run --bin q2 -- preview docs/` (Q2, never the system `quarto`). End-to-end verification is
  mandatory before declaring P2 done (CLAUDE.md "End-to-end verification before declaring success").

### Phase 3 — "Depth cursor (nested blocks)" unlock (flagged, default-off)
Entry points confirmed present: `crates/pampa/src/writers/qmd.rs::write_single_block` (line ~2392,
public, fresh context = no `> `/indent prefixer), WASM `crates/wasm-quarto-hub-client/src/lib.rs::apply_node_edit`
(~2938), `ts-packages/preview-runtime/src/wasmRenderer.ts` (export pattern for JS wrappers). Suggested
decomposition (each a review-gated subtask):
- **P3.1 (Rust/WASM)** — `pampa::regenerate_nested_buffers(content, untransformed_ast_json) -> String`
  (JSON `{siKey: cleanQmd}`) using `write_single_block`→`trim_end`; restriction: block has a prefixing
  ancestor (fenced `:::` excluded) AND is multi-line; no reachability filter in Rust (over-inclusion
  harmless). `siKey = serializeSourceEntry` format `"0:<r0>-<r1>:0"`. WASM export mirroring
  `apply_node_edit`; JS wrapper `regenerateNestedBuffers` in `wasmRenderer.ts`. Native-testable —
  Rust tests for siKey contract, source fidelity (shortcode/math/raw, code blocks), offset-domain.
- **P3.2 (setting + threading)** — `unlockDepthCursor: boolean` (default false) in
  `hub-client/src/services/preferences/schema.ts` (+ DEFAULT_PREFERENCES), a checkbox in
  `SettingsTab.tsx` (mirror `errorOverlayCollapsed`), read via `usePreference`. Thread host→iframe:
  reuse the proven `editingDisabled` iframe wire (UPDATE_AST payload → entry.tsx → PreviewContext);
  ADD the missing **hub-client host leg** `ReactPreview → ReactRenderer → Q2PreviewIframe`
  pass-through (hub-client threads no such flag today; SPA bypasses ReactRenderer). Phase 3 is
  **hub-client-only by design** (SPA stays on locked/Phase-2 behavior).
- **P3.3 (behavior when on + buffer gating)** — click resolves to the **leaf** (no coincidence-climb,
  no prefixing-atomic); identity is the same `anchorR0`, plus a `leafAnchorR0` scalar for "in"; depth
  keys mutate anchorR0 along the AST path (**macOS `Cmd+Ctrl+←/→`**, **Win/Linux `Alt+Shift+←/→`**),
  clamp at ends at key-press; path derived from the AST each render (no stored depth/path). Gate the
  `regenerateNestedBuffers` table in a `useMemo` keyed on `[unlockDepthCursor, renderedContent,
  untransformedAstJson]` (else a module-level shared empty object for referential stability); seed
  `draft` from `nestedEditBuffers?.[siKey] ?? slice` at activate (so a siKey-shift doesn't break the
  active editor). Plumb `nestedEditBuffers` onto the UPDATE_AST payload from **both** hosts.
- **P3.4 (breadcrumb floating toolbar)** — stopPropagation on its own pointer handlers; absolutely
  positioned above the active surface's top-left; shows the AST-derived ancestor path with `◀`/`▶`
  in/out buttons (+ platform shortcut tooltips, the touch affordance); shown only when the flag is on.
- **P3.5 (e2e)** — setting end-to-end, depth keys, WASM round-trip (edit nested blockquote child →
  clean commit), cross-platform Playwright for the bindings (verify native word/line-select still work).

See the plan's Phase 3 section for the full detail and TDD checklist; the above is the decomposition.

---

## 5. Decisions already made (don't re-litigate)
- **Blockquote is atomic in locked (default) mode** (confirmed with the user 2026-06-13): a click
  anywhere in a blockquote/list selects the whole outermost prefixing container. The stale plan
  checklist line ("blockquote text → child") was corrected. Full per-layer descent is the Phase 3
  unlock. Reason: only the outermost prefixing container has a clean byte-slice; inner targets'
  slices carry the outer `> `/indent (that's why Phase 3 needs AST regeneration, gated behind the flag).
- **Controlled value via root ref + local state**, not whole-document memoization (lower risk; see §3).
- **Plain-commit focus-restoration deferred from P2.3a → P2.4c** (folded into `pendingLanding{intent:'focus'}`).
  The broken pool-id `setTimeout` focus-restore was removed in P2.3a.
- **Click-switch dirty case projects the destination** and relands post-commit-re-render (can't use a
  stale pre-commit anchorR0); unmodified click-switch uses the existing direct-activate path.
