# Hand-off: block-editing execution (Phase 2 done → Phase 3)

**Updated:** 2026-06-13 (Phase 2 done; **Phase 3 P3.1 + P3.2 done and green; P3.3 next** — see the
START-HERE block below). **Plan:** `claude-notes/plans/2026-06-11-block-editing-improvements.md`
(symlinked `CURRENT.md`). Read that plan, then this file — this is the *execution* companion: how we
work, what's done, the testing rules we learned the hard way, and curated facts so you don't re-derive
them.

---

## Phase 3 progress — START HERE (updated 2026-06-13, end of impl session)

**P3.1 + P3.2 are DONE and fully green. P3.3 is next.** This block supersedes the
"warming up / not started" framing above.

**Commits (on `feature/block-editing-improvements`):**
- **P3.1** (Rust `regenerate_nested_buffers` + WASM export + JS `regenerateNestedBuffers`):
  `f5cb3132`, `8a51bb92`.
- **P3.2** (setting + both-host threading + gating + fixes): `7f14e5ed`, `17f9b196`,
  `aae25bf0`, `dff074da`, `c96f06a0`, `8af69988`, `b66f898a`.

**Verified green (full scope):** pampa 3943 · preview-renderer 305 unit + 353 integration ·
hub-client 755 (`test:ci`) · SPA 25 unit + 75 integration (`test:integration`) · `npm run build:all` ·
all typechecks. Gating fail-on-revert independently re-verified (remove `!unlock` → reddens).

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
- **PreviewContext now EXPOSES `unlockDepthCursor` + `nestedEditBuffers`** but nothing CONSUMES them yet
  (no leaf-click/depth-key/draft-seed/commit). That consumption is P3.3.

