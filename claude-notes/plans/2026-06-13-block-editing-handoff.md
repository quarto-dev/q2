# Hand-off: block-editing execution (Phase 2 done → Phase 3)

**Updated:** 2026-06-13 (Phase 2 done; **Phase 3 P3.1 + P3.2 + P3.3 done and green; self-heal-on-write →
P3.4 next** — see the START-HERE block below). **Plan:** `claude-notes/plans/2026-06-11-block-editing-improvements.md`
(symlinked `CURRENT.md`). Read that plan, then this file — this is the *execution* companion: how we
work, what's done, the testing rules we learned the hard way, and curated facts so you don't re-derive
them.

---

## Phase 3 progress — START HERE (updated 2026-06-13, end of impl session)

**P3.1 + P3.2 + P3.3 are DONE and green (jsdom tier). NEXT: do _self-heal on write_ (verify-then-fix)
FIRST, then P3.4 (breadcrumb) — see the "NEXT SESSION" block below.**

**Commits (on `feature/block-editing-improvements`):**
- **P3.1** (Rust `regenerate_nested_buffers` + WASM export + JS `regenerateNestedBuffers`):
  `f5cb3132`, `8a51bb92`.
- **P3.2** (setting + both-host threading + gating + fixes): `7f14e5ed`, `17f9b196`,
  `aae25bf0`, `dff074da`, `c96f06a0`, `8af69988`, `b66f898a`.
- **P3.3** (§3b depth behavior + §3c regenerated-buffer commit): `8643c27f` (`depthNav.ts` pure
  module — `parentSurface`/`childSurfaceToward` range-containment, `classifyDepthKey`,
  `buildDepthCommitDestination`), `0dde110c` (§3c: leaf resolution, buffer-seeded draft +
  `seededDraft` dirty-baseline, live-identity `commitDepthEdit`), `ad797be1` (§3b: depth-key in/out
  nav + clamp + off-inert). Plan checkboxes + finding: `20607ffb`.

**Verified green (full scope):** pampa 3943 · preview-renderer **351 unit + 364 integration** ·
hub-client 755 (`test:ci`) · SPA 25 unit + 75 integration (`test:integration`) · `npm run build:all` ·
all typechecks. *(P3.3 added preview-renderer tests only; hub-client/SPA/pampa counts are from the
P3.2 verification — re-run before relying on them.)* Gating independently re-verified.

**P3.3 caveats the next session MUST know:**
- **jsdom tier only.** The real key-chords (macOS `Cmd+Ctrl+←/→`, Win/Linux `Alt+Shift+←/→`) + native
  word/line-select non-conflict, soft-wrap geometry, and the WASM round-trip (nested edit → clean
  `> `-rewrapped commit) are NOT verified — they need `q2 preview` + the `build:wasm` chain + a browser
  → **P3.5 Playwright/e2e**. `classifyDepthKey` logic is unit-tested; the wiring is integration-tested
  with the *detected*-platform chord (jsdom → 'other' → Alt+Shift).
- **Depth move = re-selection, not a commit.** A depth in/out re-seeds the draft from the new node's
  buffer/slice ("read once at selection"); it does NOT commit, so a dirty draft from the previous depth
  position is replaced. The real commit is blur/Cmd-Enter via `commitDepthEdit` (live `editTargetRef`).
