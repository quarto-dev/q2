# PreviewDocument.tsx merge-resolution briefing

**File:** `ts-packages/preview-renderer/src/q2-preview/PreviewDocument.tsx`
**Conflicts:** 3 regions (lines ~116, ~200, ~237 after the user's other merge work)
**Hard question raised by user:** "render_page_for_preview doesn't accept attribution params — does that mean PreviewDocument shouldn't take them either? But the conflict shows it apparently does."

## Short answer to the hard question

**The user's intuition is correct on the substance, but the conflict isn't actually saying PreviewDocument *takes attribution as a prop*.** It doesn't. Look at the component signature (lines 45–53):

```tsx
export const PreviewDocument = ({
    ast,
    onNavigateToDocument,
    setAst,
}: {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}) => { ... };
```

No `attribution` prop. The attribution surface enters via **React context** (`AttributionLookupContext`), which is provided by `framework/Ast.tsx` from the JSON's `astContext.attribution*` fields — i.e., from inside the AST payload itself. The hook PR #190 added (`useAttributionHover()`) reads that context.

So the relationship to `render_page_for_preview`'s missing attribution params plays out like this:

1. **Today** (post-merge): `render_page_for_preview` does not install a `PreBuiltAttributionProvider`. The resulting AST JSON has empty `astContext.attribution` / `attributionActors`.
2. `framework/Ast.tsx` builds a lookup map from those empty fields → `AttributionLookupContext` gets `null` (or an empty `Map`).
3. `useAttributionHover()` checks the context, sees it's empty, and returns **inert wiring**: `{ enabled: false, stylesheet: null, hostProps: {}, overlay: null }` (the exact shape; check `attribution.tsx`).
4. Rendering interpolates `{attr.stylesheet}` (renders nothing), `{...attr.hostProps}` (spreads `{}`, no-op), `{attr.overlay}` (renders nothing). **DOM-identical to pre-attribution.**
5. When `render_page_for_preview` is eventually extended to thread attribution, the same code lights up — no further React changes needed.

Main's PR #190 designed `useAttributionHover` this way on purpose ("returns inert wiring when AttributionLookupContext is unpopulated — off-path DOM stays byte-identical to pre-attribution" — comment in the resolved component). The attribution branches in `PreviewDocument.tsx` are **dormant infrastructure** on the preview path until the Rust side wires the provider through.

So: **keep main's attribution wiring during this merge**. It's a no-op today and lights up later. The follow-up issue to fix in `render_page_for_preview` is exactly the producer-side gap you noted; the consumer side (`PreviewDocument.tsx`) is ready for it.

## Conflict-by-conflict resolution

### Conflict 1 (variable declarations, lines ~116–157)

The two sides declare independent things at the same spot. Both must live:

- **Feature** declares the Phase F.2 chrome strings — `navbarHtml`, `sidebarHtml`, `pageNavHtml`, `tocHtml`, `footerHtml`, `tocTitle`, `headerIncludes` — pulled from `meta.rendered.*`. These drive the `<NavbarSlot>` / `<SidebarSlot>` / etc. tags below.
- **Main** declares `const attr = useAttributionHover();` — the inert-by-default attribution wiring.

**Resolution (concat both):**

```tsx
    // Phase F.2 (bd-kw93.15): chrome HTML strings populated by the
    // `*-render` transforms now in the q2-preview pipeline. Each
    // slot is React.memo'd so an identical re-post (edit to body
    // content) doesn't tear down the chrome DOM.
    const navbarHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'navbar']),
    );
    const sidebarHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'sidebar']),
    );
    const pageNavHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'page_navigation']),
    );
    const tocHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'toc']),
    );
    const footerHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'footer']),
    );
    const tocTitle =
        extractMetaString(getMetaPath(meta, ['navigation', 'toc', 'title'])) ??
        '';
    const headerIncludes = extractMetaStringList(
        getMetaPath(meta, ['rendered', 'includes', 'header']),
    );

    // Attribution wiring (Phase 3 of `2026-05-13-q2-preview-attribution.md`):
    // returns inert when AttributionLookupContext is unpopulated, so the
    // preview path is byte-identical to pre-attribution until
    // `render_page_for_preview` is extended to thread attribution.
    // (Tracking issue: file one to update render_page_for_preview to
    // accept and forward attribution_json.)
    const attr = useAttributionHover();
```

### Conflict 2 (non-minimal JSX top, lines ~200–229)

Structural overlap on the `<div id="quarto-content">` element. Both sides want to add things *to and around* it, in compatible-but-conflicting ways.

- **Feature** wraps the div with `<HeaderIncludesEffect>` + `<NavbarSlot>` outside, `<SidebarSlot>` + `<TocSlot>` inside-before-main.
- **Main** puts `{attr.stylesheet}` before the div and spreads `{...attr.hostProps}` onto the div.

The two are orthogonal. Combine:

```tsx
    return (
        <>
            {/* Attribution stylesheet — inert (renders nothing) when
                attribution context is empty. Lives next to header-
                includes since both are document-head-adjacent. */}
            {attr.stylesheet}

            {/* Phase F.2: header-includes (favicon, RSS links, user
                includes) appended imperatively to `document.head`. */}
            <HeaderIncludesEffect items={headerIncludes} />

            {/* Navbar lives BEFORE quarto-content (template.rs:178-180). */}
            {navbarHtml ? <NavbarSlot html={navbarHtml} /> : null}

            <div
                id="quarto-content"
                className={`quarto-container page-columns page-rows-contents page-layout-${pageLayout}`}
                {...attr.hostProps}
            >
                {/* Sidebar — INSIDE quarto-content, before TOC + main
                    (template.rs:186-188). */}
                {sidebarHtml ? <SidebarSlot html={sidebarHtml} /> : null}

                {/* TOC — INSIDE quarto-content, before main
                    (template.rs:189-200). */}
                {tocHtml ? <TocSlot html={tocHtml} title={tocTitle} /> : null}
```

**Why `{...attr.hostProps}` goes on `<div id="quarto-content">` and not on `<main>`:** that's where main put it, and it's the right host — `attr.hostProps` carries an `onMouseOver` delegation that should catch hover events across the whole document body, not just the `<main>` interior (which would miss hovers into the chrome). `attr.hostProps` is empty in inert form, so this spread is a no-op today; it lights up when attribution is on.

### Conflict 3 (non-minimal JSX bottom, lines ~237–251)

Same shape as Conflict 2 — orthogonal additions trying to occupy the closing region.

- **Feature** keeps `<PageNavSlot>` inside `<main>` (after body content, before main closes), then closes the div, then emits `<FooterSlot>` after the div (matching `template.rs:244–254`).
- **Main** closes `<main>` and `<div>` straight, then emits `{attr.overlay}` near the end.

Combine:

```tsx
                    {children}

                    {/* Page-nav (prev/next) — INSIDE main, after body
                        content (template.rs:244-246). */}
                    {pageNavHtml ? <PageNavSlot html={pageNavHtml} /> : null}
                </main>
            </div>

            {/* Page-footer lives AFTER quarto-content
                (template.rs:252-254). */}
            {footerHtml ? <FooterSlot html={footerHtml} /> : null}

            {/* Attribution overlay (inert when off-path). */}
            {attr.overlay}
        </>
    );
};
```

## Minimal-mode branch (already auto-merged — no conflict here)

Worth noting that the minimal branch (lines ~165–198) was auto-merged correctly and demonstrates the design clearly: it conditionally introduces a host `<div>` *only when `attr.enabled`*, otherwise stays on the Fragment:

```tsx
        if (attr.enabled) {
            return (
                <>
                    {attr.stylesheet}
                    <div {...attr.hostProps}>{minimalInner}</div>
                    {attr.overlay}
                </>
            );
        }
        return minimalInner;
```

The non-minimal branch doesn't need this gate because its host `<div id="quarto-content">` exists unconditionally for Bootstrap layout — `{...attr.hostProps}` just spreads an empty object on it when off-path.

## Validation steps after applying the resolution

1. `cd ts-packages/preview-renderer && npx tsc --noEmit` — should go clean. The pre-resolution errors all clustered on conflict-marker lines.
2. `cd hub-client && npm run build:all` — production build is stricter than vitest; will catch any project-references issue.
3. Run the parity tests: `cd hub-client && npx vitest run framework parity.integration` — the framework-primitive parity test (Plan 2A item 14) walks the rendered DOM and would catch surprise changes from a wrong merge.
4. Sanity-check `q2 render docs/` still works (the navbar regression test we set up earlier).

## Follow-up issue worth filing

When you wrap this up, a new beads issue tracking the producer-side gap:

> **Title:** q2-preview: thread attribution through render_page_for_preview
> **Type:** feature
> **Description:** The hub-client's q2-preview surface has attribution-ready consumer wiring (`useAttributionHover`, `<AttributionWrap>` on dispatchers, `{attr.stylesheet}` / `{...attr.hostProps}` / `{attr.overlay}` interpolated in `PreviewDocument.tsx`). All of this is inert today because `render_page_for_preview` (in `crates/wasm-quarto-hub-client/src/lib.rs`) doesn't accept an `attribution_json` argument, so the active-page ctx never gets a `PreBuiltAttributionProvider` and the resulting AST JSON's `astContext.attribution*` fields are empty.
>
> Extending `render_page_for_preview` to accept `attribution_json: Option<String>` and forward it through the same machinery `parse_qmd_to_ast_with_attribution` / `render_page_in_project_with_attribution` use is the missing piece. Once it lands, the React side lights up automatically — no further hub-client changes required.
>
> Discovered while merging origin/main into feature/q2-preview-command (PR #190 attribution pipeline). See `claude-notes/research/2026-05-19-attribution-merge-briefing.md` and `claude-notes/research/2026-05-19-PreviewDocument-merge-resolution.md`.
