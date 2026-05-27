# Reactji authorship-aware add/remove for q2-preview (`comment.tsx`)

**Worktree:** `.worktrees/provenance-reactji-demo/` on `provenance-reactji-demo`, branched off `feature/provenance`.
**Fixture:** `crates/quarto/tests/playwright-fixtures/q2-preview/render-components-comment/{render-components-comment.qmd, comment.tsx, _quarto.yml}` (copied verbatim from `~/docs/demo-playground/gordon/render-components/`; moved out of `smoke-all/` per the playwright-fixtures distinction documented in `claude-notes/instructions/testing.md`).
**Sister pattern:** `crates/quarto/tests/playwright-fixtures/q2-debug/{render-components-reactji.qmd, reactji.tsx}` + `hub-client/e2e/q2-debug-render-components.spec.ts` (May 12; moved to `playwright-fixtures/` alongside this work).

## Goal

Two ends:

1. **Diagnose** whether authorship / attribution data reaches the front-end render-components TSX environment for `q2-preview` (not just `q2-debug`).
2. **Implement** authorship-aware reactji toggling. Clicking an emoji bubble whose count includes a contribution authored by *me* removes my contribution; otherwise adds one. Today the bubble unconditionally adds.

We don't yet know whether (2) is reachable without plumbing changes. The first phase is an investigation phase that produces the "answer to (1)" as a failing or red-passing e2e snapshot. The shape of the rest of the plan depends on what we find there.

## Background — what's already in place on `feature/provenance`

From the survey at the start of this session (file paths + line numbers):

- **Wire contract (Plan 5, shipped).** `pampa/writers/json` emits `astContext.attribution` (sparse `[{ s, actor, time }, …]` keyed by source-info pool index) and `astContext.attributionActors` (`{ actor → { display_name, color } }`). Off-path byte-identicality is preserved when attribution is disabled. See `crates/pampa/tests/attribution_json_wire_test.rs:19-23, 89-151`.
- **Producer side (hub-client).** `hub-client/src/hooks/useAttribution.ts` replays the Automerge run-list, maps char→byte offsets, resolves identities (profile metadata or fallback formula), and produces the JSON payload for `parseQmdToAstWithAttribution(content, payload)`. Threaded into `hub-client/src/components/render/ReactPreview.tsx:283-311, 349, 388, 409` for **both** q2-preview and q2-debug — the comment in `useAttribution.ts:16` still says "q2-debug renderer to consume" but ReactPreview.tsx already drives q2-preview through the same call site.
- **Consumer-side hook.** `ts-packages/preview-renderer/src/framework/AttributionLookupContext.tsx` defines `useNodeAttribution(node)` returning `{ actor, name, color, time } | null`. Already wired through `framework/Ast.tsx` for the q2-debug overlay. q2-preview's `PreviewDocument.tsx:199` comment confirms the context is plumbed for the format.
- **Render-components plumbing (q2-preview).** `ts-packages/preview-renderer/src/q2-preview/entry.tsx:97-111` exposes `__Q2_PREVIEW_RENDERER__` (`renderChildren, renderNode, renderSlot, Node, Block, Inline, previewRegistry, extractMeta*, *toPlainText, PreviewTitleBlock`). User TSX is loaded via `LOAD_CUSTOM_COMPONENTS` message; user components receive `NodeArgs<T> = { node, onNavigateToDocument?, setLocalAst }` (`ts-packages/preview-renderer/src/framework/types.ts:142-146`).
- **Iframe AST payload (q2-preview).** `Q2PreviewIframe.tsx:163-194` posts `UPDATE_AST` with `{ astJson, currentFilePath, assetManifest, projectFilePaths, pendingAnchor, pendingAnchorEpoch }`. The `astJson` string already carries `astContext.attribution*` when attribution is on (the WASM call output). **No** current-user identity is in the payload.
- **Current-user identity.** `getActorId()` from `@quarto/preview-runtime` returns the Automerge actor id. It is consumed in the parent app (e.g. `ReplayDrawer.tsx:126`), **but not forwarded into the q2-preview iframe**, and **not exposed to user TSX**.

