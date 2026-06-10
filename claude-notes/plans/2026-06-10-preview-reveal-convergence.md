# q2 preview: converge revealjs with render (kill drift, keep the React path)

**Strand:** bd-ibqkf9ry
**Related:** bd-jij5gge2 (render-side linked assets — done), bd-kw93 (q2-preview epic)
**Date:** 2026-06-10
**Status:** DRAFT — iterating. **Do not implement until the user gives the go-ahead.**

## Why preview reveal is React (and stays React)

`q2 preview` renders reveal decks via `RevealDeck.tsx` + `@revealjs/react`
React components, NOT render's Rust `assemble.rs` HTML. **This is by design and
must stay:** preview is a *separate format* whose React components map back to
their originating source AST nodes — the basis for the collaborative-editing
AST↔source communication API (preview documents will push AST edits back to the
`.qmd`). A "render once, show the HTML in an iframe" approach would lose that
node→source mapping, so it's rejected.

So **some** render/preview duplication is intrinsic (two renderers: Rust HTML
writer + React mirror). The goal is not to remove the duplication but to remove
the **drift**: make both pipelines use the **same reveal version** and the
**same CSS source**, and give the preview deck the **same CSS environment** as
render.

## The two symptoms (one root)

Observed on `~/Desktop/daily-log/2026/06/10/q2-reveal-test.qmd` (preview vs a
static server of the render output):

1. **Faint grey line under each slide title.** Source rule (pinned in-browser):
   `h2, .h2 { border-bottom: 1px solid rgb(222,222,223); padding-bottom: .5rem }`
   — **Bootstrap**. The q2-preview iframe injects the compiled HTML-theme
   (Bootstrap) CSS via a `data-q2-theme` `<link>` in `document.head`
   (`q2-preview/entry.tsx`) for *every* document, including reveal decks. Render's
   standalone deck never loads Bootstrap, so no line.
