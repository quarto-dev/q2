/**
 * Entry point for the q2-preview renderer iframe.
 *
 * Loaded by `/q2-preview.html` and handles the postMessage protocol
 * with the parent (`Q2PreviewIframe`). Mounts the framework's `<Ast>`
 * with `previewRegistry` as the format-side defaults, layered with any
 * user-TSX overrides loaded via `LOAD_CUSTOM_COMPONENTS`.
 *
 * Two structural differences from `q2-debug/entry.tsx`:
 *
 *  1. `UPDATE_THEME` is handled at module top (not inside any React
 *     component). The handler imperatively manages a single
 *     `<link rel="stylesheet" data-q2-theme>` element in `document.head`
 *     — pure DOM, no React state. This avoids a race: if the listener
 *     lived in `PreviewRoot`'s `useEffect`, it would only attach after
 *     React commits the mount triggered by the first `UPDATE_AST`. The
 *     parent posts theme + AST from sibling `useEffect`s on the same
 *     `iframeReady` transition; if theme posts first the message would
 *     be dropped.
 *
 *  2. `__Q2_PREVIEW_RENDERER__` is set at module top (not inside
 *     `loadCustomComponents`). This makes the renderer surface
 *     importable in tests without firing `LOAD_CUSTOM_COMPONENTS`
 *     setup messages — the framework-primitive parity test (Plan 2A
 *     item 14) relies on this.
 *
 *  3. `PreviewRoot` is extracted to `PreviewRoot.tsx` so tests can
 *     mount it directly without importing these module-top side effects.
 */

import { createRoot } from 'react-dom/client';
import React from 'react';
import katex from 'katex';
import 'katex/dist/katex.min.css';
// Phase F.1 (bd-kw93.14): Bootstrap 5 bundled JS, vendored at the
// repo's `resources/js/bootstrap/`. We embed it as raw text and
// inject as an inline `<script>` at module top so Bootstrap's
// data-API click delegates are attached before any chrome HTML
// arrives. The vendored copy is paired-versioned with the SCSS at
// `resources/scss/bootstrap/` (5.3.1 today). See the Phase F plan
// for the rationale: `BootstrapJsStage` could ride the WASM
// pipeline (math_js.rs precedent), but q2-preview also excludes
// `ApplyTemplateStage` so the JS would never get a `<script>` tag.
// Static iframe injection is the cleaner separation — chrome JS is
// iframe-template responsibility, not document-render responsibility.
// `?raw` is typed via `src/global.d.ts` (Vite's `?raw` suffix
// returns a string at build time).
import bootstrapJsSrc from '../../../../resources/js/bootstrap/bootstrap.bundle.min.js?raw';

(() => {
    const existing = document.head.querySelector('script[data-q2-bootstrap]');
    if (existing) return;
    const tag = document.createElement('script');
    tag.setAttribute('data-q2-bootstrap', '1');
    tag.textContent = bootstrapJsSrc;
    document.head.appendChild(tag);
})();

import {
    Node,
    renderChildren,
    renderNode,
    rewrapCustomNodes,
    extractMetaString,
    extractMetaBool,
    extractMetaStringList,
    inlinesToPlainText,
    blocksToPlainText,
    AttributionLookupContext,
    useNodeAttribution,
    CurrentActorContext,
    useCurrentActor,
} from '../framework';
import { Block, Inline, previewRegistry, usePreviewEdit } from '.';
import { renderSlot } from './utils';
import { PreviewTitleBlock } from './custom/PreviewTitleBlock';
import {
    buildCustomRegistry,
    type ComponentExports,
} from '../utils/customRegistry';
import { PreviewRoot } from './PreviewRoot';

// Set the renderer-surface global at module top. Importing this module
// is sufficient to populate `window.__Q2_PREVIEW_RENDERER__`. The
// explicit-object form (rather than `{ ...framework, ...preview }`
// spread) keeps framework internals (`renderChildrenRegistry`,
// `RegistryContext`) off the global and locks the public surface.
//
// Plan 2C exposes `renderSlot` so user TSX overrides of CustomNode
// components can recurse into named slots (Callout's title/content,
// FloatRefTarget's caption_long/caption_short, ...) without
// reimplementing the per-slot setLocalAst plumbing.
//
// Plan 2D (6.0c.1) exposes the framework-tier meta and plain-text
// helpers so a user TSX override of `__title_block__` can coerce
// `ast.meta` values without re-implementing the Pandoc-AST walks.
// Plan 2D (7.3.1) exposes `PreviewTitleBlock` so a user override
// can compose the built-in chrome (e.g. wrap it and add a DOI line)
// instead of re-implementing it from scratch.
//
// Reactji-authorship demo (2026-05-25) exposes `useNodeAttribution`
// + `AttributionLookupContext` (Plan 5 wire surfaces) so user TSX can
// resolve per-node authorship from the attribution lookup map that
// `framework/Ast.tsx` provides. `useCurrentActor` is added below in
// the `CurrentActorContext` block once the iframe payload carries the
// actor id.
(window as any).__Q2_PREVIEW_RENDERER__ = {
    renderChildren,
    renderNode,
    renderSlot,
    Node,
    Block,
    Inline,
    previewRegistry,
    extractMetaString,
    extractMetaBool,
    extractMetaStringList,
    inlinesToPlainText,
    blocksToPlainText,
    PreviewTitleBlock,
    usePreviewEdit,
    useNodeAttribution,
    AttributionLookupContext,
    useCurrentActor,
    CurrentActorContext,
};

