/**
 * Tests for useDocumentTitle.
 *
 * Regression coverage for https://github.com/quarto-dev/q2/issues/721:
 * the browser titlebar kept showing the last-opened project/file after
 * exiting to the project selector, because nothing restored the
 * default title when the Editor unmounted.
 *
 * @vitest-environment jsdom
 */

import { describe, expect, it } from 'vitest';
import { renderHook } from '@testing-library/react';

import { DEFAULT_DOCUMENT_TITLE, useDocumentTitle } from './useDocumentTitle';

describe('useDocumentTitle', () => {
  it('sets document.title while mounted', () => {
    renderHook(() => useDocumentTitle('index.qmd — my project — Quarto Hub'));
    expect(document.title).toBe('index.qmd — my project — Quarto Hub');
  });

  it('updates document.title when the title changes', () => {
    const { rerender } = renderHook(({ title }) => useDocumentTitle(title), {
      initialProps: { title: 'a.qmd — proj — Quarto Hub' },
    });
    expect(document.title).toBe('a.qmd — proj — Quarto Hub');

    rerender({ title: 'b.qmd — proj — Quarto Hub' });
    expect(document.title).toBe('b.qmd — proj — Quarto Hub');
  });

  it('restores the default title on unmount', () => {
    const { unmount } = renderHook(() =>
      useDocumentTitle('index.qmd — my project — Quarto Hub'),
    );
    expect(document.title).not.toBe(DEFAULT_DOCUMENT_TITLE);

    unmount();
    expect(document.title).toBe(DEFAULT_DOCUMENT_TITLE);
  });
});
