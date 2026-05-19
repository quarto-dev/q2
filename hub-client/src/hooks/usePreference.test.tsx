/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import { usePreference } from './usePreference';

afterEach(() => {
  cleanup();
  localStorage.clear();
});

// `errorOverlayCollapsed` is the chosen probe key — a persisted
// boolean preference whose default is `true`. The test exercises
// generic cross-instance reactivity, not the semantics of any
// particular key, so any boolean preference works.
function Writer() {
  const [value, setValue] = usePreference('errorOverlayCollapsed');
  return (
    <button data-testid="writer" onClick={() => setValue(!value)}>
      writer: {String(value)}
    </button>
  );
}

function Reader() {
  const [value] = usePreference('errorOverlayCollapsed');
  return <span data-testid="reader">reader: {String(value)}</span>;
}

describe('usePreference cross-instance reactivity', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('a setValue in one component is observed by a sibling instance', () => {
    render(
      <>
        <Writer />
        <Reader />
      </>,
    );

    // Both start at default (true for errorOverlayCollapsed).
    expect(screen.getByTestId('writer').textContent).toBe('writer: true');
    expect(screen.getByTestId('reader').textContent).toBe('reader: true');

    // Toggle in the writer — the reader observes the change without
    // remounting.
    fireEvent.click(screen.getByTestId('writer'));

    expect(screen.getByTestId('writer').textContent).toBe('writer: false');
    expect(screen.getByTestId('reader').textContent).toBe('reader: false');
  });
});