2. **No slide transitions.** Preview's `@revealjs/react` `<Deck config={{…,
   transition:'slide'}}>` doesn't animate; render's `Reveal.initialize({transition:
   'slide'})` does. `window.Reveal.getConfig()` is null in preview (the instance
   is React-wrapped). Cause TBD — see Phase 4.

**Root (the avoidable part):** render uses the **vendored** `resources/revealjs/`
(reveal.js 6.0.0, `include_str!`) + `Reveal.initialize`; preview imports the
**npm** `reveal.js/*.css` + `@revealjs/react` (npm reveal.js engine). Two CSS
sources, two engine copies, and a shared document with the app's Bootstrap CSS.
The CSS files are byte-identical *today* (both 6.0.0) but nothing enforces it,
and the Bootstrap leak + missing `reset.css` already diverge the result.

## Convergence design (Option B, user-directed)

### Prong A — one reveal **version**, enforced

`resources/revealjs/` is "Copied from `node_modules/reveal.js/dist/`" (its
README) — npm is already the upstream. Make that a guarantee, not a habit:

- Pin npm `reveal.js` to an **exact** version (drop the `^`), matching the
  vendored copy. **[Q-A1]** also pin `@revealjs/react` exact (it carries its own
  reveal.js — confirm it resolves the *same* reveal.js, not a second copy).
- Add a **sync check** (xtask or a test) asserting
  `resources/revealjs/{reveal.css,reset.css,theme/white.css,reveal.js}` are
  byte-identical to `node_modules/reveal.js/dist/…`. A future `reveal.js` bump
  then fails CI until the vendored copy is re-synced — render and preview can't
  drift. **[Q-A2]** xtask (`cargo xtask check-revealjs-sync`) vs a Rust/TS test;
  where does it run in `verify`.

### Prong B — one CSS **source**

`RevealDeck.tsx` currently imports `reveal.js/reveal.css` + `reveal.js/theme/
white.css` (npm) and `resources/revealjs/quarto-reveal.css` (vendored). Make
**all** reveal CSS come from the vendored copy, matching render's exact set and
order:

- Import `resources/revealjs/reset.css` (**currently missing in preview**),
  `…/reveal.css`, `…/theme/white.css`, `…/quarto-reveal.css`.
- Drop the `reveal.js/*.css` npm imports.

Now render (artifacts from `resources/revealjs/`) and preview (Vite imports of
the same files) draw from one source — they cannot disagree.

### Prong C — same CSS **environment** (the grey-line fix)

The reveal deck must not inherit the HTML-theme/Bootstrap CSS. In the q2-preview
iframe, the `data-q2-theme` Bootstrap link is for *HTML* document preview; a
reveal deck should render in a reveal-only CSS environment (as render does):

- When the active document is `format: revealjs`, **don't inject** (or remove)
  the `data-q2-theme` HTML-theme `<link>` — the deck supplies its own complete
  CSS. **[Q-C1]** exact mechanism: gate the theme-injection in `entry.tsx` on
  the format, vs. scoping Bootstrap so it can't reach `.reveal`. Gating is
  cleaner and matches render (render decks have zero Bootstrap). Confirm nothing
  in the deck content (rendered via `previewRegistry`) actually needs Bootstrap.

### Prong D — transitions

Investigate why `@revealjs/react`'s `<Deck transition:'slide'>` doesn't animate
while native `Reveal.initialize` does. Hypotheses: the `<Deck>` re-creates/
re-syncs on every preview re-render (preview re-renders on each edit), resetting
transition state; or a config/lifecycle nuance of `@revealjs/react`; or it needs
`Reveal.sync()` vs full re-init. **[Q-D1]** confirm it reproduces on a static
(non-editing) load — if transitions work until the first edit, it's the
re-render lifecycle, not config. This prong may be independent of A–C.

## Open questions

- **[Q-A1]** ✅ RESOLVED: `@revealjs/react@0.2.0` declares `reveal.js` as a
  **peerDependency** (`>=5`), no bundled/nested copy — it uses the hoisted
  top-level `reveal.js@6.0.0`. So there's exactly one npm reveal.js and the
  vendored copy derives from it; pinning the top-level `reveal.js` exact +
  enforcing vendored byte-identity gives both pipelines one engine version.
- **[Q-A2]** Sync-check home (xtask vs test) + wiring into `cargo xtask verify`.
- **[Q-C1]** Theme-injection gating point + confirming reveal content needs no
  Bootstrap.
- **[Q-D1]** Transition root cause (lifecycle vs config) — may split into its
  own sub-strand if unrelated to the CSS/version convergence.
- **[Q-E1]** Is the same drift latent for the **HTML** preview (`q2-debug`/
  `q2-preview` formats) — i.e., does the HTML preview's CSS also come from a
  different code path than render's `CompileThemeCssStage`? Out of scope here
  (reveal-only), but worth a sibling strand if so — it's the same class of bug.

## Phasing (TDD-first)

- [x] **1 — Prong A:** DONE (commit `6c30e7db`). Pinned `reveal.js` → `6.0.0` +
  `@revealjs/react` → `0.2.0` (exact) in the 3 package.json; added the
  `vendored_reveal_assets_match_npm_package` test in `assemble.rs` (compares the
  `include_str!` constants to `node_modules/reveal.js/dist/*`; skips w/o
  node_modules). Runs in `cargo nextest` (part of verify).
- [x] **2 — Prong B:** DONE (commit `d366924d`). `RevealDeck.tsx` imports the
  vendored `resources/revealjs/{reset,reveal,theme/white,quarto-reveal}.css`
  (added the missing `reset.css`) instead of npm `reveal.js/*.css`. tsc clean,
  181 preview-renderer tests pass.
- [x] **3 — Prong C:** DONE (commit `d366924d`). `entry.tsx` reconciles the
  `data-q2-theme` HTML-theme link against the active format — attaches it only
  for non-slide docs (remembers last theme URL + `isSlides`, driven by a
  `useEffect`). Browser-verified: every slide `h2` `borderBottom` is now
  `0px none` (was `1px solid #dedede`), no `data-q2-theme` link in the deck.
- [x] **4 — Prong D (transitions): RESOLVED BY PRONG C.** Root cause found: the
  leaked Bootstrap reboot carries
  `@media (prefers-reduced-motion: reduce) { *,::before,::after {
  transition-duration: .01ms !important; … } }`. For a user with reduced-motion
  enabled, that `!important` override killed reveal's `0.8s` slide transition in
  preview (render, never having Bootstrap, always animated; headless defaults to
  no-preference, which is why it didn't reproduce there). Removing the Bootstrap
  leak (Prong C) restores transitions. Verified: under emulated
  `reduced-motion: reduce`, the preview deck section `transition-duration` is
  `0.8s` (animates) with no reduced-motion rule present. **One leak, two
  symptoms (grey line + dead transitions); one fix (C).**
- [ ] **5 — E2E parity + full verify.** Browser parity done (preview vs render:
  identical `slide` class + section transition, navigation works, no Bootstrap
  on `.reveal`, no grey line). Remaining: full `cargo xtask verify` (WASM + SPA
  + hub tests) as the gate.

## Out of scope

- The HTML-preview CSS-codepath convergence ([Q-E1]) — sibling strand if needed.
- The served-iframe embed work (bd-kjrpya2d) — orthogonal.
- Render-side reveal (bd-jij5gge2) — done.