let root: ReturnType<typeof createRoot> | null = null;
let customRegistry: Record<string, React.ComponentType<any>> = {};
let componentsLoading = false;

interface UpdateAstPayload {
    astJson: string;
    currentFilePath: string;
    /**
     * Pre-pipeline (untransformed) AST JSON shipped in lockstep with
     * `astJson` + `renderedContent` (same compound-state generation).
     * Received by `PreviewRoot` to build `sourceIndex` for the
     * structural editability gate (Plan 2a).
     */
    untransformedAstJson?: string | null;
    /**
     * Manifest of `{ origPath → blobUrl }` produced by the parent's
     * `assetWalker.ts`. Forwarded into `AssetManifestContext` so
     * `<Image>` can resolve project-relative URLs to blob URLs without
     * any VFS access in the iframe. External URLs (`https?:`, `data:`,
     * `//`) are not in the manifest — `lookupAssetUrl` passes them
     * through.
     */
    assetManifest?: Record<string, string>;
    /**
     * Phase F.1 (bd-kw93.14): project file paths (no leading slash)
     * forwarded into the iframe link handler so artifact-rooted
     * `.html` clicks can be reverse-mapped to source `.qmd`. Used
     * for documentation today — `installLinkHandlers` always
     * intercepts artifact-rooted hrefs.
     */
    projectFilePaths?: readonly string[];
    /**
     * Phase F.1 (bd-kw93.14): anchor (without `#`) to scroll into
     * view after React commits this AST. Set on cross-page nav and
     * back/forward; null/undefined means "no pending scroll".
     *
     * Paired with `pendingAnchorEpoch`: the iframe scrolls only when
     * the epoch ticks past what it last saw. Re-renders from edits
     * carry the same epoch so they don't trigger a re-scroll.
     */
    pendingAnchor?: string | null;
    pendingAnchorEpoch?: number;
    renderedContent?: string;
    /**
     * Reactji-authorship demo (2026-05-25 plan): current viewer's
     * Automerge actor id. Provided via `CurrentActorContext` to user
     * TSX so `useCurrentActor()` can drive `actor === me` checks.
     */
    currentActor?: string | null;
    /**
     * Globally disable the edit surface (bd-ov4gqk3m). Forwarded into
     * `PreviewContext.editingDisabled`; set by read-only hosts
     * (`q2 preview` without `--allow-edit`). Absent/false ⇒ editable.
     */
    editingDisabled?: boolean;
    /**
     * P3.2: nesting-cursor mode for nested blocks. Forwarded into
     * `PreviewContext.unlockNestingCursor`. Default-off (undefined/false).
     */
    unlockNestingCursor?: boolean;
    /**
     * P3.2: per-siKey clean QMD buffers for nested blocks. Forwarded
     * into `PreviewContext.nestedEditBuffers`. Undefined when flag is off.
     */
    nestedEditBuffers?: Record<string, string>;
}

// Module-top message handler. Registered before `IFRAME_READY` is
// posted so the parent's `UPDATE_THEME` (which can fire immediately
// after `IFRAME_READY` from a sibling `useEffect`) is never dropped.
window.addEventListener('message', async (event) => {
    if (event.data.type === 'LOAD_CUSTOM_COMPONENTS') {
        componentsLoading = true;
        await loadCustomComponents(event.data.componentsCode);
        componentsLoading = false;
    } else if (event.data.type === 'UPDATE_AST') {
        if (componentsLoading) {
            await new Promise((resolve) => {
                const check = setInterval(() => {
                    if (!componentsLoading) {
                        clearInterval(check);
                        resolve(undefined);
                    }
                }, 50);
            });
        }
        updateAst(event.data.payload);
    } else if (event.data.type === 'UPDATE_THEME') {
        lastThemeCssUrl = event.data.cssUrl;
        reconcileThemeLink();
    }
});

/**
 * Imperatively manage a single `<link data-q2-theme>` in
 * `document.head`. The `data-q2-theme` attribute is the idempotency
 * selector — repeated `applyTheme` calls with the same URL just
 * `setAttribute('href', sameUrl)` in place, no element duplication.
 *
 * `cssUrl === null` removes the element (explicit clear). The pre-
 * first-message state has no `<link data-q2-theme>` element at all
 * and is distinct from a received `cssUrl: null` (which removes any
 * prior element).
 */
function applyTheme(cssUrl: string | null): void {
    let link = document.head.querySelector<HTMLLinkElement>(
        'link[data-q2-theme]',
    );
    if (cssUrl === null) {
        if (link) link.remove();
        return;
    }
    if (!link) {
        link = document.createElement('link');
        link.setAttribute('rel', 'stylesheet');
        link.setAttribute('data-q2-theme', '1');
        document.head.appendChild(link);
    }
    link.setAttribute('href', cssUrl);
}

