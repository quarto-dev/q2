# hub-client `q2-preview`: editable / read-only toggle in the bottom status bar

**Strand:** bd-ew0vak6b
**Status:** implemented 2026-09-09 (commit `93b146c`); strand open pending
review/merge.
**Related:** bd-ov4gqk3m (`q2 preview --allow-edit`, the plan at
`2026-06-10-q2-preview-edit-writeback.md`, which built the `editingDisabled`
plumbing this work reuses); bd-0rsk07il (the Comments toggle in the same bar).

## Overview

`format: q2-preview` in hub-client is always editable. That makes links
hard to follow: the block-edit hover surface treats a mouse click on any
editable block as "open the editor for this block", so a click on a link
inside a paragraph both follows the link *and* activates the block editor
(details under *Why links are hard to click*). `q2 preview` already has the
editable/read-only distinction via `--allow-edit`, and the renderer already
implements read-only mode behind a single flag. hub-client just never sets
it.

The deliverable is an **Edit** pill in the bar at the bottom of the document
view (the replay bar, which already carries the **Authors** pill and the
**Comments** three-way toggle) that turns block editing on and off. The
choice is persisted as a user preference. When editing is off the preview
behaves like a read-only `q2 preview`: no hover outlines, no edit
affordances, links are plain links.

## What the source study found

### The read-only flag already exists end to end inside the renderer

`ts-packages/preview-renderer` implements read-only mode as
`editingDisabled`, introduced for `q2 preview` without `--allow-edit`
(bd-ov4gqk3m):

- `Q2PreviewIframe` takes `editingDisabled?: boolean`
  (`ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx:95`) and
  ships it inside the `UPDATE_AST` payload. The flag is in the effect's
  dependency array, so **flipping the prop re-posts `UPDATE_AST`** — no new
  message type is needed for a live toggle.
- `entry.tsx` unpacks it from the payload and hands it to `PreviewRoot`,
  which puts it on `PreviewContext.editingDisabled`
  (`PreviewRoot.tsx:1574`).
- Consumers: `Para`, `Header`, `BulletList`, `OrderedList`,
  `DefinitionList`, `taskList`, `listBorrow` all fold `!ctx?.editingDisabled`
  into `isEditable`, so no `data-block-pool-id` is emitted; and
  `useBlockEditHover` returns an inert surface (`{ hostProps: {},
  stylesheet: null }`) — no pointer handlers, no affordance CSS, no roving
  tabindex (`useBlockEditHover.tsx:341`).
- Existing tests: "Affordance gate — global editingDisabled" in
  `q2-preview.integration.test.tsx:1145`.

The `q2-preview-spa` is the reference host: it passes
`editingDisabled={!state.allowEdit}` to the iframe
(`q2-preview-spa/src/PreviewApp.tsx:1389`) **and** drops any stray
`setAst` payload in `handleSetAst` when `allowEdit` is false
(`PreviewApp.tsx:609`, "defense in depth").

### The hub-client prop path stops one hop short

The iframe is reached from `Editor.tsx` through three components. Every
sibling toggle already threads this way; `editingDisabled` is simply
absent from the hub-client half:

| Component | `commentsMode` (model) | `richText` (model) | `editingDisabled` today |
|---|---|---|---|
| `Editor.tsx` | `useState`, passed to `ReplayDrawer` + `PreviewRouter` | — | — |
| `PreviewRouter.tsx` | forwarded | — | — |
| `ReactPreview.tsx` | forwarded | `usePreference('richText')` read here (`:508`) | — |
| `ReactRenderer.tsx` | forwarded (`:332`) | forwarded | — |
| `Q2PreviewIframe` | in `UPDATE_AST` | in `UPDATE_AST` | **prop exists, never passed** |

