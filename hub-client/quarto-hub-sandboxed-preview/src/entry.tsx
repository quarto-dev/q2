/**
 * Entry point for the q2-sandboxed-preview renderer iframe.
 *
 * Adapted copy of `@quarto/preview-renderer/q2-preview/entry` (which is
 * NOT modified by the sandboxed-preview port — see
 * claude-notes/plans/2026-09-01-port-q2-preview-into-sandboxed-preview.md).
 * The renderer itself (PreviewRoot, registry, framework) is imported
 * unmodified from the package source; only the boundary differs. The
 * differences from the q2-preview entry, all consequences of running
 * cross-origin (GitHub Pages) instead of same-origin:
 *
 *  1. `UPDATE_THEME` carries the compiled CSS **text**, not a
 *     parent-minted blob URL — blob URLs are origin-scoped and
 *     unreachable from this frame. The entry mints its own blob URL
 *     (same-origin to itself) and swaps it into the
 *     `<link data-q2-theme>` element.
 *
 *  2. A service worker proxies document asset fetches (images, fonts
 *     referenced by theme CSS) back to the parent over postMessage —
 *     registered in `init()` before IFRAME_READY is posted, so the
 *     first painted document already has interception in place.
 *
 *  3. Message handling goes through the promise-ordered
 *     `makeIframeMessageDispatcher` (the q2-debug dispatcher) instead
 *     of the original entry's 50ms-polling LOAD/UPDATE gate.
 */

import { createRoot } from 'react-dom/client';
import React from 'react';
import katex from 'katex';
import 'katex/dist/katex.min.css';
// Bootstrap 5 bundled JS, headroom, and quarto-nav, vendored at the
// repo's `resources/js/`. Embedded as raw text and injected as inline
// `<script>` at module top so Bootstrap's data-API click delegates are
// attached before any chrome HTML arrives — same injection (and
// ordering: bootstrap, headroom, quarto-nav) as the q2-preview entry.
import bootstrapJsSrc from '../../../resources/js/bootstrap/bootstrap.bundle.min.js?raw';
import headroomJsSrc from '../../../resources/js/headroom/headroom.min.js?raw';
import quartoNavJsSrc from '../../../resources/js/quarto-nav/quarto-nav.js?raw';