// bd-y259zb57: the `UPDATE_THEME` channel carries the active document's
// *compiled theme*. For an HTML page that's Bootstrap; for a `format: revealjs`
// deck it's the compiled Quarto reveal theme, delivered through the SAME
// `css:theme:<fp>` → styles.css transport. Both must be applied as the
// `<link data-q2-theme>` so preview matches render.
//
// (Previously this suppressed the theme link on slides, because the preview
// only ever produced Bootstrap CSS — never the reveal theme — and reveal decks
// fell back to a hard-coded stock `white.css` import in `RevealDeck`. That was
// the centered/uppercase render↔preview divergence this strand fixes.)
let lastThemeCssUrl: string | null = null;

function reconcileThemeLink(): void {
    applyTheme(lastThemeCssUrl);
}

/**
 * Phase F.1 (bd-kw93.14): scroll the iframe document to the element
 * with the given id.
 */
function scrollToAnchorInDocument(anchor: string): boolean {
    const el = document.getElementById(anchor);
    if (!el) return false;
    el.scrollIntoView({ behavior: 'instant', block: 'start' });
    return true;
}

async function loadCustomComponents(componentsCode: Record<string, string>) {
    // Tied to dynamic user-TSX imports — materialize React and katex
    // when LOAD_CUSTOM_COMPONENTS arrives. q2-preview does NOT set
    // `window.RevealReact` (q2-debug-only — slide-demo template).
    (window as any).React = React;
    (window as any).katex = katex;

    const loadedModules: ComponentExports[] = [];
    for (const [componentName, code] of Object.entries(componentsCode)) {
        try {
            const blob = new Blob([code], { type: 'application/javascript' });
            const url = URL.createObjectURL(blob);
            try {
                const module = await import(/* @vite-ignore */ url);
                loadedModules.push(module as ComponentExports);
                console.log(
                    `[Q2PreviewIframe] Loaded custom component: ${componentName}`,
                );
            } finally {
                URL.revokeObjectURL(url);
            }
        } catch (err) {
            console.error(
                `[Q2PreviewIframe] Failed to load custom component ${componentName}:`,
                err,
            );
        }
    }

    customRegistry = buildCustomRegistry(loadedModules);
}

function updateAst(payload: UpdateAstPayload) {
    const {
        astJson,
        currentFilePath,
        assetManifest,
        projectFilePaths,
        pendingAnchor,
        pendingAnchorEpoch,
        renderedContent,
        untransformedAstJson,
        currentActor,
        editingDisabled,
        unlockNestingCursor,
        nestedEditBuffers,
    } = payload;
    const rootElement = document.getElementById('root');
    if (!rootElement) {
        console.error('Root element not found');
        return;
    }

    try {
        if (!root) {
            root = createRoot(rootElement);
        }
        root.render(
            <PreviewRoot
                astJson={astJson}
                currentFilePath={currentFilePath}
                assetManifest={assetManifest ?? {}}
                projectFilePaths={projectFilePaths}
                pendingAnchor={pendingAnchor}
                pendingAnchorEpoch={pendingAnchorEpoch}
                renderedContent={renderedContent}
                untransformedAstJson={untransformedAstJson}
                currentActor={currentActor ?? null}
                editingDisabled={editingDisabled}
                unlockNestingCursor={unlockNestingCursor}
                nestedEditBuffers={nestedEditBuffers}
                customRegistry={customRegistry}
                scrollToAnchor={scrollToAnchorInDocument}
                onNavigateToDocument={(path, anchor) => {
                    window.parent.postMessage(
                        { type: 'NAVIGATE_TO_DOCUMENT', path, anchor },
                        '*',
                    );
                }}
                setAst={(newAst) => {
                    // PreviewNodeEditPayload: pass through directly without
                    // rewrapping custom nodes (it's not a PandocAST).
                    if ((newAst as unknown as { __isPreviewNodeEdit?: boolean }).__isPreviewNodeEdit) {
                        window.parent.postMessage({ type: 'SET_AST', ast: newAst }, '*');
                        return;
                    }
                    // Rewrap JS-native CustomNodes back to wire-format
                    // Div/Span before posting. Keeps the parent-side
                    // (and any downstream consumer reading `data-custom-*`
                    // attributes) on the wire shape it expects.
                    window.parent.postMessage(
                        { type: 'SET_AST', ast: rewrapCustomNodes(newAst) },
                        '*',
                    );
                }}
            />,
        );
    } catch (err) {
        console.error('Failed to render AST:', err);
        rootElement.innerHTML = `
      <div style="padding: 20px; color: red;">
        <strong>Render Error:</strong>
        <pre>${err instanceof Error ? err.message : String(err)}</pre>
      </div>
    `;
    }
}

// Signal that the iframe is ready to receive messages. Posted AFTER
// the module-top message listener is registered so no UPDATE_THEME
// or UPDATE_AST is dropped.
window.parent.postMessage({ type: 'IFRAME_READY' }, '*');
