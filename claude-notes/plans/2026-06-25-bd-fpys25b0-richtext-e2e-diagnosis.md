# bd-fpys25b0 — Diagnosis: "12 hub-client block-editing e2e specs red since rich-text default-on"

**Strand:** bd-fpys25b0 (discovered-from bd-9x3zbuj8)
**Date:** 2026-06-25
**Checkout:** main @ `c64e391e`
**Author of investigation:** session reproduction + static trace

## TL;DR (verdict)

**The bug as stated does NOT reproduce on `main`. All 12 specs are green.**
The underlying *mechanism* the strand describes is real and reproducible, but it
is **already fixed** by bd-038tnyqy's chokepoint pin (commit `b91d7277`), which
covers all 12 specs — not "a subset," as the strand assumes. The strand's
premise ("bd-038tnyqy only pinned a subset; these 12 were missed") is incorrect.

Recommended disposition: **close bd-fpys25b0 as already-fixed**, optionally
preceded by a small *hardening* refactor (Option B below) because the green
state currently rests on an implicit, non-obvious registration-order invariant
that bd-9x3zbuj8's in-flight work could violate.

There is also a **separate environmental gotcha** (missing tiptap deps) that is
the more likely thing the other agent actually hit — see "Secondary finding."

## Evidence

All runs used a correct, freshly built `VITE_E2E=1` hub-client bundle
(`dist/index.html` @ 2026-06-25 15:34; bundle references `richText`), after
`npm install` from the repo root (required — see Secondary finding).

1. **Baseline (pin intact, as on `main`): all 12 specs pass.**
   `npx playwright test <all 12 q2-preview specs> --retries=0` → **42 passed (34.5s)**.

2. **Control (pin defeated): the strand's symptom reproduces exactly.**
   Temporarily flipped the three `richText: false` writes in
   `bootstrapProjectSet` (`hub-client/e2e/helpers/projectFactory.ts`) to
   `richText: true`, then ran two *paragraph-editing* specs:
   `delete-by-emptying` + `breadcrumb-geometry` → **8 failed, 1 passed**, every
   failure being:
   ```
   waiting for locator('iframe[src*="q2-preview.html"]').contentFrame()
     .locator('textarea').first() to be visible
   > await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
   ```
   i.e. Para blocks opened in `.ProseMirror`, no `<textarea>` mounted — exactly
   the described failure. (Control edit has been reverted; tree is clean.)

Conclusion from (1)+(2): the rich-text-on build *does* gate the editor on
`richText`, and the `richText:false` pin is **load-bearing**. With the pin (the
state on `main`) the specs are green.

## Why the chokepoint fix already covers all 12

`bd-j1nto6eq` made `richText` default ON
(`hub-client/src/services/preferences/schema.ts:34` — `z.boolean().default(true)`).
The editor surface is chosen in
`ts-packages/preview-renderer/src/q2-preview/dispatchers.tsx:515` —
`richTextAvailable = !!ctx.richText && RICHTEXT_SUPPORTED_TYPES.has(type)`,
where `RICHTEXT_SUPPORTED_TYPES = {'Para','Header'}` (line 512). So a Para/Header
block opens ProseMirror when `richText` is true; everything else (and the
`richText:false` case) opens the `<textarea>`.

`bd-038tnyqy` (commit `b91d7277`) added an `addInitScript` **inside
`bootstrapProjectSet`** (`projectFactory.ts:198-222`) that *merges*
`richText:false` into the seeded prefs:

```js
const cur = JSON.parse(raw);
if (cur.richText === undefined) {
  localStorage.setItem(KEY, JSON.stringify({ ...cur, richText: false }));
}
```

`bootstrapProjectSet` is the single chokepoint every editing spec routes through
(`openFile()` → `bootstrapProjectSet()`), so this is a **global** pin, not a
per-spec one. The reads round-trip cleanly: `getPreferences()`
(`preferences/index.ts:18`) → `validatePreferences` preserves an explicit
`richText:false`; only an *absent* `richText` re-defaults to `true`.

### The registration-order subtlety (why it actually works)

The 12 specs fall into two shapes, both currently safe:

- **Own-prefs specs** (breadcrumb-geometry/-isolation, nesting-caret-in/-size-in,
  scrolljack, inline-edit, item-edit-size, block-nav-p2-5b, crumb-no-carry):
  each registers its *own* `addInitScript` that does a **full**
  `localStorage.setItem('quarto-hub:preferences', {…})` **omitting `richText`**
  (to set `unlockNestingCursor`). That alone would drop the pin — but each spec
  registers it *before* calling `openFile()`→`bootstrapProjectSet()`, so
  bootstrap's merge init-script is registered **last** and therefore **runs
  last** on every navigation, re-adding `richText:false` after the spec's
  overwrite. Net: `richText:false` wins. (Playwright runs init-scripts in
  registration order on every document load.)
- **No-prefs specs** (delete-by-emptying, self-heal-on-write, expand-on-edit):
  no own pref seeding; bootstrap's pin applies directly.

