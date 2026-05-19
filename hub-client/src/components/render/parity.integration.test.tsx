/**
 * Framework-primitive parity test (Plan 2A item 14).
 *
 * Locks the contract from Plan 2A §"TSX globals: portability and
 * parity": both iframe globals expose the framework primitives
 * (`renderChildren`, `renderNode`, `Node`) byte-identically; format-
 * specific helpers diverge. Any framework primitive added in the
 * future MUST land on both globals in lockstep — reviewers should
 * treat asymmetric additions as a bug.
 *
 * Assertion: each framework primitive is reference-equal across:
 *   - the framework module's exports
 *   - `__REACT_AST_DEBUG_RENDERER__`
 *   - `__Q2_PREVIEW_RENDERER__`
 *
 * `Block` and `Inline` are deliberately NOT part of the assertion —
 * those are format-specific (different miss-path styling) and
 * intentionally diverge despite sharing names.
 *
 * This requires both entries to set their renderer-surface globals at
 * module top (not lazily inside `loadCustomComponents`). q2-debug's
 * entry was refactored to that effect in Plan 2A item 9; q2-preview's
 * entry does it from the start.
 */

import { describe, test, expect, beforeAll, vi } from 'vitest';

// q2-debug's entry imports `@revealjs/react` and `reveal.js/reveal.css`.
// In the test environment those would pull a heavy dependency graph;
// stub them so importing the entry is fast and side-effect-free for
// the renderer-surface global.
vi.mock('@revealjs/react', () => ({
    Deck: () => null,
    Slide: () => null,
}));
vi.mock('reveal.js/reveal.css', () => ({}));
vi.mock('reveal.js/theme/white.css', () => ({}));
vi.mock('katex/dist/katex.min.css', () => ({}));
vi.mock('react-dom/client', () => ({
    createRoot: vi.fn(() => ({ render: vi.fn() })),
}));

beforeAll(async () => {
    // Side-effect imports populate the two iframe globals. The order
    // doesn't matter — both entries set their global at module top.
    await import('./q2-debug/entry');
    await import('./q2-preview/entry');
});

const FRAMEWORK_PRIMITIVES = ['renderChildren', 'renderNode', 'Node'] as const;

describe('framework-primitive parity across iframe globals', () => {
    test.each(FRAMEWORK_PRIMITIVES)(
        '%s is reference-equal across both globals and the framework module',
        async (name) => {
            const framework = await import('@quarto/preview-renderer/framework');
            const debug = (window as any).__REACT_AST_DEBUG_RENDERER__[name];
            const preview = (window as any).__Q2_PREVIEW_RENDERER__[name];
            expect(debug).toBe((framework as any)[name]);
            expect(preview).toBe((framework as any)[name]);
            expect(debug).toBe(preview);
        },
    );

    test('Block and Inline intentionally diverge between formats', () => {
        // Both globals expose `Block` and `Inline` under the same key,
        // but they're different functions — q2-debug's render bordered
        // "Not registered" miss path, q2-preview's render the muted-
        // gray "(not yet implemented)" placeholder + recurse into
        // children. This asserts the divergence is real, so a future
        // refactor that accidentally collapses them gets caught.
        const debugBlock = (window as any).__REACT_AST_DEBUG_RENDERER__.Block;
        const previewBlock = (window as any).__Q2_PREVIEW_RENDERER__.Block;
        expect(debugBlock).not.toBe(previewBlock);
        const debugInline = (window as any).__REACT_AST_DEBUG_RENDERER__.Inline;
        const previewInline = (window as any).__Q2_PREVIEW_RENDERER__.Inline;
        expect(debugInline).not.toBe(previewInline);
    });
});
