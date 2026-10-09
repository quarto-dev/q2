---
title: 'q2 preview: audit HTML preview CSS for the render-drift class (bd-4b7f1hr7)'
date: 2026-06-10
description: 'Audits whether the HTML preview''s theme CSS and math rendering diverge from `q2 render`, finding that KaTeX is pinned differently on each side and proposing a version pin and a sync test.'
---

**Date:** 2026-06-10
**Braid:** bd-4b7f1hr7
**Checkout:** room-2 main checkout, branch `main` \@ `e628a18f` (investigation committed here; implementation stacks on `feature/revealjs-render-preview-convergence`, PR #271 — decided \[Q1\])
**Status:** DONE (2026-06-10) — all phases complete, full `cargo xtask verify`
green on branch `beads/bd-4b7f1hr7-q2-preview-audit-html` (stacked on PR
#271's `feature/revealjs-render-preview-convergence`). Not pushed. Follow-up
strands: bd-izs62xci (SCSS compiler split), bd-sm314r1x (math engine
divergence).

## Triage verdict

**Ready to design.** The audit's central fear — that the HTML preview's theme
CSS comes from a different codepath than render's — turns out to be **mostly
unfounded** (same Rust stage, same vendored Bootstrap), but the investigation
found **one confirmed drift instance (KaTeX)**, one **unverified identity
assumption** (theme-CSS bytes), and one **stale sibling surface (q2-debug)**.
The work is well-scoped once the design questions below are settled.

## Issue context

Filed 2026-06-10 by Carlos (via the reveal-convergence session) as the sibling
of bd-ibqkf9ry, which fixed the same drift class for reveal decks: preview drew
reveal CSS from npm while render used vendored `resources/revealjs/`, and the
app's Bootstrap leaked onto the deck. This strand asks whether the **HTML**
preview formats (`q2-preview` / `q2-debug`) have the same disease:

1. Does the HTML preview's CSS come from the same codepath as render's
   `CompileThemeCssStage`, or a divergent bundle? Is the `data-q2-theme`
   link's CSS byte-identical to render's output for the same doc/theme?
2. Is the Bootstrap version/source pinned and shared between render and
   preview, or can they drift (cf. reveal's version pin + vendored↔npm sync
   test)?
3. Any DOM/computed-style divergences beyond what `/preview-render-parity`
   already tracks per-node?

If drift exists: apply the reveal convergence pattern — single CSS
source/codepath + a sync/identity check, keeping the React preview path.

## Dependency graph

- **related: bd-ibqkf9ry** (closed) — the reveal precedent. Fix shipped as
  **PR #271** (`feature/revealjs-render-preview-convergence`, open against
  `main` at investigation time): pinned reveal.js 6.0.0 + @revealjs/react
  0.2.0 exact, vendored↔npm byte-identity test
  (`vendored_reveal_assets_match_npm_package` in `assemble.rs`),
  `RevealDeck.tsx` imports vendored CSS, `entry.tsx` gates the
  `data-q2-theme` link off for slide docs. Plan:
  `claude-notes/plans/2026-06-10-preview-reveal-convergence.md` (on the PR
  branch). Its open question **\[Q-E1\]** is literally this strand.
- **parent-child: bd-kw93** (open epic) — `q2 preview` epic; this is parity
  polish (epic phase D territory). Epic integration branch convention says
  sub-task work normally branches off the epic's integration line — see \[Q1\].

## What the code looks like today (audit findings, `main` \@ e628a18f + PR #271)

### (1) Theme CSS codepath: SHARED — but output identity is unverified

Unlike reveal, the HTML preview theme CSS is **not** a separate bundle:

- The q2-preview pipeline **includes the same `CompileThemeCssStage`** as the
  html pipeline (`crates/quarto-core/src/pipeline.rs:488` vs `:289`; the
  stage-exclusion comment at `:341` says it's included deliberately so the
  compiled theme CSS exists for preview).
- The stage writes the artifact to the VFS at
  `DEFAULT_CSS_ARTIFACT_PATH = /.quarto/project-artifacts/styles.css`
  (`pipeline.rs:89`), mirrored by hand in
  `ts-packages/preview-renderer/src/types/artifactPaths.ts` (sync-by-comment,
  same pattern as `types/diagnostic.ts`).
- Parent-side `Q2PreviewIframe.tsx` reads the VFS bytes, mints a blob URL,
  posts `UPDATE_THEME`; the iframe's `entry.tsx` manages the single
  `<link data-q2-theme>`. PR #271 gates this link off for slide docs.

So the SCSS→CSS compilation (including Bootstrap) is the same Rust code
compiled for native and WASM. **What is NOT verified:** that the *inputs* to
the stage match — format-specific metadata (`format.html` vs
`format.q2-preview` theme keys), project-config resolution, stage ordering —
i.e. that the artifact bytes equal what `q2 render` emits for the same
doc/theme. No test asserts this today. That identity check is the core
deliverable of this strand.

### (2) Bootstrap: single vendored source, version-pairing by convention

- **SCSS:** compiled inside `CompileThemeCssStage` from `resources/scss/`
  (compile-time embed; shared native/WASM by construction). No npm
  `bootstrap` dependency exists anywhere in the workspace (checked root,
  hub-client, ts-packages package.json + imports) — **no second source, no
  reveal-style npm↔vendored drift possible.**
- **JS:** render's `BootstrapJsStage` does
  `include_bytes!("resources/js/bootstrap/bootstrap.bundle.min.js")`
  (`bootstrap_js.rs:77`); preview's `q2-preview/entry.tsx:45` imports the
  **same file** via Vite `?raw`. Same bytes by construction.
- **Soft spot:** the SCSS (5.3.1) and the JS bundle are paired only by a
  comment ("Bump the two together", `bootstrap_js.rs:75`). A version-pairing
  check (cf. reveal's sync test) would make the pairing a guarantee, not a
  habit. Cheap.

### (3) KaTeX: CONFIRMED drift — the reveal disease, arguably worse

- **Render** (`math_js.rs:75`): emits a CDN link —
  `DEFAULT_KATEX_URL_BASE = "https://cdn.jsdelivr.net/npm/katex@latest/dist/"`
  — note **floating `@latest`**.
- **Preview** (`q2-preview/entry.tsx:30–31`): bundles **npm
  `katex@^0.16.28`** (caret, not exact) JS + `katex/dist/katex.min.css`.

Different sources AND both unpinned: render's math output can change under
users when the CDN advances, independently of the npm lockfile, and the two
pipelines can render math differently today. This is exactly the drift class
the strand asks about. (The `@latest` is also a reproducibility bug for
render on its own, preview aside.)

### (4) q2-debug: untouched by the reveal fix

`hub-client/src/components/render/q2-debug/entry.tsx:13–16` still imports
**npm** `reveal.js/reveal.css` + `reveal.js/theme/white.css` + npm KaTeX CSS.
PR #271 only converged the `ts-packages/preview-renderer` q2-preview entry.
q2-debug ignores `UPDATE_THEME` entirely (no theme link), so it's a debug
surface with deliberately different chrome — scope question \[Q3\].

### (4b) Math engine divergence (found during implementation)

The preview typesets math **unconditionally with KaTeX**
(`ts-packages/preview-renderer/src/q2-preview/inlines/Math.tsx`,
`katex.renderToString`) and never consults `html-math-method`; render's
*default* engine is **MathJax 3** (`MathEngine::default_engine`). So with
default settings the two surfaces typeset math with different engines.
Plausibly deliberate (KaTeX is synchronous and fast — right for live
editing), but it should be a decision, not an accident. Filed
**bd-sm314r1x** (discovered-from this strand) to assess; out of scope here.
The KaTeX version pin from Phase 2 means that *when* both sides use KaTeX,
they now match exactly.

### (5) Computed-style parity sweep

Not yet performed (browser work). The `/preview-render-parity` skill exists
for per-node diffs; the strand asks for a one-time broader sweep. Deferred to
a phase so findings land as fixtures/strands rather than ad-hoc notes.

## Design decisions (aligned with user, 2026-06-10)

1. **\[Q1\] Base branch: stack on `feature/revealjs-render-preview-convergence`**
   (PR #271's head). Branch off its tip; lands after #271 merges.
2. **\[Q2\] KaTeX: pin the CDN URL** (option a — simplest). Implies pinning npm
   `katex` **exact** (`0.16.28`, dropping the `^`) so "CDN matches the npm
   pin" is well-defined, changing `DEFAULT_KATEX_URL_BASE` from `@latest` to
   `@0.16.28`, and adding a version-sync test. Vendoring (option b) deferred.
3. **\[Q3\] q2-debug: fix if simple.** It's internal-only and likely to
   disappear soon — converge its npm reveal/KaTeX CSS imports opportunistically
   (vendored CSS, same as q2-preview), but don't sink design effort into it.
   If anything non-trivial surfaces, exempt with a comment and move on.
4. **\[Q4\] Sync-check home: plain Rust `#[test]`**, following the reveal
   precedent (`vendored_reveal_assets_match_npm_package` in `assemble.rs`) —
   parse root `package.json` for the pinned katex version, assert it matches
   the version in `DEFAULT_KATEX_URL_BASE`. Runs under `cargo nextest`, so
   `verify` picks it up with no new wiring. Same home for the Bootstrap
   SCSS↔JS pairing check if we keep it.
5. **\[Q5\] Parity sweep: dropped from this strand.** A future, more thorough
   pass will address reveal 6 themes + quarto's SCSS system together; the
   browser sweep waits for that. Phase 5 below shrinks to E2E verification of
   the changes made here.

## Phases

- **Phase 0 — Test plan (TDD: failing tests first).**
  - [x] Theme-CSS identity test:
        `crates/quarto-core/tests/integration/preview_render_css_parity.rs` —
        4 cases (default theme, `theme: cosmo`, format-scoped
        `format: html: theme: darkly`, `theme: none`), each rendering the
        same doc through `render_qmd_to_html` and `render_qmd_to_preview_ast`
        with private sass caches and byte-comparing the
        `css:theme:<fingerprint>` artifact (key + content).
  - [x] KaTeX version-sync test: `katex_cdn_version_matches_npm_pin` in
        `math_js.rs` tests — root `package.json` AND
        `hub-client/quarto-hub-sandboxed-preview/package.json` (a
        non-workspace sub-project with its own bundled KaTeX, discovered
        during implementation) must pin `katex` exactly, agree with each
        other, and match `DEFAULT_KATEX_URL_BASE`. **Verified failing
        first** (`^0.16.28` not exact; URL said `@latest`).
- **Phase 1 — Theme-CSS identity.** ✅ **No divergence found**: all 4
  identity cases passed on first run — the html and q2-preview pipelines
  already assemble byte-identical theme CSS (incl. the format-scoped case,
  because `MetadataMergeStage` flattens on `ctx.format.identifier` = html
  for the q2-preview pseudo-format). The tests stay as regression guards.
  No normalization needed.
- **Phase 2 — KaTeX convergence.** ✅ Pinned `katex` to exactly `0.16.28`
  in root + sandboxed-preview package.json (lockfiles regenerated);
  `DEFAULT_KATEX_URL_BASE` → `katex@0.16.28` (was `@latest`); sync test
  green; all 27 katex/math/parity tests pass.
- **Phase 3 — q2-debug opportunistic cleanup.** ✅
  - [x] `q2-debug/entry.tsx`: npm `reveal.js/*.css` → the four vendored
        `resources/revealjs/{reset,reveal,theme/white,quarto-reveal}.css`
        (mirrors `RevealDeck.tsx`; q2-debug had also been missing
        `reset.css`/`quarto-reveal.css` relative to render).
  - [x] `RevealjsReactAstSlideRenderer.tsx` (hub editor's deck renderer —
        same drift class, discovered during the sweep): swapped its two npm
        base-CSS imports for the vendored equivalents (byte-identical today
        → no visual change). Deliberately did NOT add
        `reset.css`/`quarto-reveal.css` there — it predates them and adding
        them would change the live editor's appearance.
  - [x] `parity.integration.test.tsx` mocks updated to the vendored paths.
        (Note: hub-client integration tests need `npm run build:wasm` in a
        fresh worktree; the parity suite fails to collect without it —
        pre-existing, verified via stash-compare.)
- **Phase 4 — E2E verification.** Per the end-to-end verification policy:
  - [x] Real `q2 render` through the binary, output inspected:

    ```
    $ cargo run --bin q2 -- render \
        claude-notes/plans/html-preview-css-drift-audit-investigation/math-katex.qmd
    $ grep -o 'https://cdn.jsdelivr.net/npm/katex[^"]*' math-katex.html | sort -u
    https://cdn.jsdelivr.net/npm/katex@0.16.28/dist/contrib/auto-render.min.js
    https://cdn.jsdelivr.net/npm/katex@0.16.28/dist/katex.min.css
    https://cdn.jsdelivr.net/npm/katex@0.16.28/dist/katex.min.js
    ```

    All three emitted KaTeX URLs carry the exact pin; no `@latest`.
    Fixture committed at
    `claude-notes/plans/html-preview-css-drift-audit-investigation/math-katex.qmd`
    (generated outputs removed).
  - [x] Full `cargo xtask verify` (hub build leg included — hub-client
        files changed): **all steps passed** (2026-06-10, branch
        `beads/bd-4b7f1hr7-q2-preview-audit-html`).
  - [x] Bootstrap SCSS↔JS pairing check:
        `bootstrap_js_version_matches_scss_readme` in `bootstrap_js.rs`
        tests — the JS bundle's `Bootstrap vX.Y.Z` banner must equal the
        version `resources/scss/README.md` documents for the SCSS dist
        (the SCSS distribution itself carries no machine-readable version
        string, so the README — part of the documented bump procedure —
        is the strongest SCSS-side marker). Passes today (both 5.3.1);
        guards future bumps.
  - [x] Filed **bd-izs62xci** (discovered-from bd-4b7f1hr7) for the
        **SCSS compiler split**: native render compiles assembled SCSS
        with `grass`, the browser preview compiles the same SCSS with
        dart-sass (JS bridge) — so the bytes shipped to the browser are
        NOT guaranteed identical to render's even with converged inputs
        (which the identity tests now pin). Structural, out of scope
        here; the strand covers measuring the practical difference and
        deciding converge-vs-document.

## Risks / tradeoffs

- **PR #271 is in flight.** We stack on its head (\[Q1\]); if #271 gets
  reworked in review, this branch rebases with it. Accepted.
- **The theme-identity test may be flaky-by-construction** if render
  legitimately post-processes CSS (e.g. URL rewriting for output dirs). If
  bytes can't match exactly, the test needs a principled normalization — to
  be discovered in Phase 1, worth flagging now.
- **KaTeX `@latest`:** any fix changes render output for all users (the CDN
  URL in emitted HTML). Low risk, but it's a user-visible output change worth
  a changelog note.
- **The `artifactPaths.ts` mirror constant** is sync-by-comment; if Phase 1
  touches the artifact path it must update both sides (existing convention,
  noted in the file).
