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
 */

import { createRoot } from 'react-dom/client';
import React, { useEffect } from 'react';
import katex from 'katex';
import 'katex/dist/katex.min.css';

import { Ast, Node, renderChildren, renderNode } from '../framework';
import type { FormatRegistry, PandocAST } from '../framework';
import { Block, Inline, previewRegistry, PreviewContext } from '.';
import {
    buildCustomRegistry,
    type ComponentExports,
} from '../../../utils/customRegistry';
import { installLinkHandlers } from '../../../utils/iframeLinkHandlers';

// Set the renderer-surface global at module top. Importing this module
// is sufficient to populate `window.__Q2_PREVIEW_RENDERER__`. The
// explicit-object form (rather than `{ ...framework, ...preview }`
// spread) keeps framework internals (`renderChildrenRegistry`,
// `RegistryContext`) off the global and locks the public surface.
//
// Plan 2B will extend this object with q2-preview's leaf components as
// they ship.
(window as any).__Q2_PREVIEW_RENDERER__ = {
    renderChildren,
    renderNode,
    Node,
    Block,
    Inline,
    previewRegistry,
};

let root: ReturnType<typeof createRoot> | null = null;
let customRegistry: Record<string, React.ComponentType<any>> = {};
let componentsLoading = false;

interface UpdateAstPayload {
    astJson: string;
    currentFilePath: string;
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
        applyTheme(event.data.cssUrl);
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

interface PreviewRootProps {
    astJson: string;
    currentFilePath: string;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}

function PreviewRoot(props: PreviewRootProps) {
    // Install link handlers once per mount. The iframe remounts on
    // every document switch (q2-debug's existing behavior — see
    // `ReactPreview.tsx` previewState reset), so closures captured
    // here cannot go stale within a single mount.
    useEffect(() => {
        installLinkHandlers(document, {
            currentFilePath: props.currentFilePath,
            onQmdLinkClick: (arg) => {
                if ('path' in arg) {
                    props.onNavigateToDocument?.(arg.path, arg.anchor);
                } else {
                    props.onNavigateToDocument?.(props.currentFilePath, arg.anchor);
                }
            },
        });
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    const mergedRegistry: FormatRegistry = {
        ...previewRegistry,
        ...customRegistry,
    } as FormatRegistry;

    return (
        <PreviewContext.Provider
            value={{ currentFilePath: props.currentFilePath }}
        >
            <Ast
                astJson={props.astJson}
                currentFilePath={props.currentFilePath}
                onNavigateToDocument={props.onNavigateToDocument}
                setAst={props.setAst}
                registry={mergedRegistry}
            />
        </PreviewContext.Provider>
    );
}

function updateAst(payload: UpdateAstPayload) {
    const { astJson, currentFilePath } = payload;
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
                onNavigateToDocument={(path, anchor) => {
                    window.parent.postMessage(
                        { type: 'NAVIGATE_TO_DOCUMENT', path, anchor },
                        '*',
                    );
                }}
                setAst={(newAst) => {
                    window.parent.postMessage(
                        { type: 'SET_AST', ast: newAst },
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