Two precedents for where the state lives: the **Authors** and **Comments**
toggles are session-only `useState` in `Editor.tsx` (treated as
inspection modes), while `richText` / `unlockNestingCursor` are persisted
preferences in `services/preferences/schema.ts` read directly by
`ReactPreview` via `usePreference`. `usePreference` instances sync across
the window through `PREFERENCE_CHANGE_EVENT`, so a toggle in one component
takes effect in a sibling without threading.

### The bottom bar

`ReplayDrawer.tsx` renders the right-hand cluster twice (collapsed and
expanded states), in DOM order `{statusSlot} <AttributionToggle>
<CommentsModeToggle>` (`ReplayDrawer.tsx:376-386` and `:443-453`). Each
toggle renders only when both of its props are supplied. The Authors pill
(`AttributionToggle`) is the closest visual model: a bordered pill with a
dot and a label, `aria-pressed`, a `Tooltip`, `e.stopPropagation()` on
click so the bar's own click doesn't enter replay, and a `disabled` state
for formats that don't support it (`attributionDisabled`, computed in
`Editor.tsx:1436` as "not q2-debug and not q2-preview"). Its styles live in
`ReplayDrawer.css:320-460` (`.replay-drawer__attribution*`,
`.replay-drawer__authors-cell`).

### Why links are hard to click (the mechanism)

`useBlockEditHover.onPointerUp` (`useBlockEditHover.tsx:262`) activates the
block under the pointer on **any mouse pointer-up** whose target is inside
a `[data-block-pool-id]` element. `findEditTarget` is
`target.closest('[data-block-pool-id]')` with no exclusion for anchors.
Link navigation is handled separately by a document-level click handler
(`installLinkHandlers`, `PreviewRoot.tsx:1368`), so a click on a link fires
both: the link handler navigates (or scrolls), and the pointer-up opens the
editor for the paragraph. With `editingDisabled` there are no pool ids and
no pointer handlers, so the click is a plain link click.

(A narrower fix — make `findEditTarget` ignore clicks whose target is
inside an `<a>` — is a separate, complementary improvement to editable
mode. It is deliberately **out of scope** here; see *Out of scope*.)

### One gap to close in the renderer

Nothing in `PreviewRoot` reacts to `editingDisabled` **changing** while an
edit session is open (`editTarget != null`). Today the flag is fixed for
the life of a `q2 preview` session, so this never arose. With a live
toggle, flipping to read-only mid-edit would leave the editor mounted
inside a block that no longer advertises editability. In practice clicking
the pill in the parent document moves focus out of the iframe, which fires
the editor's blur → `commitIfDirty` path, but that is incidental. The plan
adds an explicit effect (decision D6).

## Design decisions (to iterate on)

- **D1 — State: persisted preference, not session-only.** Add
  `previewEditing: boolean` to `UserPreferencesSchema` with
  `.default(true)` so stored prefs from before the key parse cleanly (same
  treatment as `richText`). *Rationale:* read mode is a way of working
  (reviewing, following links) rather than a one-off inspection like the
  Authors overlay, and `q2 preview`'s equivalent is a per-session setting;
  someone who turns editing off wants it to stay off across reloads.
  *Alternative:* session-only `useState` like Authors/Comments — one less
  schema change, but resets on every reload.
- **D2 — Default: editing ON (unchanged).** Existing users see no
  behaviour change until they flip the pill. *Alternative:* default OFF
  would make links work out of the box but would silently remove the edit
  affordance everyone has today. Your call.
- **D3 — Read path: `ReactPreview` reads the preference directly** via
  `usePreference('previewEditing')`, exactly like `richText` and
  `unlockNestingCursor`, and forwards `editingDisabled={!previewEditing}`
  through `ReactRenderer` to `Q2PreviewIframe`. `Editor.tsx` reads the same
  preference to drive the pill; the two stay in sync through
  `PREFERENCE_CHANGE_EVENT`. *Rationale:* no changes to `PreviewRouter`,
  and it matches the precedent for persisted iframe-bound flags.
  *Alternative:* thread it from `Editor.tsx` like `commentsMode` (one
  owner, explicit props, two more files). If D1 flips to session-only,
  threading becomes mandatory.