- **⚠ NESTED-CHILD SELF-HEAL DROP (real, unfixed — see the plan's Risks watch-item).** A nested child
  being depth-edited DROPS its in-flight edit if a collaborator inserts ≥~2 bytes ABOVE its container:
  `findReanchorCandidate` picks the shifted *container* as the single nearest `r[0]>=anchorR0`, fails
  content-verify, and doesn't scan to the child. Phase-2 locked editing is unaffected. Two candidate
  fixes (the user is deciding scope): (a) make `findReanchorCandidate` scan ALL `r[0]>=anchorR0` for the
  first that content-verifies (client-only; contradicts the documented "single nearest"); (b) content-
  addressed relocation via a position-independent AST-subtree hash from q2's reconciliation/coarsen
  pipeline. An investigation prompt for (b) was drafted this session (ask the user; not yet dispatched).

---

## NEXT SESSION — do these in order

### Task 1 (FIRST): "Self-heal on write" — VERIFY-THEN-FIX (decided 2026-06-13)

The plan's **"Self-heal on write"** subsection (just before `## Phase 3`) is the architectural follow-up
to *complete the identity migration for WRITES* (reads already track the self-healed location). Its
TDD checklist now lives there. **Approach is verify-then-fix, not build-blind.** It is distinct from the
nested-child DROP above: that's a *read*/re-anchor failure (editor closes); this is a *write* failure
(a stale commit to the wrong byte range). Doing this is **not** what fixes the nested-child DROP.

**Load-bearing finding from this session's follow-the-consequences (do not re-derive — verify it):**
the plan's headline fix (commit destination = live `editTargetRef.current` instead of the render
closure) does **NOT** fix the stale write by itself. Trace: an external structural edit → render N with
the block's *new* offset; `editTarget` state is still old and the self-heal effect hasn't run, so
`isBlockEditTarget` goes false → the index-keyed textarea **unmounts in render N's commit** → its
`onBlur` fires while `editTargetRef.current` is *still* the old r0 (re-anchor is in the *later* layout
effect). So closure AND live identity both point at `[old]` at that instant — the destination swap is
the clean *form*, not the fix. **The fix is "a teardown/unmount blur must never write"** (intentional
commit vs React-unmount blur). This also means the section's "the guard collapses to the trivial 'is
there still an active target?'" is wrong — a per-instance "am I still the active instance?" check stays.

**Step 1 — reproduce (real `PreviewRoot`).** Dirty editor + external insert above the active block
(offset shifts, textarea remounts); assert whether the teardown `onBlur` fires a stale `commitTextEdit`
to the OLD byte range. This resolves a real contradiction: the P2.3b `commitIfDirty` guard *comment*
claims KEEP-with-shift no-ops, but the render/effect ordering says the unmount precedes the re-anchor so
the guard *passes*. **If jsdom's blur-on-unmount doesn't fire like a browser, say so and move it to
Playwright — do not fake a jsdom pass** (this is exactly the jsdom-blind-spot class that bit P2.5b).

**Step 2 — branch on the result:** reproduced → implement teardown-blur-no-write for the three
TEXT-editor commit paths (`commitIfDirty` locked branch, `requestMove` dirty commit,
`handleClickSwitchBlur`) + build their destination from live `editTargetRef.current`; fail-on-revert
each; fix the contradictory guard-comment. Not reproduced → downgrade to the behavior-equivalent
clarity refactor or skip, and correct the comment. **`commitSubtreeEdit` is OUT OF SCOPE** (decided
2026-06-13: programmatic `usePreviewEdit` path, not tied to the active editor → live-identity doesn't
apply). Rust-free → `cargo xtask verify --skip-hub-build` suffices.

### Task 2: P3.4 — breadcrumb floating toolbar (§3d)

Re-read **§3d** in the plan. A chip shown ONLY when `unlockDepthCursor` is on, that surfaces and
operates the depth cursor. Key build notes (consequences traced 2026-06-13):
- **Event isolation:** the chip `stopPropagation`s on its OWN pointer handlers so the host's delegated
  `onPointerUp`/`onPointerDown` (in `useBlockEditHover`) never see a chip click as a leaf-reset /
  click-switch. Works as a normal child or a React portal (React propagation follows the React tree).
- **Position:** absolutely positioned, anchored above the active surface's top-left (chip bottom edge =
  surface top, negative `top`), never in document flow (zero reflow). The active surface is the edit
  wrapper — use `activeEditRegionRef`. At the very top of the doc it sits in the page margin (no flip).
  **Geometry is environment → Playwright; jsdom can't verify placement.**
- **AST-derived path:** shows `Section › Div › Paragraph` (current level highlighted), derived from the
  AST each render. `depthNav` gives the *ranges* but NOT labels — P3.4 needs an **ancestor-PATH helper
  that returns the ordered path nodes with labels** (node type `t` + id/class from the AST attr). The
  labels need `sourceNode` (type + attr), which `sourceIndex` carries per entry — so extend `depthNav`
  (or add a sibling) to map the range-path → `sourceIndex` entries → `sourceNode` → label.
