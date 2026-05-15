import { useContext, useEffect } from 'react';
import {
    renderChildren,
    extractMetaString,
    extractMetaBool,
    RegistryContext,
    useAttributionHover,
} from '../framework';
import type { BlockNode, PandocAST } from '../framework';
import * as Custom from './custom';

/**
 * q2-preview's document-root wrapper. Registered into `registry.ts`
 * under the `'Ast'` key. Mirrors the HTML pipeline's
 * `<div id="quarto-content"><main class="content">` wrapper at
 * `crates/quarto-core/src/template.rs:185-247` byte-for-byte.
 *
 * `body-classes` is applied imperatively to `document.body` in a
 * `useEffect`; the wrapper itself emits the inner `<div>` + `<main>`.
 * `minimal: true` and `theme: none | pandoc` skip the wrapper entirely
 * (mirrors Rust's `is_minimal_html()` at `format.rs:306-318`).
 *
 * Minimal-mode title synthesis (Phase 6.2): the Rust `title-block`
 * transform is in `Q2_PREVIEW_TRANSFORM_EXCLUDED`
 * (`pipeline.rs:1052`), so without React-side replication q2-preview's
 * minimal mode silently drops the title. When the document is
 * minimal AND has a `title` AND no user-authored level-1 Header,
 * prepend a synthetic `<h1>{title}</h1>`.
 *
 * Iframe `<title>` (Phase 6.2a): when `meta.pagetitle` or `meta.title`
 * resolves to a non-empty string, write `document.title`; restore on
 * unmount. When neither resolves, leave the static title from
 * `q2-preview.html:6` alone.
 */
export const PreviewDocument = ({
    ast,
    onNavigateToDocument,
    setAst,
}: {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}) => {
    const meta = ast.meta ?? {};

    // Mirror Rust template.rs:415-417: page-layout defaults to "article".
    const pageLayout = extractMetaString(meta['page-layout']) ?? 'article';

    // Mirror Rust template.rs:177 body-class default. The
    // SidebarRenderTransform-computed value isn't in q2-preview's
    // pipeline (Q2_PREVIEW_TRANSFORM_EXCLUDED), so the precedence here
    // is user override → literal default. Empty string is the user's
    // opt-out (matches Rust's $body-classes$ substitution); only
    // `undefined` triggers the fallback.
    const bodyClassesValue = extractMetaString(meta['body-classes']);
    const bodyClasses = bodyClassesValue ?? 'fullcontent';

    // Mirror Rust is_minimal_html() (format.rs:306-318).
    const minimal =
        extractMetaBool(meta.minimal) === true ||
        extractMetaString(meta.theme) === 'none' ||
        extractMetaString(meta.theme) === 'pandoc';

    // Resolve the title-block component via the registry so user TSX
    // can override it under the synthetic `__title_block__` key.
    // Pattern matches the dispatchers at `dispatchers.tsx:39`. Called
    // unconditionally before any early returns to keep hook order
    // stable (React rules-of-hooks).
    const { registry } = useContext(RegistryContext);
    const TitleBlock = registry.__title_block__ ?? Custom.PreviewTitleBlock;

    // Imperative body-class management. Restore on unmount so test
    // re-mounts (vitest, Playwright) start with a clean slate.
    useEffect(() => {
        const previous = document.body.className;
        document.body.className = bodyClasses;
        return () => {
            document.body.className = previous;
        };
    }, [bodyClasses]);

    // Iframe `<title>` wiring. Writes only when an AST title resolves
    // to a non-empty string. Cleanup restores the pre-mount value
    // (which equals the static literal from q2-preview.html:6 on
    // first mount).
    useEffect(() => {
        const next =
            extractMetaString(meta.pagetitle) ??
            extractMetaString(meta.title);
        if (!next) return;
        const previous = document.title;
        document.title = next;
        return () => {
            document.title = previous;
        };
    }, [meta]);

    // Attribution wiring (Phase 3 of
    // `2026-05-13-q2-preview-attribution.md`): delegated to
    // `useAttributionHover`, which returns inert wiring when
    // `AttributionLookupContext` is unpopulated — off-path DOM stays
    // byte-identical to pre-attribution. Same hook is consumed by
    // q2-debug's `AstRenderer`.
    const attr = useAttributionHover();

    const children = renderChildren({
        node: ast as any,
        setLocalAst: setAst as any,
        onNavigateToDocument,
    });

    if (minimal) {
        // Re-implement the Rust title-block transform's minimal-mode
        // branch on the React side. Rust's transform (excluded from
        // q2-preview's pipeline; behavior at
        // transforms/title_block.rs:54-110) prepends a synthetic
        // level-1 Header from meta.title when minimal AND no
        // user-authored level-1 Header exists.
        const title = extractMetaString(meta.title);
        const hasLevel1Header = (ast.blocks ?? []).some(
            (b: BlockNode) =>
                (b as { t?: string }).t === 'Header' &&
                Array.isArray((b as { c?: unknown[] }).c) &&
                ((b as { c: unknown[] }).c[0] === 1),
        );
        const minimalInner = (
            <>
                {title && !hasLevel1Header ? <h1>{title}</h1> : null}
                {children}
            </>
        );
        // When attribution is on we need a host element to carry the
        // mouseover delegation. Off-path stay on the Fragment so the
        // minimal-mode DOM is byte-identical to today's.
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
    }

    return (
        <>
            {attr.stylesheet}
            <div
                id="quarto-content"
                className={`quarto-container page-columns page-rows-contents page-layout-${pageLayout}`}
                {...attr.hostProps}
            >
                <main className="content" id="quarto-document-content">
                    <TitleBlock
                        ast={ast}
                        setAst={setAst}
                        onNavigateToDocument={onNavigateToDocument}
                    />
                    {children}
                </main>
            </div>
            {attr.overlay}
        </>
    );
};