- **D4 — Pill placement and look.** A new `EditToggle` in `ReplayDrawer`,
  rendered **before** the Authors pill in both bar states:
  `{statusSlot} [Edit] [Authors] [Comments]`. Same pill anatomy as Authors
  (dot + label, `aria-pressed`, tooltip, `stopPropagation`), sharing the
  off-state look. On-state colour: reuse the Authors green (`--editor-success*`)
  for consistency — one visual language for "this mode is on". Tooltip copy:
  "Turn off editing — links become plain links" / "Turn on editing".
  *Alternative:* a two-button segmented control like Comments ("Edit" /
  "Read"). The pill is smaller and matches Authors; a segmented control is
  more explicit about there being two modes.
- **D5 — Format gating.** The pill is `disabled` (greyed, tooltip "Editing
  is not available for this format") unless `currentFormat === 'q2-preview'`.
  `q2-debug` and `q2-slides` have no block-edit surface, and `q2-debug`
  ignores `editingDisabled`. Note this is stricter than
  `attributionDisabled`, which also allows `q2-debug`. The stored
  preference is untouched while disabled, so switching back to a
  `q2-preview` document restores the user's choice.
- **D6 — Closing an open editor when editing is switched off.** Add a
  small effect in `PreviewRoot`: when `editingDisabled` transitions to
  `true` and `editTarget != null`, run the existing commit-if-dirty path
  and clear the edit target. Prefer the existing blur/close helper over a
  new code path (to be located during implementation; candidates are the
  `commitIfDirty` / plain-close helpers referenced around
  `PreviewRoot.tsx:113` and `:1083`). This is the only change inside
  `ts-packages/preview-renderer`.
- **D7 — Defense in depth in the host.** `ReactPreview.handleSetAst` drops
  `PreviewNodeEditPayload`s while editing is off, mirroring the SPA. Cheap,
  and it makes the invariant "read-only never writes" hold even if a
  payload is in flight when the toggle flips.
- **D8 — Not in the Settings tab (v1).** The pill is the control. Adding a
  row to `SettingsTab` is trivial later if wanted; skipping it keeps the
  change focused on what was asked.
- **D9 — Independent of `q2 preview --ui editor` without `--allow-edit`.**
  In that mode hub-client is served by a read-only `q2 preview` session
  (`previewSession.allowEdit === false`, which today only shows the
  ephemeral banner). Monaco edits are already ephemeral there; block edits
  go to the same in-memory document, so the pill stays functional and is
  not forced off. No coupling.

## Work items

### Phase 1 — tests first (all red before Phase 2)

- [x] `services/preferences/schema.test.ts`: `previewEditing` defaults to
      `true`; a stored prefs object without the key still parses and
      preserves the other settings (same shape as the `richText` tests).
- [x] `components/ReplayDrawer.test.tsx`, new `describe('Edit toggle')`
      mirroring the Attribution block: renders in collapsed and expanded
      states only when both `previewEditing` + `onPreviewEditingChange` are
      supplied; `aria-pressed` reflects state; click calls the callback
      with the negation; click does not call `controls.enter`; `disabled`
      renders non-interactive and drops `aria-pressed`.
- [x] `components/render/ReactRenderer.integration.test.tsx` (or a new
      focused test alongside it): `editingDisabled` given to
      `ReactRenderer` for `q2-preview` appears in the `UPDATE_AST` payload
      posted to the iframe (use the `postMessage` spy + `IFRAME_READY`
      pattern from `Q2PreviewIframe.integration.test.tsx:266`). Assert both
      `true` and `false`, and that a rerender with the other value re-posts.
- [x] `components/render/ReactPreview.*.integration.test.tsx`: with the
      `usePreference` mock returning `previewEditing: false`, a
      `PreviewNodeEditPayload` passed to `setAst` neither calls
      `applyNodeEdit` nor `onContentRewrite` (D7). With `true`, the existing
      path runs.
- [x] `ts-packages/preview-renderer` `q2-preview.integration.test.tsx`:
      with an open edit target, rerendering `PreviewRoot` with
      `editingDisabled: true` closes the session (edit target cleared; a
      dirty draft is committed through the normal path) (D6).
- [x] Playwright, new `hub-client/e2e/q2-preview-edit-toggle.spec.ts`
      (model: `q2-preview-locked-hover.spec.ts`): a `q2-preview` document
      with a paragraph containing a link to a second document. (a) Default:
      `[data-block-pool-id]` present; (b) click the Edit pill → no
      `[data-block-pool-id]` in the iframe, hovering a paragraph adds no
      outline; clicking the link navigates to the second document without
      opening an editor; (c) reload → still off (D1); (d) click the pill
      again → affordances return.

### Phase 2 — implementation

- [x] `services/preferences/schema.ts`: add `previewEditing` with
      `.default(true)` and the matching `DEFAULT_PREFERENCES` entry; update
      the seeded prefs object in `AboutTab.test.tsx:43` if its shape is
      asserted exactly.
- [x] `ReplayDrawer.tsx`: `EditToggle` component + `previewEditing` /
      `onPreviewEditingChange` / `previewEditingDisabled` props; render in
      both bar states before `AttributionToggle` (D4, D5).
- [x] `ReplayDrawer.css`: pill styles. Either extract the shared pill
      rules from `.replay-drawer__attribution` into a `.replay-drawer__pill`
      base used by both, or add a sibling `.replay-drawer__edit` block —
      decide when touching the file; no alpha colours (see
      `.claude/rules/hub-client-theme.md`).
- [x] `Editor.tsx`: `const [previewEditing, setPreviewEditing] =
      usePreference('previewEditing')`; pass to `ReplayDrawer` with
      `previewEditingDisabled={currentFormat !== 'q2-preview'}`.
- [x] `ReactPreview.tsx`: read the preference (D3); pass
      `editingDisabled={!previewEditing}` to `ReactRenderer`; early-return in
      `handleSetAst` when off (D7).
- [x] `ReactRenderer.tsx`: `editingDisabled?: boolean` prop, forwarded to
      `Q2PreviewIframe` only (documented like `commentsMode`).
- [x] `PreviewRoot.tsx` (preview-renderer): the close-on-disable effect
      (D6).
- [x] `DevHarness.tsx` replay-drawer fixture: pass the new props so the dev
      gallery shows the pill (optional but cheap; keeps the a11y harness
      specs covering it).

### Phase 3 — verification and bookkeeping

- [x] `cd hub-client && npm run test:ci` and `npm run build:all` (the
      production build is stricter than vitest).
- [x] `ts-packages/preview-renderer` tests (`npm test -w
      ts-packages/preview-renderer`).
- [x] Run the new Playwright spec (`VITE_E2E=1 npm run build`, then
      `npx playwright test e2e/q2-preview-edit-toggle.spec.ts
      --project=chromium --workers=1`).
- [x] **End-to-end in a real browser** against a running hub (`npm run
      dev`, or `cargo build --bin hub && npm run build:local-prod && npm run
      local-prod`): open a `q2-preview` document with a link, toggle the
      pill, follow the link, reload, toggle back. Record the invocation, a
      screenshot or DOM snippet, and an explicit "inspected" note here.
- [x] `cargo xtask verify` (full, since `hub-client` and
      `ts-packages/preview-renderer` change).
- [x] Two-commit workflow: code commit, then `hub-client/changelog.md`
      entry under `### 2026-09-09` with the hash.
- [ ] `braid close bd-ew0vak6b`.

## Files touched (expected)

| File | Change |
|---|---|
| `hub-client/src/services/preferences/schema.ts` (+ test) | `previewEditing` preference |
| `hub-client/src/components/ReplayDrawer.tsx` (+ test, `.css`) | `EditToggle` pill |
| `hub-client/src/components/Editor.tsx` | read pref, wire pill, format gating |
| `hub-client/src/components/render/ReactPreview.tsx` (+ test) | read pref → `editingDisabled`; drop guard |
| `hub-client/src/components/render/ReactRenderer.tsx` (+ test) | forward `editingDisabled` |
| `hub-client/src/components/DevHarness.tsx` | fixture props |
| `hub-client/e2e/q2-preview-edit-toggle.spec.ts` | new |
| `ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx` (+ test) | close open editor on disable |
| `hub-client/changelog.md` | entry |

No Rust changes. No changes to `Q2PreviewIframe`, `entry.tsx`,
`PreviewContext`, or the block components — the renderer side is already
complete.

## Out of scope (file as follow-ups if wanted)

- **Links clickable while editing is on.** Making `findEditTarget` (or
  `onPointerUp`) ignore clicks whose target is inside an `<a>` would fix
  the link problem *without* leaving edit mode. It changes the edit
  surface's click semantics for everyone and needs its own e2e; worth a
  separate strand linked `related` to bd-ew0vak6b.
- **Settings-tab row** for the preference (D8).
- **Per-document or per-project default** for editability (e.g. a YAML
  key). The preference is per browser, like the others.
- **`q2-preview-spa` status bar.** The SPA has no bottom bar; its mode is
  fixed by `--allow-edit`. Nothing changes there.

## Review decisions (2026-09-09)

All of D1–D9 accepted as written:

- D1 persisted preference; D2 default ON (navigation is expected to be
  less common than editing); D3 direct `usePreference` read in
  `ReactPreview`; D4 pill before Authors, Authors green for the on state;
  D5 `q2-preview`-only gating; D6 commit-if-dirty then close on disable;
  D7 drop guard in `handleSetAst`; D9 no coupling to `--allow-edit`.
- D8 confirmed: the pill is the only control, no Settings-tab row. This
  matches the header's view-mode toggle (markup / split / preview,
  `ViewModeContext.tsx`), which is likewise persisted and controlled only
  from the chrome. The one difference is storage: view mode uses its own
  bare localStorage key, while `previewEditing` lives in the versioned
  preferences schema, so it gets validation and cross-component sync.