- **Buttons + crumb-click:** `◀` (out) / `▶` (in) call the existing `ctx.requestDepthMove('out'|'in')`;
  the platform shortcut is each button's tooltip (discoverability + the touch affordance — touch has no
  modifiers). **Crumb-click "jumps to that depth"** = a NEW capability (requestDepthMove only steps one
  level) → add a `requestDepthSelect(r0,r1)` (or jump-to-anchor) that re-targets directly; factor the
  re-target core out of `requestDepthMove` and share it. `leafAnchorR0` is unchanged by a jump (a later
  "in" still descends toward the original clicked leaf).
- **Tier:** the AST-path derivation + button→requestDepthMove + crumb-click→requestDepthSelect are
  jsdom/RTL-testable; chip geometry + the real chords/tooltips are Playwright (P3.5).

### Task 3 (later): P3.5 — Playwright + WASM e2e

Cross-platform chord bindings (assert native word/line-select still works per platform), soft-wrap
last-visual-line, chip geometry, and the WASM round-trip (nested edit → clean `> `-rewrapped commit).
Needs the full `build:wasm → build-q2-preview-spa → build --bin q2` chain + a browser.

---

**Decisions / gotchas the next session must know:**
- **P3.1:** the reviewer-suggested `Figure.caption.long` descent was *reverted* — untestable with
  this parser (a Figure under a prefixing container is emitted whole regardless; no caption-only
  block exists), so it was carrying untested code. DefinitionList is produced by `::: {.definition-list}`
  fenced syntax (NOT Pandoc `Term\n: def`, which this parser makes a Para).
- **P3.2 setting:** `unlockDepthCursor: z.boolean().default(false)` — the `.default()` is load-bearing:
  a *required* field would make `validatePreferences` `safeParse` fail on old prefs and wipe ALL of a
  user's settings. Additive default-off prefs use `.default()`; the `version` literal is the breaking-change gate.
- **P3.2 gating:** the gate lives in one exported pure helper `computeNestedEditBuffers(unlock, content,
  ast, regen)` (in `ReactPreview.tsx`), called by BOTH hosts. Because PreviewApp passes the
  `regenerateNestedBuffers` import to it on *every* render, any test that strict-`vi.mock`s
  `@quarto/preview-runtime` MUST stub `regenerateNestedBuffers` or it throws "No export defined" (this
  was a 47-test SPA regression — fixed in `b66f898a`; **always verify against the full CI config, not a
  narrow scope**).

**Durable implementation notes for P3.4 (fold into the implementer prompt):**
- **Extract the testable logic as pure functions** — e.g. the AST ancestor-path walk, the clamp, the
  "which child range contains `leafAnchorR0`", the commit-destination builder from `editTargetRef.current`
  — and test those directly. Precedent: P3.2's `computeNestedEditBuffers`. Keeps tests off fragile
  component-driving, which is where vacuous jsdom tests breed.
- **Be explicit about what jsdom can't verify** (real key chords + native-conflict, soft-wrap
  last-visual-line, chip geometry): defer them to the P3.5 Playwright pass and *say so* — do not write a
  jsdom test that simulates them and asserts success (project end-to-end-verification policy).
- **Verify at full scope** — `test:ci` + `test:integration` + `build:all`, never a narrow file/subset. A
  narrow run hid a 47-test SPA regression this session (`b66f898a`).

---

## 0. FIRST: confirm the current state

