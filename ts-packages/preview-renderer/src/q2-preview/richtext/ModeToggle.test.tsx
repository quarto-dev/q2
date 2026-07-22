/**
 * bd-igpm0xur Phase 0 — ModeToggle unit tests.
 *
 * The single-icon rich/plain toggle that replaces the two-button text toggle in
 * the deleted left-margin EditAffordance. It renders one Markdown-mark button on the
 * pop-up toolbar, reads `ctx.editorMode`, and on click swaps the session mode
 * via `setEditorMode` — guarded by `editorModeSwitchRef` so the mode swap does
 * NOT blur → commit/close the edit session (ported verbatim from
 * EditAffordance.choose).
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/react';
import { PreviewContext } from '../PreviewContext';
import type { PreviewContextValue } from '../PreviewContext';
import { ModeToggle } from './ModeToggle';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

function renderToggle(overrides: Partial<PreviewContextValue> = {}) {
    const setEditorMode = vi.fn();
    const editorModeSwitchRef = { current: false };
    const ctx = {
        currentFilePath: '/t.qmd',
        editorMode: 'rich',
        setEditorMode,
        editorModeSwitchRef,
        ...overrides,
    } as unknown as PreviewContextValue;
    const utils = render(
        <PreviewContext.Provider value={ctx}>
            <ModeToggle />
        </PreviewContext.Provider>,
    );
    return { ...utils, setEditorMode, editorModeSwitchRef };
}

const toggleBtn = (c: HTMLElement) => c.querySelector<HTMLButtonElement>('.q2-rt-tb-mode');

describe('ModeToggle', () => {
    it('renders exactly one Markdown-mark button with a stable, non-empty aria-label', () => {
        const { container } = renderToggle({ editorMode: 'rich' });
        const btns = container.querySelectorAll('button.q2-rt-tb-mode');
        expect(btns).toHaveLength(1);
        const btn = btns[0] as HTMLButtonElement;
        // The icon is the inline Markdown-mark SVG (no text).
        expect(btn.querySelector('svg'), 'renders the Markdown-mark SVG icon').not.toBeNull();
        const label = btn.getAttribute('aria-label');
        expect(label, 'aria-label must be present and non-empty').toBeTruthy();
    });

    it('keeps the SAME aria-label across modes (stable name, state via aria-pressed)', () => {
        const rich = renderToggle({ editorMode: 'rich' });
        const richLabel = toggleBtn(rich.container)!.getAttribute('aria-label');
        cleanup();
        const plain = renderToggle({ editorMode: 'plain' });
        const plainLabel = toggleBtn(plain.container)!.getAttribute('aria-label');
        expect(plainLabel).toBe(richLabel);
    });

    it('aria-pressed reflects editorMode === plain', () => {
        const rich = renderToggle({ editorMode: 'rich' });
        expect(toggleBtn(rich.container)!.getAttribute('aria-pressed')).toBe('false');
        cleanup();
        const plain = renderToggle({ editorMode: 'plain' });
        expect(toggleBtn(plain.container)!.getAttribute('aria-pressed')).toBe('true');
    });

    it('click swaps to the opposite mode and flips the switch-ref true→false', () => {
        vi.useFakeTimers();
        const { container, setEditorMode, editorModeSwitchRef } = renderToggle({ editorMode: 'rich' });
        fireEvent.mouseDown(toggleBtn(container)!);
        // Swaps to the opposite mode.
        expect(setEditorMode).toHaveBeenCalledWith('plain');
        // Guard is set true synchronously so the outgoing surface's blur is a no-op…
        expect(editorModeSwitchRef.current).toBe(true);
        // …and cleared on the next tick.
        vi.runAllTimers();
        expect(editorModeSwitchRef.current).toBe(false);
    });

    it('click from plain swaps back to rich', () => {
        const { container, setEditorMode } = renderToggle({ editorMode: 'plain' });
        fireEvent.mouseDown(toggleBtn(container)!);
        expect(setEditorMode).toHaveBeenCalledWith('rich');
    });
});
