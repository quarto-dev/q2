/**
 * Iframe-side re-render-after-component-load test (GH #402 /
 * bd-ue80chl0 Phase 2).
 *
 * `LOAD_CUSTOM_COMPONENTS` rebuilds the iframe's `customRegistry`, but
 * historically the new registry only took effect on the NEXT
 * `UPDATE_AST` — a component (re)load with no AST change repainted
 * nothing. The fix caches the last `UPDATE_AST` payload at module top
 * and re-runs `updateAst` after a load completes, so a `.tsx` edit
 * repaints the live document (SPA and hub-client alike).
 *
 * Same harness as `entry.integration.test.tsx`: side-effect import of
 * `./entry` registers the module-top message listener; `react-dom/client`
 * is mocked so `root.render` calls are countable without mounting the
 * framework.
 */

import { describe, test, expect, beforeAll, vi } from 'vitest';

const { renderSpy } = vi.hoisted(() => ({ renderSpy: vi.fn() }));

vi.mock('react-dom/client', () => ({
    createRoot: vi.fn(() => ({ render: renderSpy })),
}));
vi.mock('katex/dist/katex.min.css', () => ({}));

beforeAll(async () => {
    document.body.innerHTML = '<div id="root"></div>';
    await import('./entry');
});

const EMPTY_AST_JSON = JSON.stringify({
    'pandoc-api-version': [1, 23, 0],
    meta: {},
    blocks: [],
});

function dispatchLoadComponents(componentsCode: Record<string, string>) {
    window.dispatchEvent(
        new MessageEvent('message', {
            data: { type: 'LOAD_CUSTOM_COMPONENTS', componentsCode },
        }),
    );
}

function dispatchUpdateAst() {
    window.dispatchEvent(
        new MessageEvent('message', {
            data: {
                type: 'UPDATE_AST',
                payload: {
                    astJson: EMPTY_AST_JSON,
                    currentFilePath: 'index.qmd',
                },
            },
        }),
    );
}

/** Drain the async message handler (LOAD awaits loadCustomComponents). */
async function flush() {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 0));
}

describe('q2-preview/entry LOAD_CUSTOM_COMPONENTS re-render', () => {
    test('boot-order LOAD before any UPDATE_AST does not render', async () => {
        dispatchLoadComponents({});
        await flush();
        expect(renderSpy).not.toHaveBeenCalled();
    });

    test('LOAD after UPDATE_AST re-renders the cached payload', async () => {
        dispatchUpdateAst();
        await flush();
        expect(renderSpy).toHaveBeenCalledTimes(1);

        // A component (re)load with no AST change must repaint so the
        // rebuilt registry takes effect. Empty componentsCode keeps the
        // blob-import machinery out of jsdom; the re-render contract is
        // the same regardless of module count.
        dispatchLoadComponents({});
        await flush();
        expect(renderSpy).toHaveBeenCalledTimes(2);
    });
});