```bash
cd /Users/gordon/src/q2/.worktrees/block-editing
git log --oneline -25
cd ts-packages/preview-renderer && npm run test && npm run test:integration && npm run typecheck
```
Branch `feature/block-editing-improvements`. **Phase 1 + Phase 2 (2.1–2.5) are complete**, including
the self-heal design-bug fix (commits `26a4ed8b` + `2e6e1133`: KEEP works, commit-on-drop guarded).
**Plan 4 (section editing) was cancelled + expunged** (commit `efb39382`) — do not look for it.
One Phase-2 item is **deferred** (not blocking): the collapsed-region self-heal drop (needs Playwright;
tracked in the plan + a `PreviewRoot.tsx` comment). A **test-binding audit** of the Phase-2
behaviors runs in a separate detached worktree `.worktrees/block-editing-audit` (it reverts production
in isolation, so it won't disturb this branch) — do not touch it; let its findings land via its own branch.

---

## 1. How we work (process + testing rules — keep this rigor; it has paid off repeatedly)

**Skill:** `superpowers:subagent-driven-development`. Per plan task:
1. **Implementer** — a fresh `Agent` (`general-purpose`, **model `sonnet`**) with a *fully
   self-contained* prompt (paste curated task + context + file:line refs + commands; don't make it
   read the plan). TDD.
2. **Spec-compliance reviewer** (sonnet) — independent, "don't trust the report", read the diff, run
   tests. (Small leaf tasks: spec+quality can be one combined review.)
3. **Code-quality reviewer** (sonnet) — after spec passes.
4. Implementer fixes; re-review until clean.
5. Check off the plan checkbox(es) + `git commit` per task. **NEVER push** (`git push` is denied;
   the user approves pushes).

**Tracking surface:** plan checkboxes. `CURRENT.md` is a symlink — Edit refuses symlink writes; edit
the real file `2026-06-11-block-editing-improvements.md`.

**Commits:** conventional, scope `block-editing`. **hub-client changes need a `hub-client/changelog.md`
entry** (two-commit workflow: code commit, then changelog commit with the hash) — Phase 3 §3.2 touches
hub-client.

### ⚠ TESTING RULES — read before writing ANY test (this is the lesson of this whole session)

We repeatedly hit **"test theater"**: subagents, facing a hard-to-mount component + a jsdom that has
no layout, wrote harness components that *reimplemented* the production logic and asserted against the
copy. Those tests pass even when production is broken/missing. They hid **three real production bugs**
(self-heal KEEP unreachable; `isOnLastVisualLine` integer-rounding; 2-tile nav guard) and one
silently-reverted wiring. Every time we converted a test from theater→real, it found a bug. So:

1. **Name the test's SUBJECT and exercise it for real.** An integration test MUST mount the real
   component (`import { PreviewRoot } from './PreviewRoot'`) or call the real exported function. You
   MAY mock the *environment* (rects, geometry, `setAst` spy, network). You MUST NOT reimplement the
   *logic under test*. If you find yourself writing a local `executeLanding`/`requestMove`/effect
   body in a test, STOP — that's theater. Reference exemplars: `p2-4-real.integration.test.tsx`,
   `p2-3b-real.integration.test.tsx`, `p2-4d.integration.test.tsx`.
2. **Match the test TIER to where the risk lives:**
   - *logic-dominated* behavior (byte arithmetic, content-verify, re-anchor, the Rust regen) →
     pure-function unit test or jsdom real-`PreviewRoot` integration. Cheap and real.
   - *environment-dominated* behavior (soft-wrap geometry, real CSS coincidence, layout) → jsdom is
     nearly worthless no matter how faithful the mock; **only Playwright is a real test.** P2.5b
     proved it: two bugs the jsdom tier *structurally could not* catch.
   For Phase 3: the Rust `regenerate_nested_buffers` is logic (Rust unit tests); the depth-key
   bindings + breadcrumb geometry are environment (Playwright).

**Tests / commands:**
- `ts-packages/preview-renderer`: `npm run test` (unit `*.test.ts`), `npm run test:integration`
  (`*.integration.test.tsx`), `npm run typecheck`. **Run directly — never pipe through tail/head.**
- Phases 1–2 are Rust-free. **Phase 3 touches the WASM leg** (`crates/pampa`,
  `crates/wasm-quarto-hub-client`): `cargo nextest run -p pampa` + full `cargo xtask verify`
  (NOT `--skip-hub-build`). For a live `q2 preview` check, the WASM does NOT auto-rebuild — run
  `cd hub-client && npm run build:wasm` → `cargo xtask build-q2-preview-spa` → `cargo build --bin q2`.
- e2e (Playwright): `hub-client/e2e/` (real hub on :3031 + iframe + Automerge; `npm run test:e2e`).
  **Phase 2 is Rust-free so you can skip `build:wasm`** for Phase-2-only e2e: `cd ts-packages/preview-renderer && npm run build`
  → `cd hub-client && VITE_E2E=1 npm run build` → `npx playwright test <spec>`. Phase-3 e2e DOES need
  a fresh WASM. q2-preview-spa/e2e spins up a real `q2 preview` binary (used by `edit-cell-sizing`).
  Per CLAUDE.md, the real binary is `cargo run --bin q2 -- preview …` (Q2, never the system `quarto`).

---

## 2. State — DONE (run `git log` for exact hashes; all on `feature/block-editing-improvements`)

✅ **Phase 1** active-region bug fix — a click inside the open editor no longer climbs to the parent
(shared `ctx.activeEditRegionRef` on the editor's inner wrapper; `onPointerUp` guard).
✅ **P2.1** `byteLineMap.ts` (UTF-8 byte↔0-based line) + `r[0]`-uniqueness regression over a real
pampa fixture (`__fixtures__/r0-uniqueness.{qmd,ast.json}`).
✅ **P2.2** `lockedTiles.ts` — locked resolution (prefixing-atomic dominates, else coincidence climb),
`enumerateLockedTiles`, `isVisibleTile`, `rectsCoincide`; `activate` resolves to the locked tile;
roving-tabindex navigates locked tiles.
✅ **P2.3a** byte-offset identity (`editTarget={anchorR0,anchorR1,anchorSlice,contentHeight,boxStyle}`);
controlled `EditTextarea` via root `editDraftRef`+local state (survives remount); dirty guard
(`normalizeLineEndings(draft).trimEnd() !== anchorSlice`); empty→cancel; IME; match by
`anchorR0 === resolved.sourceEntry.r[0]`.
✅ **P2.3b** self-heal + helpers `tileForAnchorR0`, `findReanchorCandidate`. The KEEP path had a
critical bug (theater hid it) — **now fixed** (`26a4ed8b` + `2e6e1133`); KEEP/DROP/commit-guard are
real-`PreviewRoot`-tested. Collapsed-region drop deferred (see below).
✅ **P2.4a** `caretGeometry.ts` — mirror-div `isOnFirst/LastVisualLine` + pure `getLogicalColumn`/`placeCaretAtColumn`.
✅ **P2.4b** arrow-nav move machine: `requestMove`, `pendingLanding` (discriminated `activate`|`focus`),
`executeLanding`, reland `useLayoutEffect` (render-input-keyed) + 250ms timeout fallback; destLine
projection (down=L0+n, up=L0); `captureEditTarget`/`measureTileBox` shared with `activate`;
caret-on-arrival via `pendingCaretRef`; bare-arrow-only guard.
✅ **P2.4c** plain-commit focus-restoration (`intent:'focus'` landing; Esc uses the timeout fallback).
✅ **P2.4d** click-switch — classify in `onPointerDown`; dirty source → commit + projected reland on B.
**Its production wiring had been silently reverted and was masked by theater tests** — restored +
real-tested + projection off-by-one fixed.
✅ **P2.5a** real-`PreviewRoot` coverage for the move/reland/focus machine; retired the P2.4b/c
reimplementation harnesses; binding verified.
✅ **P2.5b** Playwright e2e (`hub-client/e2e/q2-preview-block-nav-p2-5b.spec.ts`, 13 tests). **Found +
fixed two real browser-only bugs** (now reviewed + jsdom-regression-guarded): `isOnLastVisualLine`
integer-`scrollHeight` (2px tolerance in `caretGeometry.ts`); `requestMove` 2-tile no-op (guard
`<=1`→`===0` in `PreviewRoot.tsx`).

✅ **Self-heal design bug — FIXED (`26a4ed8b` + `2e6e1133`).** Retiring the theater had exposed that the
headline data-integrity feature was broken: **Bug 1** KEEP unreachable (the Step-2 visibility check
used `tileForAnchorR0`, which can never find the active editor — while editing it's a textarea wrapper
with no `data-block-pool-id` → always "hidden" → drop); **Bug 2** the unmounting textarea's `onBlur`
committed the stale draft (corruption). **Fix shipped:** removed the broken tile-based check (KEEP
works), and hoisted an active-target guard to the top of `commitIfDirty` so a stale/unmounting
textarea no-ops (no commit, no spurious cancel). The principled collapsed-region drop (visibility via
`activeEditRegionRef` after the re-anchor remount) is **deferred to Playwright** (jsdom has no layout)
— rare case, tracked. **Lesson for Phase 3: the active editor is a wrapper, not a tile — never judge
its visibility via the tile set.**

**Structural note:** `PreviewRoot` was **extracted from `entry.tsx` into its own `PreviewRoot.tsx`**
(so it's mountable in tests). `entry.tsx` now imports it. All the edit-state machinery (editTarget,
the effects, the request* callbacks, the context provider) lives in `PreviewRoot.tsx`.

---

## 3. Curated technical facts (don't re-derive)

**Coordinate systems (critical):**
- Source offsets are **UTF-8 byte offsets**; `content` (= `props.renderedContent`) is a UTF-16 JS
  string; `sliceBytes(content,r0,r1)` (`utils/sliceSource.ts`) slices by bytes; `byteLineMap` works in
  byte space. The textarea draft / caret column work in JS-string space — keep visual-line geometry,
  logical-line column, and byte offsets distinct.
- `anchorR0` lives in the pool `r` space, which **equals** the untransformed `sourceEntry.r` (value-keyed
  correspondence: `resolveSource` returns `sourceEntry = pool[node.s]`, so
  `pool[tile.s].r[0] === resolved.sourceEntry.r[0]`). Capture at click time from `pool[s].r`; match at
  render time against `resolved.sourceEntry.r[0]`.
- `data-block-pool-id` holds the pool **index `s`** (reassigned every render), NOT `r[0]`. **While a
  block is being edited it is a textarea WRAPPER with NO `data-block-pool-id`** (Phase-1 fix) —
  tracked only by `activeEditRegionRef`. This is exactly the trap behind the self-heal KEEP bug:
  `tileForAnchorR0`/`enumerateLockedTiles` scan pool-id tiles and so *never* see the active editor.
- Assumption B: block/block `r[0]` distinct; a block and its first *inline* child can share `r[0]`
  (dispatcher matches block nodes only; self-heal's `find` returns the block first; `anchorSlice`
  content-check is the arbiter).
- CRLF: pampa parses CRLF natively (offsets include `\r`); textarea LF-normalizes → normalize **sliced
  strings** (`normalizeLineEndings`), NEVER the `content` buffer.

**Module map (`ts-packages/preview-renderer/src/q2-preview/`):**
- `lockedTiles.ts` — `resolveLockedTile`, `enumerateLockedTiles`, `isVisibleTile`, `rectsCoincide`,
  `tileForAnchorR0(host,pool,r0,{exactOnly?})`, `findReanchorCandidate`, `captureEditTarget`,
  `measureTileBox`. Shared geometry/identity primitives — reuse in Phase 3.
- `byteLineMap.ts` — `buildByteLineMap(content)` → `{lineOf,lineStart,lineCount}` (0-based, byte space).
- `caretGeometry.ts` — `isOnFirst/LastVisualLine` (mirror-div; mock in jsdom), `getLogicalColumn`,
  `placeCaretAtColumn`.
- `useBlockEditHover.tsx` — delegated host handler; `activate(el)` (resolve locked tile + capture
  identity + measure box + seed `editDraftRef` + setEditTarget); onPointerDown click-switch
  classify; onPointerUp active-region guard.
- `dispatchers.tsx` — `Block`/`CustomBlock`, `isBlockEditTarget` (matches anchorR0), `EditTextarea`
  (controlled value, dirty guard, IME, commit, onKeyDown move trigger, mount caret placement,
  blur click-switch/focus-restore + the commit guard).
- **`PreviewRoot.tsx`** — owns `editTarget` state, `editDraftRef`, `editTargetRef`, `activeEditRegionRef`,
  the **self-heal effect**, the **reland effect** + `pendingLandingRef`/`pendingCaretRef`/`fallbackTimerRef`,
  `requestMove`/`requestFocusRestore`/`requestClickSwitch`/`cancelPendingLand`, `previewHostRef`
  (display:contents wrapper scoping tile queries), and provides `PreviewContext`. **This is the
  component your Phase-3 tests must mount.**
- `entry.tsx` — thin; imports `PreviewRoot`, owns module-top side effects + the `setAst` wiring.
- `PreviewContext.tsx` — context interface (editTarget, editDraftRef, editTargetRef, request* methods,
  resolveSource, pool, content, editingDisabled, …).
- `sourceIndex.ts` — `buildSourceIndex`, `serializeSourceEntry` (`"t:r0-r1:d"`), `ResolvedSource`.

**Controlled-value architecture:** draft lives in root ref `editDraftRef`, seeded ONLY at fresh-open
(activate/reland), NOT in `setEditTarget` (so self-heal re-anchor preserves the draft). `EditTextarea`
mirrors it in local state. Do NOT put the draft in `PreviewContext` (per-keystroke whole-doc re-render).

**pendingLanding machine:** one reland mechanism (`executeLanding` + render-input-keyed `useLayoutEffect`
+ 250ms timeout fallback for byte-identical commits) serving `intent:'activate'` (move/click-switch)
and `intent:'focus'` (plain commit). Self-heal is a *sibling* effect (runs when the editor is still
open = collaborator re-render); reland runs when it's closed (your own move/commit). `fromFile`
cancels a stale landing on file switch.

---

## 4. Phase 3 — "Depth cursor (nested blocks)" unlock (flagged, default-off)

The default (Phase 2 / "locked") edits a whole blockquote/list as one clean-slice buffer. Phase 3 is
the opt-in power-user mode: descend into nested prefixing containers, edit each nested block cleanly
(via AST-regenerated buffers so no `> `/indent pollution), with depth keys + a breadcrumb. Gated
behind a default-off setting so the expensive WASM regeneration path is unreachable by default.
(Decision recap: blockquotes/lists are *atomic* in the default; only the outermost prefixing container
has a clean byte-slice, so descending requires regeneration — hence the flag. See §5.)

**Decomposition:** P3.1 (Rust `regenerate_nested_buffers` + WASM + JS wrapper), P3.2 (setting +
both-host threading), and P3.3 (depth behavior + regenerated-buffer commit) are **DONE** — see the
START-HERE commits. The remaining work (self-heal-on-write → P3.4 breadcrumb → P3.5 e2e) is in the
**"NEXT SESSION — do these in order"** block above; the full TDD checklist is in the plan's Phase 3
section + the "Self-heal on write" subsection.

**Status:** P3.1 + P3.2 + P3.3 done and green (see the START-HERE block at the top for commits,
decisions, and caveats). **Next: "Self-heal on write" (verify-then-fix) → P3.4 (breadcrumb) →
P3.5 (Playwright + WASM e2e).** See the "NEXT SESSION — do these in order" block at the top.

---

## 5. Decisions already made (don't re-litigate)
- **Blockquote/list atomic in locked (default) mode** (confirmed w/ user 2026-06-13): click anywhere
  selects the whole *outermost* prefixing container. Reason: only the outermost has a clean byte-slice;
  inner targets' slices carry the outer `> `/indent — which is *why* Phase 3 (descent) needs AST
  regeneration, gated behind the flag. Full per-layer descent is the Phase-3 unlock.
- **Controlled value via root ref + local state**, not whole-doc memoization.
- **Self-heal visibility must use the active editor's wrapper (`activeEditRegionRef`), never
  `tileForAnchorR0`** — the active editor is not a pool-id tile while edited. (The fix.)
- **Commit guard:** a textarea must not commit unless it's still the active target (`editTargetRef`),
  so an unmount-triggered blur can't write a stale draft.
- **Plain-commit focus-restoration** folded into `pendingLanding{intent:'focus'}`.
- **Click-switch dirty case projects the destination** and relands post-commit-re-render.
- **No test theater** — see the Testing Rules in §1. This is the standing rule for Phase 3.
