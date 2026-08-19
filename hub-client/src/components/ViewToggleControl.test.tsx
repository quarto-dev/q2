/**
 * Accessibility tests for ViewToggleControl.
 *
 * - 1.1.1: decorative SVG icons are aria-hidden so only the button
 *   names are announced
 * - 4.1.2: the active toggle exposes its state via aria-pressed
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import ViewToggleControl from './ViewToggleControl';
import { ViewModeProvider } from './ViewModeContext';

afterEach(cleanup);

function renderControl() {
  return render(
    <ViewModeProvider>
      <ViewToggleControl />
    </ViewModeProvider>,
  );
}

describe('ViewToggleControl accessibility', () => {
  it('marks decorative icons aria-hidden', () => {
    const { container } = renderControl();
    const svgs = container.querySelectorAll('svg');
    expect(svgs.length).toBe(3);
    svgs.forEach((svg) => expect(svg.getAttribute('aria-hidden')).toBe('true'));
  });

  it('exposes pressed state on the active toggle', () => {
    renderControl();
    const markup = screen.getByRole('button', { name: 'Markup view' });
    const split = screen.getByRole('button', { name: 'Split view' });
    const preview = screen.getByRole('button', { name: 'Preview view' });

    // Default view mode shows both panes.
    expect(split.getAttribute('aria-pressed')).toBe('true');
    expect(markup.getAttribute('aria-pressed')).toBe('false');
    expect(preview.getAttribute('aria-pressed')).toBe('false');
  });
});