## End-to-end verification record (2026-09-09)

Real Chromium against a real hub (Playwright's e2e harness: `globalSetup`
starts `cargo run --bin hub` on port 3031 and serves the `VITE_E2E=1`
build):

```
cd hub-client && VITE_E2E=1 npm run build
npx playwright test e2e/q2-preview-edit-toggle.spec.ts --project=chromium --workers=1
# ✓ 1 passed
```

Observed (asserted by the spec, and inspected via full-page screenshots of
the same session): with the pill on, both paragraphs carry
`data-block-pool-id` and hovering a paragraph draws the edit outline; after
clicking the pill (aria-pressed → false, label "Editing off") the iframe
has zero `[data-block-pool-id]` elements, hovering draws no outline, and
clicking "link to the other page" changes the route to `/file/other.qmd`
with no `<textarea>` and no `.q2-rt-toolbar` in the iframe; after
`page.reload()` the pill still reads "Editing off" and the page is still
read-only; clicking the pill again restores one `p[data-block-pool-id]`.
Gates: `cargo xtask verify --skip-rust-tests` (Rust untouched) and
`cargo xtask lint` green.

Note on ordering: the unit/integration tests were written and run red
before implementation; the Playwright spec was written after and run green
only (its failure mode without the implementation is trivially the pill not
existing).

Not covered by tests: a *dirty* rich-text session closed by the toggle
(no existing test makes the tiptap editor dirty in jsdom; the rich
`commit` is the same function the blur path uses).