**P3.3 — read these BEFORE wiring anything:** plan §3b/§3c **and** the new **"Self-heal on write"**
subsection (plan ~`:647`, immediately before `## Phase 3`). The load-bearing rule: the §3c
regenerated-buffer commit is a NEW write path — source its destination from the **live**
`editTargetRef.current` (`{t:0, r:[anchorR0, anchorR1], d:0}`, no-op if null), **never** a per-render
`resolved.sourceEntry` closure. Do NOT refactor the existing `EditTextarea` commit (separate Phase-2
follow-up); just don't inherit the closure pattern. Then: seed `draft = nestedEditBuffers?.[siKey] ??
normalize(sliceBytes(...))` at `activate`; leaf resolution (no coincidence-climb / no prefixing-atomic);
`leafAnchorR0` scalar for "in"; depth keys (**macOS `Cmd+Ctrl+←/→`**, **Win/Linux `Alt+Shift+←/→`**),
clamp at keypress, AST path derived each render (no stored depth). Tier: jsdom real-`PreviewRoot` for
logic; the key-bindings + breadcrumb geometry are environment → Playwright (P3.5).

**Testing methodology note (corrected provenance):** fail-on-revert is NOT part of the superpowers
skills (those prescribe TDD fail-*first* + don't-trust-the-report reviews). It is a project-local
discipline (hand-off §1) that exists because fail-first and reading-reviews both miss *theater*. A
reusable write-up now lives at `~/.claude/skills/fail-on-revert/SKILL.md` (per-user; reviewer-triggered
for non-local binding; both-prove + independent re-verify). That skill is grounded in real failures but
is **not yet GREEN-verified** with a controlled non-local subagent scenario — finish that before relying on it.

**Loose end:** the fail-on-revert audit's `useBlockEditHover.integration.test.tsx` hardening (adds pool
entry 99 so the Phase-1 climb-guard test reddens on revert) was reverted out of this worktree as a
stray audit artifact — ensure it lands via the audit's own branch; it's a real coverage fix.

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
tracked in the plan + a `PreviewRoot.tsx` comment). A **fail-on-revert audit** of the Phase-2
behaviors runs in a separate detached worktree `.worktrees/block-editing-audit` (it reverts production
in isolation, so it won't disturb this branch) — do not touch it; let its findings land via its own branch.

**Phase 3 has started: P3.1 + P3.2 are done and green (see START-HERE above).** P3.3 is next.

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
2. **Fail-on-revert is mandatory for every behavioral/bugfix test.** After green, revert ONLY the
   production hunk, confirm the test goes red, paste the failure, restore. "Failed before I wrote the
   test" is NOT enough (the harness didn't exist before either). The test must fail because
   *production* is wrong.
3. **Match the test TIER to where the risk lives:**
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
real-`PreviewRoot`-tested with fail-on-revert. Collapsed-region drop deferred (see below).
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
reimplementation harnesses; fail-on-revert verified.
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

Entry points (verified present): `crates/pampa/src/writers/qmd.rs::write_single_block` (~:2392, public,
fresh context = no `> `/indent prefixer); WASM `crates/wasm-quarto-hub-client/src/lib.rs::apply_node_edit`
(~:2938) as the export model; `ts-packages/preview-runtime/src/wasmRenderer.ts` for the JS wrapper.

**Suggested decomposition (each a review-gated subtask; sonnet implementers):**
- **P3.1 (Rust/WASM)** — `pampa::regenerate_nested_buffers(content, untransformed_ast_json) -> String`
  (JSON `{siKey: cleanQmd}`) via `write_single_block`→`trim_end`. Restriction: block has a prefixing
  ancestor (fenced `:::` excluded) AND is multi-line; no reachability filter in Rust (over-inclusion
  harmless). `siKey` = `serializeSourceEntry` format `"0:<r0>-<r1>:0"`. WASM export mirroring
  `apply_node_edit`; JS wrapper `regenerateNestedBuffers` in `wasmRenderer.ts`. **Logic-dominated →
  Rust unit tests** (siKey contract; source fidelity for shortcode/math/raw/code-blocks; offset-domain).
- **P3.2 (setting + threading — BOTH hosts)** — `unlockDepthCursor:boolean` (default false), two
  sources feeding the same host-agnostic iframe behavior:
  - **hub-client:** in `hub-client/src/services/preferences/schema.ts` (+ DEFAULT_PREFERENCES); checkbox
    in `SettingsTab.tsx` (mirror `errorOverlayCollapsed`); read via `usePreference` (reactive). Reuse
    the proven `editingDisabled` iframe wire; ADD the missing host leg `ReactPreview → ReactRenderer →
    Q2PreviewIframe` (hub-client threads no such flag today).
  - **SPA:** a `?depthCursor=1` URL query param in `PreviewApp.tsx`, read at load, passed **directly**
    to `Q2PreviewIframe` (the SPA bypasses `ReactRenderer`). Read-at-load = no live toggle (fine).
  Both pass the **optional** `unlockDepthCursor` + `nestedEditBuffers`; a host with its opt-in off
  omits them → iframe locked (zero-touch). (Revised 2026-06-13: the SPA depth cursor is in scope —
  ~20 lines in `PreviewApp.tsx`, reusing the host-agnostic iframe behavior; no Rust/iframe changes.)
- **P3.3 (behavior when on + buffer gating)** — click resolves to the **leaf** (no coincidence-climb,
  no prefixing-atomic); identity = same `anchorR0` + a `leafAnchorR0` scalar for "in"; depth keys
  mutate anchorR0 along the AST path (**macOS `Cmd+Ctrl+←/→`**, **Win/Linux `Alt+Shift+←/→`**), clamp
  at ends at key-press; path derived from the AST each render (no stored depth/path). Gate the buffer
  table in a `useMemo` keyed on `[unlockDepthCursor, renderedContent, untransformedAstJson]` (else a
  module-level shared empty object for referential stability). Seed `draft` from
  `nestedEditBuffers?.[siKey] ?? slice` at activate. Plumb `nestedEditBuffers` on the UPDATE_AST
  payload from **both** hosts.
- **P3.4 (breadcrumb floating toolbar)** — `stopPropagation` on its own pointer handlers; absolutely
  positioned above the active surface's top-left; AST-derived ancestor path with `◀`/`▶` in/out
  buttons (+ platform-shortcut tooltips, the touch affordance); shown only when the flag is on.
- **P3.5 (e2e)** — setting end-to-end; depth keys; WASM round-trip (edit nested blockquote child →
  clean commit, no `> `); **cross-platform Playwright** for the bindings (verify native word/line-select
  still work) — environment-dominated, so Playwright is the real test.

See the plan's Phase 3 section for the full TDD checklist.

**Status:** P3.1 + P3.2 done and green (see the START-HERE block at the top for commits, decisions,
and the P3.3 instructions). P3.3 → P3.4 → P3.5 remain.

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