Additionally, the *nesting/list* specs edit **list items**, which are not in
`RICHTEXT_SUPPORTED_TYPES`, so they always get a textarea regardless of
`richText` — doubly safe.

This is exactly why the strand's "bd-038tnyqy only pinned a subset" model is
wrong: there is no per-spec subset; one chokepoint covers them all.

## Latent fragility (the legitimate kernel of the strand)

The green state depends on an **implicit invariant**: every spec's full-overwrite
`addInitScript` must be registered *before* `bootstrapProjectSet`. Nothing
enforces this. A future spec (or a refactor of `openFile` that seeds prefs
*after* bootstrap) that registers a full pref-overwrite **after** the chokepoint
would silently drop `richText:false` and time out — the precise failure the
strand predicts. bd-9x3zbuj8 is actively editing these breadcrumb/nesting specs,
so the risk is live even though nothing is red today.

## Secondary finding (likely what the other agent actually hit)

This checkout's `node_modules` was **missing the tiptap/prosemirror deps** that
the rich-text editor needs (declared in
`ts-packages/preview-renderer/package.json` — `@tiptap/*`, `prosemirror-markdown`
— but not installed). Consequences observed:

- `VITE_E2E=1 npm run build` **fails** with `tsc` `TS2307: Cannot find module
  '@tiptap/core'` (and ~20 siblings). The lifecycle script exits 2.
- Because the build fails, `dist/` stays stale. A subsequent
  `playwright test` (whose `webServer` does `vite preview` over `dist/` with
  `reuseExistingServer`) then silently serves a **pre-rich-text bundle**, in
  which `richText` gates nothing and *every* spec passes vacuously — a classic
  stale-bundle false-negative.

Fix: `npm install` from the **repo root** (per CLAUDE.md), then rebuild. After
that, the build is green and the reproduction above holds.

If the "different agent" reported the suite as broken, the most probable trigger
is this **build failure / stale-bundle** situation, not a live `richText`
timeout on `main`.

## Fix plan

### Option A — close as already-fixed (minimal)
1. Add a comment in `bootstrapProjectSet` documenting the registration-order
   invariant (bootstrap's merge must remain the last-registered pref init-script;
   specs must register their own pref overwrites *before* calling it).
2. Close bd-fpys25b0 referencing this doc + the reproduction evidence.

### Option B — harden, then close (recommended given bd-9x3zbuj8)
Remove the order-dependence and the duplicated literal pref objects:

1. Add a shared helper in `hub-client/e2e/helpers/projectFactory.ts`, e.g.
   `seedE2EPreferences(page, overrides)`, that registers an `addInitScript`
   writing `{ ...DEFAULT_PREFERENCES, richText: false, ...overrides }` — so
   `richText:false` is the explicit, self-documenting baseline and is
   order-independent (a spec that needs the rich editor opts in with
   `{ richText: true }`).
2. Replace the inline
   `page.addInitScript(() => localStorage.setItem('quarto-hub:preferences', {…}))`
   blocks in the own-prefs specs with `seedE2EPreferences(page, { unlockNestingCursor: true })`.
3. Keep the chokepoint merge as a belt-and-suspenders default for no-prefs specs.
4. Re-run the 12 specs (must stay green) + the control (forcing the helper to
   omit `richText` must reproduce the timeout) to prove the helper is the thing
   keeping them green.
5. Close bd-fpys25b0.

This is the "shared e2e preferences default" the strand itself floated, and it
directly de-risks bd-9x3zbuj8's parallel edits to these specs.

### Note for bd-9x3zbuj8 Task 2
The breadcrumb specs (geometry/isolation) exercise the **standalone** floating
chip. They stay valid only because `richText` is pinned OFF (with it ON they'd
render the **inline** breadcrumb and suppress the standalone chip). Any new spec
that needs richText ON must opt in explicitly and must not reuse these specs'
standalone-geometry assertions.

## How to reproduce / verify (commands)

```bash
# one-time: deps must be present (this checkout was missing tiptap)
npm install                              # from repo ROOT

cd hub-client
VITE_E2E=1 npm run build                 # must exit 0; dist/ refreshed; bundle has `richText`

# baseline — all green on main:
npx playwright test \
  q2-preview-block-nav-p2-5b q2-preview-crumb-no-carry-expansion q2-preview-inline-edit \
  q2-preview-expand-on-edit q2-preview-item-edit-size q2-preview-self-heal-on-write \
  q2-preview-breadcrumb-geometry q2-preview-nesting-size-in q2-preview-breadcrumb-isolation \
  q2-preview-nesting-caret-in q2-preview-delete-by-emptying q2-preview-scrolljack \
  --workers=3 --retries=0                 # → 42 passed

# control — defeat the pin to reproduce the strand's symptom:
#   in projectFactory.ts bootstrapProjectSet, flip the 3 `richText: false` → `richText: true`
npx playwright test q2-preview-delete-by-emptying q2-preview-breadcrumb-geometry \
  --workers=2 --retries=0 --timeout=30000 # → Para specs time out on locator('textarea')
#   then REVERT the flip.
```
