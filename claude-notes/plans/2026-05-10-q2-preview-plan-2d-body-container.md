# Plan 2D — q2-preview body container

**Date:** 2026-05-10
**Branch:** feature/q2-preview
**Status:** Implementation plan
**Milestone:** M2 polish. Closes the gap between q2-preview's bare-fragment output and the HTML pipeline's `#quarto-content > main.content#quarto-document-content` wrapper, so theme CSS that targets `.page-layout-article`, `.content`, and `body.fullcontent` selectors lands on real elements in the iframe DOM.

## Goal

Add a document-level body container to q2-preview that mirrors the HTML pipeline's wrapper structure (`crates/quarto-core/src/template.rs:140-256`) and respects user page-layout preferences read directly from `ast.meta`. After 2D lands:

- The iframe DOM has `<div id="quarto-content" class="quarto-container page-columns page-rows-contents page-layout-{layout}"><main class="content" id="quarto-document-content">{blocks}</main></div>` wrapping rendered blocks (or no wrapper when `minimal: true` / `theme: none`).
- The iframe `<body>` element carries the user's `body-classes` override or the literal default `fullcontent`, applied imperatively on document mount.
- Theme CSS rules that target `body.fullcontent .content`, `.page-layout-full`, etc. land on real elements without theme forks.
- Sidebar / navbar / TOC / footer chrome remains deferred (they require pipeline transforms that q2-preview's `Q2_PREVIEW_TRANSFORM_EXCLUDED` currently elides — a separate plan adds them and revisits body-classes computation).

## Checklist

### Phase 6 — Body container

(Phase numbering continues from Plan 2C's Phase 4 + Phase 5.)

- [ ] **6.1** Lift `extractMetaString` to a shared util — promote the private helper from `hub-client/src/components/render/ReactAstSlideRenderer.tsx:350` to `hub-client/src/utils/meta.ts` (NEW, ~30 LOC including extension to `extractMetaBool`). Update the one existing caller (`ReactAstSlideRenderer.tsx:220-221`) to import from the new location. **First commit of Phase 6**, per the "enumeration before consumers" rule.
- [ ] **6.2** Extend `q2-preview/PreviewDocument.tsx` with the body-container wrapper (~30 LOC). Read `page-layout`, `body-classes`, `minimal`, `theme` from `ast.meta` via the new util. Apply `body-classes` to `document.body.className` imperatively in a `useEffect`. Emit the wrapper structure unless `minimal: true` or `theme === 'none'` or `format: revealjs` (those paths skip the wrapper, matching the Rust template's `is_minimal_html()` check at `template.rs:501`).
- [ ] **6.3** Vitest unit tests for `extractMetaString` / `extractMetaBool` against the typed Pandoc Meta variants (`MetaString`, `MetaInlines`, `MetaBool`, missing key). Per-snapshot tests on `PreviewDocument` for: default (article + fullcontent), `page-layout: full`, `body-classes: custom-cls`, `minimal: true` (no wrapper), `theme: none` (no wrapper).
- [ ] **6.4** Smoke-all q2-preview fixtures — extend `crates/quarto/tests/smoke-all/q2-preview/` (directory exists post-2B) with:
  - `body-container-default.qmd` (single-doc; no `page-layout` set; assert `div#quarto-content.page-layout-article` and `body.fullcontent`).
  - `body-container-full-layout.qmd` (single-doc; `page-layout: full`; assert `div.page-layout-full`).
  - `body-container-override.qmd` (single-doc; `body-classes: custom-cls`; assert `body.custom-cls` AND no `body.fullcontent`).
  - `body-container-minimal.qmd` (single-doc; `minimal: true`; assert no `div#quarto-content` and no `<main>` wrapper).

  All fixtures use `_quarto.tests.run.requires_js: true` so the Playwright runner picks them up. Pattern matches Plan 2C item 5.2.
- [ ] **6.5** Run `cargo xtask verify --e2e` before declaring 2D complete (per project CLAUDE.md "End-to-end verification before declaring success"). Default `cargo xtask verify` skips the Playwright runner; without `--e2e` the smoke fixtures land in 6.4 are not exercised. Also do a manual browser session against a running hub for sanity; record the invocation and an inspected-output snippet in the implementation transcript or this plan's checklist comments.

## Scope

### In scope

#### `hub-client/src/utils/meta.ts` — shared meta helpers (NEW)

Lifted from `ReactAstSlideRenderer.tsx:350`. Two helpers:

```ts
import type { PandocAST } from '../components/render/framework/types';

type Meta = PandocAST['meta'];

/**
 * Extract a string from a Pandoc Meta value. Handles MetaString and
 * MetaInlines (the two types that real frontmatter values produce in
 * practice). Returns undefined for missing keys, MetaBool, MetaList,
 * or MetaMap (which can't reasonably be coerced to a string).
 *
 * Existing callers: q2-debug slide title/author, q2-preview body container.
 */
export function extractMetaString(meta: unknown): string | undefined {
    if (!meta || typeof meta !== 'object') return undefined;
    const m = meta as { t?: string; c?: unknown };
    if (m.t === 'MetaString' && typeof m.c === 'string') return m.c;
    if (m.t === 'MetaInlines' && Array.isArray(m.c)) {
        // Re-use the framework's plain-text walk to handle Inlines like
        // [Str("article")]. inlinesToPlainText is already shipped in
        // q2-preview/utils.ts (Plan 2B).
        return inlinesToPlainText(m.c as InlineNode[]);
    }
    return undefined;
}

/**
 * Extract a boolean from a Pandoc Meta value. Treats both MetaBool and
 * MetaString("true" | "false") as valid — the YAML parser produces one
 * or the other depending on quoting.
 */
export function extractMetaBool(meta: unknown): boolean | undefined {
    if (!meta || typeof meta !== 'object') return undefined;
    const m = meta as { t?: string; c?: unknown };
    if (m.t === 'MetaBool' && typeof m.c === 'boolean') return m.c;
    if (m.t === 'MetaString' && (m.c === 'true' || m.c === 'false')) return m.c === 'true';
    return undefined;
}
```

The `inlinesToPlainText` re-use means q2-preview's `MetaInlines` → string coercion handles bold / emph / code inline markup in the layout key — same behavior as the Rust template's variable substitution, which sees the rendered Inlines as a string.

**Why shared util, not q2-preview-local**: q2-debug's slide renderer already uses `extractMetaString`. Lifting avoids two copies that drift independently. q2-debug's existing call site is updated to import from the new location in the same commit (one-line diff). Future format-render targets (revealjs slides, future docx, etc.) get the helper for free.

#### `q2-preview/PreviewDocument.tsx` — body container

Replace the current Fragment-only body with the page-layout-aware wrapper. Source structure (annotated against the Rust template):

```tsx
import { useEffect } from 'react';
import type { NodeArgs, PandocAST } from '../framework/types';
import { renderChildren } from '../framework';
import { extractMetaString, extractMetaBool } from '../../../utils/meta';

interface PreviewDocumentArgs extends NodeArgs<PandocAST> {}

export const PreviewDocument = (args: PreviewDocumentArgs) => {
    const meta = args.node.meta ?? {};

    // Mirror Rust template.rs:415-417: page-layout defaults to "article".
    const pageLayout = extractMetaString(meta['page-layout']) ?? 'article';

    // Mirror Rust template.rs:177 + 423-429 precedence:
    //   1. User override (frontmatter `body-classes`).
    //   2. SidebarRenderTransform output — DEFERRED (Plan 2E).
    //   3. Literal "fullcontent" default.
    const bodyClasses = extractMetaString(meta['body-classes']) ?? 'fullcontent';

    // Mirror Rust template.rs:501 is_minimal_html(): minimal: true OR
    // theme: none/pandoc → skip the entire wrapper.
    const minimal =
        extractMetaBool(meta.minimal) === true ||
        extractMetaString(meta.theme) === 'none' ||
        extractMetaString(meta.theme) === 'pandoc';

    // RevealJS divergence: q2-preview doesn't render slides today (q2-debug
    // covers revealjs), but if a doc with format: revealjs ever lands in
    // q2-preview the wrapper would be wrong. Skip the wrapper for safety.
    const isRevealjs = extractMetaString(meta.format) === 'revealjs';

    // Apply body-classes to document.body imperatively. Mirrors the
    // applyTheme pattern at entry.tsx:116-131 for predictable iframe-side
    // DOM management. useEffect (vs module-scope) is fine here because
    // body-classes don't have applyTheme's race condition with the first
    // mount — they can land on the React commit.
    useEffect(() => {
        const previous = document.body.className;
        document.body.className = bodyClasses;
        return () => {
            // Restore on unmount so test re-mounts don't accumulate classes.
            document.body.className = previous;
        };
    }, [bodyClasses]);

    const children = renderChildren({
        node: args.node,
        setLocalAst: args.setLocalAst,
        currentFilePath: args.currentFilePath,
        onNavigateToDocument: args.onNavigateToDocument,
    });

    if (minimal || isRevealjs) {
        return <>{children}</>;
    }

    return (
        <div
            id="quarto-content"
            className={`quarto-container page-columns page-rows-contents page-layout-${pageLayout}`}
        >
            <main className="content" id="quarto-document-content">
                {children}
            </main>
        </div>
    );
};
```

**Key invariants** (matching `template.rs`):

- The `<div id="quarto-content">` carries exactly four classes in order: `quarto-container page-columns page-rows-contents page-layout-{layout}`. Theme CSS treats this list as load-bearing.
- The inner `<main>` carries `class="content" id="quarto-document-content"`. Both the class and the id are CSS-targeted by the bundled theme.
- The body-class precedence is user override → literal default; the SidebarRenderTransform-computed body-classes (`nav-sidebar floating` / `nav-sidebar docked`) are **explicitly out of scope** because `SidebarRenderTransform` isn't in q2-preview's pipeline (`Q2_PREVIEW_TRANSFORM_EXCLUDED`). When sidebar lands in a follow-up plan, that plan re-derives body-classes via the Option A pattern (typed field on `RenderResponse`) and wires q2-preview's resolver to be `bodyClassesOverride ?? extractMetaString(meta['body-classes']) ?? 'fullcontent'` — same precedence as `template.rs:419-429`.

### Out of scope

- **Sidebar / TOC / navbar / page-footer chrome rendering**. Each of these requires running a Rust pipeline transform that q2-preview currently excludes (sidebar-resolve, etc.). Each is a follow-up plan.
- **SidebarRenderTransform-computed body-classes** (`nav-sidebar floating` / `nav-sidebar docked`). Tied to the sidebar plan above. The body-classes precedence in 2D is user-override-or-literal-default; the typed-field-on-`RenderResponse` plumbing for transform-computed classes lands when sidebar lands.
- **Quarto Bootstrap grid layout details** (`page-columns`'s actual CSS-grid definitions, margin-sidebar tracks, etc.). The classes are emitted; theme CSS owns the visual interpretation. q2-preview ships no per-format CSS for these.
- **Custom `page-layout` values defined by user themes**. The wrapper passes the value verbatim into the class name — same as the Rust template. Theme CSS owns interpretation.
- **`format: revealjs` rendering**. q2-preview detects and skips the wrapper, but does not render slides. q2-debug owns revealjs.
- **Slide-deck-style chrome** (slide indicator, slide nav). RevealJS-only; not in scope here.

### Defensive variants

- **Missing `ast.meta`**: `args.node.meta ?? {}` defaults to empty object; all `extractMeta*` helpers return undefined; defaults kick in for every key. The wrapper still renders with `page-layout-article` and `body.fullcontent`.
- **Non-string `page-layout` value** (e.g. user wrote `page-layout: 42` accidentally): `extractMetaString` returns undefined for `MetaBool` / `MetaList` / `MetaMap`; fallback to `article`. No type coercion.
- **Whitespace-only `body-classes`** (e.g. `body-classes: " "`): the helper returns the whitespace string verbatim; `document.body.className = " "` produces a body with no classes (browser ignores whitespace-only class). Matches Rust template behavior.

## Design decisions

- **Option B (read `ast.meta` in JS), not Option A (Rust-side typed field).** The HTML pipeline reads `page-layout` directly from `ast.meta` at template-render time (`template.rs:415`); q2-preview matches that pattern. Existing precedent: `ReactRenderer.tsx:211` (format detection) and `ReactAstSlideRenderer.tsx:220-221` (slide title/author) already read `ast.meta` directly. Adds zero Rust / WASM-bridge plumbing for v1. **The Option A pattern is reserved for transform-computed values** (sidebar body-classes, future stage outputs) — when those land, `RenderResponse` grows a typed field and q2-preview's resolver layers it before the `ast.meta` read.
- **Body-class application via `useEffect`, not module-scope imperative.** `applyTheme` at `entry.tsx:116-131` lives at module scope because it has to fire before any React mount (the parent posts theme + AST from sibling `useEffect`s on the same `iframeReady` transition; if theme posts first the message would be dropped). Body-classes have no such race — they read from the AST that's already mounted, so the React commit fires before any user can observe a class mismatch. Keeps document-level concerns inside `PreviewDocument` rather than spreading across `entry.tsx` + the wrapper component.
- **Wrapper structure mirrors `template.rs` byte-for-byte.** Same element + class list + nesting. The justification is the same as Plan 2C's "Visual + structural parity target": Bootstrap-flavored theme CSS targets the wrapper's selectors, and any divergence forces a per-format CSS fork. The cost of strict mirroring is low — three classes + an id + an inner `<main>` — and the payoff is "load Quarto's compiled theme CSS and it just works".
- **`minimal` / `theme: none` / `theme: pandoc` skip the wrapper.** Mirrors `is_minimal_html()` at `template.rs:501`. The minimal HTML template at `template.rs:80-111` omits `#quarto-content` and `<main>` entirely; q2-preview's PreviewDocument matches by returning the bare Fragment. If a user opts into `minimal: true`, they expect no chrome — q2-preview shouldn't add any.
- **`format: revealjs` also skips the wrapper.** q2-preview is not the slide renderer (q2-debug owns slides), but a doc with `format: revealjs` could end up here if the user toggles formats. Skipping the wrapper keeps the layout from interfering with whatever the eventual slide renderer expects.
- **`extractMetaString` lifted to `utils/meta.ts`, not q2-preview-local.** q2-debug's slide renderer already uses it; lifting avoids parallel copies. The new file is `hub-client/src/utils/meta.ts` (sibling of `customRegistry.ts`, `iframeLinkHandlers.ts`, `atomicCustomNodes.ts`). One-line update to the existing q2-debug import.
- **Cleanup on `useEffect` unmount.** Test re-mounts (vitest, Playwright) need a clean body class slate; restoring the previous `document.body.className` on unmount keeps the suite hermetic. Production iframe unmounts only on document switch — same restore is benign there.

## Multi-plan contracts

### Consumed: Plan 2B / 2C (PreviewDocument exists)

`PreviewDocument.tsx` ships in Plan 2B as the registry's `Ast` entry. 2D extends it with the wrapper. Plan 2C's registry-unification work (single `previewRegistry`, dispatchers in `dispatchers.tsx`) is orthogonal — 2D does not touch the registry shape. **Plan 2D can land in parallel with Plan 2C** — they touch overlapping directories but distinct files (PreviewDocument.tsx vs. registry.ts / dispatchers.tsx). If both land in the same session, the merge is trivial.

### Provided: foundation for sidebar / navbar / footer plans

When Plan 2E (sidebar) lands, the `<div id="quarto-content">` wrapper is the slot the sidebar `<nav>` and `<div id="quarto-margin-sidebar">` siblings live alongside `<main class="content">`. The wrapper structure assumed by 2D is exactly the structure those follow-up plans will extend, so 2E adds elements without restructuring the wrapper.

When the typed-field-on-`RenderResponse` pattern is used for transform-computed body-classes (Plan 2E or later), q2-preview's resolver layers the typed field above the `ast.meta` read:

```ts
// Future shape (Plan 2E):
const bodyClasses =
    props.bodyClassesOverride ??                            // Plan 2E: typed field on RenderResponse
    extractMetaString(meta['body-classes']) ??              // Plan 2D: user override
    'fullcontent';                                          // Plan 2D: literal default
```

Mirrors `template.rs:419-429` precedence.

### Soft activation dependencies

None for 2D. The wrapper's structure is fully determined by `ast.meta` reads, and `ast.meta` is already plumbed end-to-end (Plan 2A's WASM bridge, Plan 2B's iframe entry, no changes needed).

## Test plan

### Test-tier conventions

Same tiers as Plans 2B / 2C: vitest unit / vitest integration / smoke-all WASM / Playwright e2e. The project-context coverage rule does **not** apply to 2D — the body container is computed entirely from `ast.meta`, which is identical in single-doc and project mode (the orchestrator merges the same metadata in both paths). Single-doc fixtures are sufficient.

### Vitest unit tests (`utils/meta.test.ts`)

NEW file at `hub-client/src/utils/meta.test.ts`:

- `extractMetaString` returns string for `MetaString`, walks `MetaInlines` via `inlinesToPlainText`, returns undefined for `MetaBool` / `MetaList` / `MetaMap` / null / undefined / wrong-shape.
- `extractMetaBool` returns boolean for `MetaBool`, parses `MetaString("true" | "false")`, returns undefined for other types.

### Vitest snapshot tests (`q2-preview/PreviewDocument.test.tsx`)

NEW file (or extend if already present post-2B/2C):

- Default render (no metadata): assert wrapper with `page-layout-article` and `<main class="content" id="quarto-document-content">`. Body class is `fullcontent`.
- `page-layout: full`: assert wrapper carries `page-layout-full`.
- `page-layout: custom`: assert wrapper carries `page-layout-custom` (verbatim — the wrapper doesn't validate against an enum).
- `body-classes: my-class`: assert `document.body.className === 'my-class'` AND that `fullcontent` is NOT applied.
- `minimal: true`: assert NO wrapper element rendered (just children inside a Fragment).
- `theme: none`: assert NO wrapper element rendered.
- `theme: pandoc`: assert NO wrapper element rendered.
- `format: revealjs`: assert NO wrapper element rendered.
- Cleanup: mount → unmount → assert `document.body.className` is restored to its pre-mount value.

### Smoke-all q2-preview fixtures

Pattern matches Plan 2C item 5.2 — `_quarto.tests.run.requires_js: true` + `ensureHtmlElements` assertions:

The `ensureHtmlElements` schema (verified at `hub-client/e2e/helpers/smokeAllDiscovery.ts:124-129` and `smokeAllAssertions.ts:122-138`) is a two-element YAML array: first inner array is selectors that must match (`toBeAttached`), second is selectors that must NOT match (`toHaveCount(0)`). Negative selectors are first-class in the harness; 2D uses both halves:

```yaml
_quarto:
  tests:
    run:
      requires_js: true
    q2-preview:
      ensureHtmlElements:
        - ['div#quarto-content.page-layout-article', 'main.content#quarto-document-content', 'body.fullcontent']
        - []  # no negatives for the default case
```

Per-fixture assertions:

- **`q2-preview/body-container-default.qmd`**: no `page-layout` / `body-classes` / `minimal`. Positives: `['div#quarto-content.page-layout-article', 'main.content#quarto-document-content', 'body.fullcontent']`. Negatives: none.
- **`q2-preview/body-container-full-layout.qmd`**: `page-layout: full`. Positives: `['div.page-layout-full', 'main.content']`. Negatives: `['div.page-layout-article']` (defends against the default leaking through).
- **`q2-preview/body-container-override.qmd`**: `body-classes: custom-cls`. Positives: `['body.custom-cls']`. Negatives: `['body.fullcontent']` (locks the override-replaces-default precedence).
- **`q2-preview/body-container-minimal.qmd`**: `minimal: true`. Positives: `['p']` (some content rendered). Negatives: `['div#quarto-content', 'main.content']` (locks the wrapper-skip behavior).

### Visual sanity check (manual)

During Phase 6.5's manual browser session:

- Open a multi-element fixture (the one Plan 2C ships at `q2-preview/multi-element-doc.qmd`) in q2-preview through a running hub.
- Confirm in DevTools that the body-container wrapper is in place, classes correct.
- Reload with theme CSS applied; confirm the document looks like the HTML pipeline's output (theme CSS targeting `body.fullcontent .content` should land).
- Toggle `page-layout: full` in frontmatter, save, confirm the iframe re-renders with `page-layout-full`.

Record the inspected output snippet in the implementation transcript.

## Risk areas

- **Theme CSS expectation drift**. Theme CSS is bundled by the Rust pipeline and shipped through `theme_fingerprint` / `applyTheme`. If the bundled theme CSS uses a selector q2-preview doesn't emit (e.g. `body.fullcontent .content > article`), 2D's wrapper won't satisfy it. Mitigation: the smoke fixtures assert the wrapper structure post-render; the manual visual check at 6.5 catches the rest. If a divergence is found, the fix is to extend the wrapper, not the theme CSS.
- **Wrapper drift between Rust template and PreviewDocument**. The classes / element / nesting pattern is replicated in two places (template.rs literal HTML, PreviewDocument.tsx JSX). If template.rs changes its wrapper (e.g. adds `quarto-document-grid` to the outer container's class list), q2-preview won't pick it up automatically. Mitigation: doc-comment the PreviewDocument wrapper with an explicit "MIRRORS template.rs:185-209 — keep in sync" pin and a line ref. Same drift-detection caveat as Plan 2C's `quartoClasses.ts`.
- **`document.body.className` collision with iframe-host CSS — verified safe**. The iframe host (`hub-client/public/q2-preview.html:28`) declares `<body>` with no class attribute. The host CSS at lines 11-14 only sets `margin: 0; padding: 0` on the bare `body` selector, no class dependency. `useEffect`'s `document.body.className = bodyClasses` is a clean overwrite — no host class to preserve, no iframe-host CSS rule that depends on a class being present. The cleanup-on-unmount restores the previous value (which is the empty string on first mount); subsequent re-mounts restore the prior `bodyClasses` cleanly.
- **Body-class restore on unmount races with re-mount**. If the iframe unmounts and immediately re-mounts (e.g. doc switch), the cleanup-then-reapply sequence may briefly flash `fullcontent` between the old doc's class and the new one. Visually invisible at React's commit cadence (microsecond), but worth noting if a future bug points at brief unstyled flashes. Mitigation: if it bites, switch to module-scope imperative management like `applyTheme`.
- **Non-string `format` field**. `format` in `ast.meta` is the merged-format ID; should always be `MetaString`. If a future pipeline change makes it `MetaInlines`, the revealjs-skip branch still works (extractMetaString handles both), but worth a regression test if format-detection is ever centralized.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `hub-client/src/utils/meta.ts` (NEW — `extractMetaString` + `extractMetaBool`) | ~30 |
| `q2-preview/PreviewDocument.tsx` extension (wrapper + useEffect) | ~30 |
| q2-debug import update (`ReactAstSlideRenderer.tsx:350` lifted) | ~3 |
| `utils/meta.test.ts` (NEW) — unit tests for both helpers | ~50 |
| `q2-preview/PreviewDocument.test.tsx` extension — wrapper snapshot tests | ~80 |
| Smoke-all q2-preview fixtures (4 single-doc fixtures, no project mode) | ~40 |
| **Total** | **~233** |

Comfortable for a focused session. ~one-fifth the size of 2C.

**Sub-ordering**: utils/meta.ts + meta.test.ts land first (the helper commit). q2-debug import update is bundled in the same commit. Then PreviewDocument.tsx extension + tests. Then smoke fixtures. Then verification.

## Dependencies

### Hard dependencies

- **Plan 2B** ✅ — ships `PreviewDocument.tsx` as the registry's `Ast` entry and `inlinesToPlainText` (used by `extractMetaString`). 2D extends both.
- **Plan 2A** ✅ — q2-preview surface scaffolding, `theme_fingerprint` plumbing (the iframe already receives theme CSS that targets the wrapper).
- **Plan 1** ✅ — pipeline + format detection. `Q2_PREVIEW_TRANSFORM_EXCLUDED` defines what runs in q2-preview's render path; 2D inherits.

### Soft / activation dependencies

None. The wrapper structure is fully determined by `ast.meta` reads available today.

### Blocks

Nothing structurally. **Plan 2C can land in parallel** — registry / dispatcher work is orthogonal to PreviewDocument's wrapper. The future sidebar plan (Plan 2E or similar) sits on top of 2D's wrapper, so 2D blocks 2E temporally but not in parallel-development terms.

## Related beads issues

None tracked specifically for this work. The `body-classes` and "Layout / chrome components" deferrals in Plans 2A and 2C are notes-only, not beads issues.

## Notes

- This plan is the smallest possible step toward HTML-pipeline visual parity for q2-preview. Sidebar / TOC / navbar / page-footer each warrant their own plans (each requires unblocking a Rust pipeline transform on the q2-preview side, which is its own design surface). 2D ships the foundation those sit on.
- `extractMetaString` already exists in q2-debug's slide renderer; the lift is opportunistic — once it's at `utils/meta.ts`, future format renderers (e.g. a docx preview, a PDF preview hypothetically) get the helper for free without re-importing from q2-debug.
- `extractMetaBool` is new. Could be deferred until a real consumer needs it (today only `minimal` reads it, and a `=== 'true'`-style string check would do). Including it here because (a) the `minimal: true` YAML form parses to MetaBool, not MetaString, so the type-correct version is needed; (b) the additional 6 LOC + 4 test cases is cheap, and bundling avoids a separate "add extractMetaBool" commit later.

## References

### Rust side (read during implementation; not modified by 2D)

- `crates/quarto-core/src/template.rs:140-256` — `FULL_HTML_TEMPLATE` (the wrapper structure 2D mirrors).
- `crates/quarto-core/src/template.rs:177` — `<body class="$if(body-classes)$$body-classes$$else$fullcontent$endif$">` (body-class precedence reference).
- `crates/quarto-core/src/template.rs:185-209` — `<div id="quarto-content">` wrapper definition (element + classes).
- `crates/quarto-core/src/template.rs:415-417` — `page-layout` template-variable injection with default.
- `crates/quarto-core/src/template.rs:419-429` — body-classes computation precedence (user override → `rendered.navigation.body-classes` → literal default).
- `crates/quarto-core/src/template.rs:501` — `is_minimal_html()` check.
- `crates/quarto-core/src/template.rs:80-111` — `MINIMAL_HTML_TEMPLATE` (the no-wrapper variant 2D matches when minimal is set).
- `crates/quarto-core/src/transforms/sidebar_render.rs:88-97` — SidebarRenderTransform body-classes computation (out of scope for 2D; documented for the future sidebar plan).

### hub-client side (modified by 2D)

- `hub-client/src/utils/meta.ts` (NEW) — `extractMetaString`, `extractMetaBool`.
- `hub-client/src/utils/meta.test.ts` (NEW) — unit tests.
- `hub-client/src/components/render/q2-preview/PreviewDocument.tsx` — extend with body-container wrapper + useEffect.
- `hub-client/src/components/render/q2-preview/PreviewDocument.test.tsx` (extend if exists, NEW otherwise) — snapshot tests.
- `hub-client/src/components/render/ReactAstSlideRenderer.tsx:350` — remove the local `extractMetaString` definition; update the import at `:220-221` to use `utils/meta.ts`.
- `crates/quarto/tests/smoke-all/q2-preview/body-container-{default,full-layout,override,minimal}.qmd` (NEW) — smoke fixtures.

### hub-client side (read-only references during implementation)

- `hub-client/public/q2-preview.html:28` — iframe `<body>` declaration (verified to carry no class — `useEffect` overwrite is safe).
- `hub-client/e2e/helpers/smokeAllDiscovery.ts:124-129` — `parseTwoArraySpec` defines the `ensureHtmlElements` YAML schema (two arrays: positives + negatives).
- `hub-client/e2e/helpers/smokeAllAssertions.ts:122-138` — `ensureHtmlElements` runner; positives use `toBeAttached`, negatives use `toHaveCount(0)`.

## Revision history

- **2026-05-10**: initial draft. Decision context: chose Option B (read `ast.meta` in JS) over Option A (typed field on `RenderResponse`) for v1 because (a) `ast.meta` reads are already idiomatic in the codebase (`ReactRenderer.tsx:211`, `ReactAstSlideRenderer.tsx:220-221`), (b) sidebar-derived body-classes — the only fields that would justify Option A's plumbing — aren't computed in q2-preview's pipeline today, so Option A would buy nothing for v1, (c) Option B keeps the plan scoped to hub-client with no Rust changes. Body-class application via `useEffect` (not module-scope imperative) chosen because there's no race-against-first-mount like `applyTheme` has — the AST is already parsed by the time the React commit runs. The Option A → typed field on `RenderResponse` pattern is reserved for the future sidebar plan, where it's load-bearing.

- **2026-05-10 (risk-area resolution)**: two risk areas flagged for "verify before implementation" in the initial draft are now resolved:
  - **Iframe host body class**: verified that `hub-client/public/q2-preview.html:28` declares `<body>` with no class attribute and host CSS only targets the bare `body` selector. `useEffect`'s `document.body.className = bodyClasses` overwrite is safe; no host-CSS dependency to preserve. Risk-area entry downgraded from "verify before implementation" to "verified safe" with a file:line citation.
  - **`ensureHtmlElements` negative selectors**: verified that the harness already supports negatives via the two-array YAML schema (`smokeAllDiscovery.ts:124-129` for parsing; `smokeAllAssertions.ts:122-138` for the runner). Negatives use Playwright's `toHaveCount(0)`. Smoke-fixture descriptions in §Test plan rewritten to use the actual two-array schema with both positive and negative selector lists per fixture.