## Three known gaps between "data reaches the iframe" and "user TSX can use it"

These are the gaps the implementation has to close. Each is its own decision point in the plan.

1. **`useNodeAttribution` is not on `__Q2_PREVIEW_RENDERER__`.** User TSX (`comment.tsx`) imports come from `window.__Q2_PREVIEW_RENDERER__`, which today exposes the renderer's helpers but not the attribution hook or context. Adding `useNodeAttribution` (and possibly `AttributionLookupContext` for advanced consumers) to that surface is a one-line change with a small surface-stability commitment.

2. **The attribution lookup is keyed by source-info pool index (`node.s`).** Original `[>> emoji]{.quarto-edit-comment}` spans come from the qmd source and carry an `s`. Reactjis added at runtime via `setLocalAst` are plain JS literals with no `s`. So for runtime-added reactjis, `useNodeAttribution(span)` will return `null` — they have no authorship until the document round-trips through the writer and parser again. This is a correctness subtlety: "I just added one" needs to be tracked through the live editing session in addition to looking at the AST.

3. **Iframe doesn't know who "me" is.** Even with `useNodeAttribution` returning `{ actor: "alice", … }`, the TSX has no way to check `actor === me`. We need to forward the current actor id through the iframe boundary. Options:
   - Piggyback on `UPDATE_AST` payload as `currentActor`.
   - Embed it in `astContext` (the WASM build can call out to a "current actor" hook on the producer side — already an option since `useAttribution.ts` knows the actor map; we'd need a designated key).
   - A separate `SET_CURRENT_ACTOR` message — symmetric with `UPDATE_THEME`.

   We will pick one in Phase 2 once we've seen the failing-test output.

## Phase 0 — fixture sanity (do this first)

The fixture as copied uses:

- a `kanban` div which currently has no matching `kanban.tsx` declared in `render-components:`. That's intentional in the demo (renders as a plain Div), but for an e2e test we may want a deterministic minimal fixture.
- a tldraw `iframe` in a `=html` raw block.
- a math display block followed by `[>> 👀]{.quarto-edit-comment}`.
- a multi-reactji header (`[>> 🤔][>> 🤔][>> 🔥][>> 🔥]`) — the **anchor for goal (2)**: two 🤔s and two 🔥s on the H1, where authorship determines what a click does.

Plan-0 work:

- [x] Add a small synthetic `_quarto.yml` to the fixture dir matching the q2-debug pattern (project type, no theme overrides). *Done — minimal `project: title: ...`.*
- [x] Decide whether to keep the kanban + tldraw blocks (good demo realism) or strip them (cleaner test surface). *Keep — Q4 of the design questions didn't push back on this.*
- [x] Confirm the fixture renders end-to-end under `q2 render` locally before writing any e2e test. *Done — `cargo run --bin q2 -- render crates/quarto/tests/playwright-fixtures/q2-preview/render-components-comment/` succeeded. Output HTML had `<span class="quarto-edit-comment">` for all reactjis (`🤔×2 🔥×2 😸 👀`) and the kanban div was present. Render artifacts cleaned up; only source files in the fixture dir.*

## Phase 1 — investigation e2e ("does authorship reach the user TSX?")

Goal: produce a Playwright spec that **proves** whether `astContext.attribution` makes it through to user TSX *today*, with no plumbing changes. The first version is allowed (encouraged) to fail at the assertion stage — the failure shape tells us which gap we hit first.

- [x] Add a diagnostic export in `comment.tsx`. Shipped as `window.__COMMENT_DIAG__ = { surfaceKeys, hasUseNodeAttribution, hasUseCurrentActor, me, blocks }`, with a `Diagnostic` sub-component conditionally mounted inside `CommentWrapper` when both hooks are present on the surface. The hook-availability gate keeps the unconditional hook calls inside `Diagnostic` from throwing when the hooks are absent.

  **Implementation note (kept-vs-removed debug surface):** during Phase 2 diagnosis the diagnostic export grew several scratch counters (`blockTrace`, `gateChecks`, `gateLastDiagAvailable`, `gateLastCommentLen`, `gateLastReactionSpansLen`, `diagnosticRendered`, `diagnosticEffectFired`, `updateAstCalls`, `lastPayloadCurrentActor`). Those were removed once their question was answered. The surface fields listed above plus `addReactionCalls` (added in Phase 2c, see below) are the **public test contract** — the spec asserts on them. If you re-add a temporary counter during future debugging, prefix it `debug…` or strip it before commit so the contract surface stays small.
- [x] New spec at `hub-client/e2e/q2-preview-render-components-comment.spec.ts`, modeled on `q2-debug-render-components.spec.ts:37-76`. Five soft assertions cover Gap 1, Gap 3a, Gap 3b, the Diagnostic mount evidence, and per-span attribution resolution. Behavioural assertion (click-to-remove) deferred to Phase 2c.
- [x] Run the test. Captured failure mode (2026-05-25, against worktree HEAD at `2bf92664` baseline):

  ```
  COMMENT_DIAG = {
    surfaceKeys: ["renderChildren", "renderNode", "renderSlot", "Node", "Block",
                  "Inline", "previewRegistry", "extractMetaString", "extractMetaBool",
                  "extractMetaStringList", "inlinesToPlainText", "blocksToPlainText",
                  "PreviewTitleBlock"],
    hasUseNodeAttribution: false,
    hasUseCurrentActor:    false,
    me: null,
    blocks: []  // Diagnostic never mounted (hook-availability gate failed)
  }
  ```

  All five soft assertions failed:
  - **Gap 1 confirmed**: `useNodeAttribution` is not in the surface keys.
  - **Gap 3a confirmed**: `useCurrentActor` is not in the surface keys.
  - **Gap 3b confirmed**: `me === null` (no producer for currentActor in UPDATE_AST today).
  - **Diagnostic mount-gate stayed shut**: `blocks` is empty because hooks weren't reachable.
  - **Per-span attribution unreachable**: not testable until the mount-gate opens.

Phase 1 outcome: all three gaps from the "Background — what's already in place" section are real on `feature/provenance` today. The diagnostic + spec stay in the tree as regression coverage for the Phase 2 plumbing.

## Phase 2 — close the gaps (only if Phase 1 fails)

Each step has a TDD pair: write the test (or extend Phase 1's diag) → confirm red → implement → confirm green.

### 2a. Expose `useNodeAttribution` on `__Q2_PREVIEW_RENDERER__`

- [x] Test: from inside `comment.tsx`, `window.__Q2_PREVIEW_RENDERER__.useNodeAttribution` is a function. *Covered by `q2-preview-render-components-comment.spec.ts`'s `diag.hasUseNodeAttribution` soft assertion (passes post-2a).*
- [x] Implementation: add `useNodeAttribution` + `AttributionLookupContext` to the global surface at `ts-packages/preview-renderer/src/q2-preview/entry.tsx:97-122`.
- [x] q2-debug parity: skipped (Q4 decision — q2-preview only this session).

### 2b. Forward current actor id into the iframe

- [x] Test: from inside `comment.tsx`, `useCurrentActor()` returns the value the parent posts. *Covered by `diag.me === TEST_ACTOR_ID` assertion in the spec. The spec injects the actor via `page.addInitScript` + a new `__QUARTO_TEST_ACTOR_ID__` override in `hub-client/src/App.tsx`'s `resolveActorId`, since `getActorId()` is null without auth.*

  **Implementation note (Automerge actor-id hex-format gotcha):** the first version of `TEST_ACTOR_ID` used a human-readable placeholder (`'test-actor-7e1f02a3'`). Automerge silently rejected writes against that actor — `page.goto` landed on the project-list screen rather than entering the editor view, with no console error visible. Switching to a 32-hex-char string (`'e2e7e1f02a30000000000000000007e1'`) fixed it immediately. Any test reusing the `__QUARTO_TEST_ACTOR_ID__` override must use a 32-char lowercase hex id.
- [x] Implementation (option A — piggyback on `UPDATE_AST`, per decision-log Q1):
  - `Q2PreviewIframeProps.currentActor` (new prop) → forwarded into `UPDATE_AST` payload.
  - `UpdateAstPayload.currentActor` consumed in `entry.tsx::updateAst`, passed to `PreviewRoot`, wrapped in `<CurrentActorContext.Provider value={...}>`.
  - New `ts-packages/preview-renderer/src/framework/CurrentActorContext.tsx` provides the context + `useCurrentActor()` hook.
  - Exposed on `__Q2_PREVIEW_RENDERER__` alongside `CurrentActorContext` for advanced consumers.
  - Parent chain: `ReactPreview.tsx:474` (`currentActor={getActorId()}`) → `ReactRenderer.tsx:225` (forwards to Q2PreviewIframe).
  - Long-term follow-up (per decision Q1): move `currentActor` to `astContext.currentActor` (Plan 5 follow-up) to decouple from the iframe wire shape.

### 2c. Authorship-aware click handler in `comment.tsx`

Relies on the CRDT round-trip (see Decision log Q3): `setLocalAst` already writes back through `incrementalWriteQmd` → Automerge content → WASM reparse, so runtime-added spans get `s` + attribution within ~50–150ms. No session-local bookkeeping. Behaviour is also gated on the user-controlled Attribution toggle being **on** (per decision-log Q1, attribution stays opt-in this session).

- [x] Test: `addReaction` is invoked on bubble click and the diagnostic captures `{ me, attributionLookupNull, reactionSpansLen }` for the call. *Covered by `reactji bubble click invokes the Phase 2c addReaction handler` in the spec.*

  **Implementation note (test scope pivot from count assertion → invocation assertion):** the original test ambition was the visible "fall-through to add" outcome: `click → bubble text "🤔2" → "🤔3"`. The offline e2e env's `setLocalAst → incrementalWriteQmd → Automerge content update → WASM reparse → iframe re-render` chain didn't reflect the new count within a 5-second budget — Playwright retried the locator 9 times and saw `"🤔2"` every time, even though `__COMMENT_DIAG__.addReactionCalls` confirmed the handler had run with the right `(emoji, me, attributionLookupNull)` triple. The pre-existing `q2-preview-render-components-write.spec.ts` works around the same offline-mode quirk by only asserting "no console.error". We pivoted to asserting the *invocation context* via `__COMMENT_DIAG__.addReactionCalls` — the strongest claim the offline env can reliably support. The actual count-change observation (and the "remove mine" branch) get verified manually in Phase 3 against an authenticated session.

- [x] Implementation: in `comment.tsx`'s `CommentWrapper`, before legacy push:
  1. `findMineSpan(emoji)` walks `reactionSpans` and returns the first span where `attributionLookup.get(span.s).actor === me`.
  2. If a match exists → `removeSpanByS(span.s)` rebuilds the block without that span, `setLocalAst(newBlock)`.
  3. Else → legacy push path. This fires when `me === null` (no auth) OR `attributionLookup === null` (Attribution toggle off). Both cases land in the test env, where the fall-through is the verified behaviour.

  **Implementation note (`reactionSpans` extraction surprise):** the original spec said "walk `comments` for the block". In practice `Block()` reassigns `comments` mid-function to *exclude* single-emoji reactjis (those get collapsed into a `reactionCounts: Map<emoji, count>` for the bubble UI). The canonical H1-followup paragraph `[>> 🤔][>> 🤔][>> 🔥][>> 🔥]` therefore reaches `CommentWrapper` with `comments.length === 0` — the diagnostic mount-gate `comments.length > 0` never fired despite `blockTrace` showing 4 reactji spans pre-filter. The fix introduced a new `reactionSpans: InlineNode[]` prop that ships from `Block` *alongside* `comments`/`reactionCounts`, carrying the unfiltered reactji Span inlines (with their `s` source-info indexes intact) for both the Diagnostic and `findMineSpan` to walk. Future user-TSX overrides that need per-span access to reactjis (rather than just aggregate counts) should follow the same pattern: pass the unfiltered spans as a sibling prop rather than re-deriving them from filtered `comments`.

- [x] Edge case from Q3: documented in code comments + decision log; not addressed in implementation per decision.

### 2d. Polish the demo UI

- [x] Reactji bubble: when *any* matching-emoji span is attributed to me, render the bubble border in my attribution colour. With Attribution toggle off, `attributionLookup` is null and the bubble keeps its neutral grey — no visual regression.
- [ ] *Skipped this session:* title-tooltip author list. Lower-value polish; can land later.
- [x] Kanban / tldraw / math sections: unchanged.

## Phase 3 — verification, parity, and follow-ups

- [x] `cargo nextest run --workspace` — 9654 tests pass, 196 skipped, 0 failed (48.9s).
- [x] `cd hub-client && npm run test:ci` — 82 hub-client unit/integration tests pass.
- [x] `cargo xtask verify --skip-rust-tests` — full chain (workspace build clean, hub-client `npm run build:all` clean, q2-preview-spa build clean, hub-client production `tsc -b && vite build` clean, no lint warnings). The skip is safe because the previous step already ran the Rust suite.
- [x] `cargo build --bin q2` — re-embeds the freshly-built q2-preview SPA via `include_dir!`, per CLAUDE.md's preview-rebuild chain. Needed so the next `q2 preview` invocation picks up the new iframe surface.
- [x] `npx playwright test e2e/q2-preview-render-components-comment.spec.ts` — 2 tests pass (8.0s). Plumbing assertions verified; `addReaction` invocation verified.
- [x] Static smoke-all parsing check (Phase 0): `cargo run --bin q2 -- render <fixture>` produces the expected `<span class="quarto-edit-comment">` markup for all reactjis.

### Manual verification step (out-of-scope for this session, required for full demo sign-off)

The "remove mine" / "selected border" UX branches cannot be exercised in the headless e2e env — they require:

1. An **authenticated hub session** so `getActorId()` returns a real Automerge actor id (the e2e env injects a placeholder via `__QUARTO_TEST_ACTOR_ID__`, but no attribution data is attached to that placeholder).
2. The **Attribution toggle** flipped on in the replay drawer (opt-in by 2026-05-25 decision log Q1).

Workflow for the manual demo:

```
# 1. Open the fixture in an authenticated hub session.
cd hub-client && npm run dev
# Sign in. Open render-components-comment.qmd from the demo project.
# Or run the standalone preview:
cargo run --bin q2 -- preview crates/quarto/tests/playwright-fixtures/q2-preview/render-components-comment/

# 2. Flip the Attribution toggle in the replay bar.
# 3. Add a fresh 🤔 reactji via the picker. The new bubble should show with
#    your attribution colour as the border (Phase 2d polish).
# 4. Click your 🤔 bubble. After ~150 ms, the count should decrement by 1.
#    Click again: another decrement. The pre-existing 🤔s (not authored by
#    you) remain — clicking after your reactjis are gone adds a fresh one
#    instead of removing somebody else's.
```

Record the observed behaviour + a screenshot in this plan file when the manual pass is done.

### Open follow-ups (logged here; do not block this session)

- Move `currentActor` from the iframe `UPDATE_AST` payload into `astContext.currentActor` (Plan 5 follow-up). Decouples actor refresh from AST cadence and centralises the wire shape. Captured in decision-log Q1.
- Define a stable contract for the user-TSX surface so it isn't a "global window blob". The growth in `__Q2_PREVIEW_RENDERER__` (now 16 keys) is straining the ad-hoc shape.
- Default-on Attribution for q2-preview: deliberately not landed this session (decision-log Q1). Worth revisiting once the demo is exercised — a default-on path makes "live attribution UI" the obvious answer for new comment-aware overrides.
- `useNodeAttribution`/`useCurrentActor` parity on q2-debug's renderer surface (decision-log Q4: skipped this session).

## What we are NOT doing in this session

- Changing the Plan 5 JSON wire contract.
- Rewriting `useAttribution.ts`; it already produces what we need.
- Threading authorship into kanban.tsx or any non-comment override.
- Implementing per-reactji "who clicked" tooltips beyond the title attribute.

## File map (proposed)

```
crates/quarto/tests/playwright-fixtures/q2-preview/render-components-comment/
  _quarto.yml                      (new — Phase 0)
  render-components-comment.qmd    (copied from demo-playground)
  comment.tsx                      (copied; will gain diag export + author-aware toggle)

hub-client/e2e/
  q2-preview-render-components-comment.spec.ts   (new — Phase 1)

ts-packages/preview-renderer/src/q2-preview/entry.tsx
  +exposes useNodeAttribution, useCurrentActor on __Q2_PREVIEW_RENDERER__   (Phase 2a-b)

ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx
  +forwards currentActor in UPDATE_AST payload                              (Phase 2b option A)

ts-packages/preview-renderer/src/framework/CurrentActorContext.tsx (new)    (Phase 2b)
```

## Decision log

- **2026-05-25.** Worktree off `feature/provenance` rather than off a sub-phase plan branch: this work doesn't extend any single Plan-N phase cleanly; it's a demo + plumbing piece that depends on Plans 5 + 6 + 7 all being landed (which they are on `feature/provenance`).
- **2026-05-25.** No beads issue: per user instruction this session. CURRENT.md symlink + plan file are the work-tracking surface.
- **2026-05-25.** Target = `q2-preview` (per fixture frontmatter), not the q2-debug sister. Plumbing for q2-debug parity is opportunistic, not required.
- **2026-05-25.** "Me" = `getActorId()` (Automerge actor id), matching the actor key in `astContext.attribution`. Screen name is presentation only.
- **2026-05-25.** Survey claims (`__Q2_PREVIEW_RENDERER__` missing `useNodeAttribution`; `UPDATE_AST` missing `currentActor`; `useNodeAttribution` keyed off `node.s`; producer wired for q2-preview today) all confirmed against current code at start of session.
- **2026-05-25 (Q1).** Phase 2b channel for current actor id: **option A — piggyback on UPDATE_AST.** Cheapest plumbing; actor id is stable per device so AST-cadence coupling is fine. Option C (inject via astContext) remains the principled long-term home; not now.
- **2026-05-25 (Q2).** User-TSX surface for attribution + current actor: **hooks** (`__Q2_PREVIEW_RENDERER__.useNodeAttribution(node)` and `.useCurrentActor()`). Matches existing useContext-based implementation and the React shape user TSX already uses.
- **2026-05-25 (Q3).** No session-local bookkeeping. The `addReaction`/click flow already round-trips through `incrementalWriteQmd` → Automerge content rewrite → WASM reparse, so a runtime-added span gets its `s` (and attribution) back within ~50–150ms. Click logic for `comment.tsx`: walk `comments`, find any span whose `useNodeAttribution(span).actor === me` → remove; else → add. **Known edge case:** fast double-click inside the round-trip window can fall through to a duplicate add because the just-added span doesn't have `s` yet. Acceptable for the demo; if it ever bites we can debounce the click or reintroduce the session-local Map.
- **2026-05-25 (Q4).** Mirror to q2-debug renderer surface: **no, q2-preview only this session.** q2-debug already has its own attribution overlay path; user-TSX parity there can wait for a real q2-debug demo need.