(() => {
    const inject = (marker: string, src: string) => {
        if (document.head.querySelector(`script[${marker}]`)) return;
        const tag = document.createElement('script');
        tag.setAttribute(marker, '1');
        tag.textContent = src;
        document.head.appendChild(tag);
    };
    inject('data-q2-bootstrap', bootstrapJsSrc);
    inject('data-q2-headroom', headroomJsSrc);
    inject('data-q2-quarto-nav', quartoNavJsSrc);
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
} from '@quarto/preview-renderer/framework';
import {
    Block,
    Inline,
    previewRegistry,
    usePreviewEdit,
} from '@quarto/preview-renderer/q2-preview';
import { renderSlot } from '@quarto/preview-renderer/q2-preview/utils';
import { PreviewTitleBlock } from '@quarto/preview-renderer/q2-preview/custom/PreviewTitleBlock';
import {
    buildCustomRegistry,
    type ComponentExports,
} from '@quarto/preview-renderer/utils/customRegistry';
import { PreviewRoot } from '@quarto/preview-renderer/q2-preview/PreviewRoot';
import { makeIframeMessageDispatcher } from './iframeMessageDispatch';
import { init as initServiceWorker } from './registerServiceWorker';

// Renderer-surface global for user TSX overrides, identical to the
// q2-preview entry's (the surface is part of the render-components
// contract, so the sandboxed preview exposes the same names).
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
// Last UPDATE_AST payload, cached so a LOAD_CUSTOM_COMPONENTS that
// arrives with no accompanying AST change (a `.tsx` edit re-transpiled
// by the parent) can repaint the current document with the rebuilt
// registry. Null until the first UPDATE_AST — the boot-order LOAD
// (posted before the first AST) must not render.
let lastAstPayload: UpdateAstPayload | null = null;

// Slide-navigation bridge. `RevealDeck`'s `RevealNavSync` registers an
// imperative `goTo` here (and clears it on unmount); the parent's
// `SET_SLIDE` message drives it.
let revealSlideNavigator: ((index: number) => void) | null = null;
const registerSlideNavigator = (nav: ((index: number) => void) | null) => {
    revealSlideNavigator = nav;
};
const postSlideChanged = (index: number) => {
    window.parent.postMessage({ type: 'SLIDE_CHANGED', index }, '*');
};

/** Same payload shape as the q2-preview entry's UpdateAstPayload. */
interface UpdateAstPayload {
    astJson: string;
    currentFilePath: string;
    untransformedAstJson?: string | null;
    /**
     * In the sandboxed protocol the manifest maps `origPath → origPath`
     * (identity): the `<img>` fetch goes to the network on this frame's
     * own origin, where the service worker intercepts it and proxies the
     * bytes from the parent's VFS. (In q2-preview it maps to parent-minted
     * blob URLs, which this frame could not fetch.)
     */
    assetManifest?: Record<string, string>;
    projectFilePaths?: readonly string[];
    pendingAnchor?: string | null;
    pendingAnchorEpoch?: number;
    renderedContent?: string;
    currentActor?: string | null;
    editingDisabled?: boolean;
    commentsMode?: 'expand' | 'show' | 'hide';
    unlockNestingCursor?: boolean;
    richText?: boolean;
    nestedEditBuffers?: Record<string, string>;
}

const dispatch = makeIframeMessageDispatcher({
    loadCustomComponents: async (componentsCode) => {
        await loadCustomComponents(componentsCode);
        // Repaint the current document so the rebuilt registry takes
        // effect immediately. Without this, a component (re)load only
        // showed up on the next UPDATE_AST.
        if (lastAstPayload) {
            updateAst(lastAstPayload);
        }
    },
    updateAst: (payload) => {
        lastAstPayload = payload as UpdateAstPayload;
        updateAst(lastAstPayload);
    },
    applyTheme: (cssText) => {
        applyThemeText(cssText);
    },
});

// Module-top message handler. Registered before `IFRAME_READY` is
// posted so the parent's `UPDATE_THEME` (which can fire immediately
// after `IFRAME_READY` from a sibling `useEffect`) is never dropped.
window.addEventListener('message', (event) => {
    const data = event.data;
    if (!data || typeof data.type !== 'string') return;
    if (data.type === 'SET_SLIDE') {
        // Drive the reveal deck imperatively (no AST re-render). No-op when
        // the current preview isn't a slide deck (no navigator registered).
        revealSlideNavigator?.(data.index);
        return;
    }
    if (
        data.type === 'LOAD_CUSTOM_COMPONENTS' ||
        data.type === 'UPDATE_AST' ||
        data.type === 'UPDATE_THEME'
    ) {
        void dispatch(data);
    }
});

/**
 * Imperatively manage a single `<link data-q2-theme>` in
 * `document.head`, minting a local blob URL from the posted CSS text.
 * `cssText === null` removes the element (explicit clear). The blob URL
 * is created on THIS frame's origin, so relative `url()` references in
 * the CSS resolve against this origin — where the service worker can
 * intercept them.
 */
let currentThemeBlobUrl: string | null = null;

function applyThemeText(cssText: string | null): void {
    let link = document.head.querySelector<HTMLLinkElement>(
        'link[data-q2-theme]',
    );
    if (currentThemeBlobUrl) {
        // The browser retains the old bytes while the <link> swap is in
        // flight, so revoking the prior URL here is safe.
        URL.revokeObjectURL(currentThemeBlobUrl);
        currentThemeBlobUrl = null;
    }
    if (cssText === null) {
        if (link) link.remove();
        return;
    }
    const blob = new Blob([cssText], { type: 'text/css' });
    currentThemeBlobUrl = URL.createObjectURL(blob);
    if (!link) {
        link = document.createElement('link');
        link.setAttribute('rel', 'stylesheet');
        link.setAttribute('data-q2-theme', '1');
        document.head.appendChild(link);
    }
    link.setAttribute('href', currentThemeBlobUrl);
}

/** Scroll the iframe document to the element with the given id. */
function scrollToAnchorInDocument(anchor: string): boolean {
    const el = document.getElementById(anchor);
    if (!el) return false;
    el.scrollIntoView({ behavior: 'instant', block: 'start' });
    return true;
}

async function loadCustomComponents(componentsCode: Record<string, string>) {
    // Tied to dynamic user-TSX imports — materialize React and katex
    // when LOAD_CUSTOM_COMPONENTS arrives.
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
                    `[Q2SandboxedPreview] Loaded custom component: ${componentName}`,
                );
            } finally {
                URL.revokeObjectURL(url);
            }
        } catch (err) {
            console.error(
                `[Q2SandboxedPreview] Failed to load custom component ${componentName}:`,
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
        commentsMode,
        unlockNestingCursor,
        richText,
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
                commentsMode={commentsMode}
                unlockNestingCursor={unlockNestingCursor}
                richText={richText}
                nestedEditBuffers={nestedEditBuffers}
                customRegistry={customRegistry}
                scrollToAnchor={scrollToAnchorInDocument}
                registerSlideNavigator={registerSlideNavigator}
                onSlideChange={postSlideChanged}
                onAstRendered={() => {
                    window.parent.postMessage({ type: 'AST_RENDERED' }, '*');
                }}
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
                    // Div/Span before posting.
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

// Register the asset-proxy service worker, then signal readiness. The
// message listener above is already registered, so nothing the parent
// posts after IFRAME_READY can be dropped; gating IFRAME_READY on the
// service worker means the first rendered document already has asset
// interception in place. `init()` resolves (never rejects) even when
// registration fails — e.g. the same-origin dev fallback without a
// served serviceWorker.js — so the preview still boots, just without
// proxied assets.
initServiceWorker()
    .catch((err) => {
        console.error('[Q2SandboxedPreview] service worker init failed:', err);
    })
    .finally(() => {
        window.parent.postMessage({ type: 'IFRAME_READY' }, '*');
    });
