/**
 * ViewToggleControl: the three split presets in the document header.
 * The expand presets must send fractions a divider drag (clamped to
 * 15%–85%) can't produce, so Editor can tell "hide the other pane" apart
 * from a drag to the limit (GH #1037).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import ViewToggleControl, { SPLIT_PRESETS } from './ViewToggleControl';
import { viewToggle } from '../strings';

afterEach(cleanup);

describe('ViewToggleControl', () => {
  it('sends 1 for "Expand markup", 0.5 for "Split equally", 0 for "Expand preview"', () => {
    const onSelect = vi.fn();
    render(<ViewToggleControl fraction={0.5} onSelect={onSelect} />);

    fireEvent.click(screen.getByRole('button', { name: viewToggle.markupView }));
    fireEvent.click(screen.getByRole('button', { name: viewToggle.splitView }));
    fireEvent.click(screen.getByRole('button', { name: viewToggle.previewView }));

    expect(onSelect.mock.calls).toEqual([[1], [0.5], [0]]);
    expect(SPLIT_PRESETS).toEqual({ markup: 1, split: 0.5, preview: 0 });
  });

  it.each([
    [1, viewToggle.markupView],
    [0.5, viewToggle.splitView],
    [0, viewToggle.previewView],
  ])('marks the preset for fraction %s as active', (fraction, activeLabel) => {
    render(<ViewToggleControl fraction={fraction} onSelect={vi.fn()} />);
    for (const label of [viewToggle.markupView, viewToggle.splitView, viewToggle.previewView]) {
      expect(screen.getByRole('button', { name: label }).getAttribute('aria-pressed')).toBe(
        String(label === activeLabel),
      );
    }
  });

  it('marks no preset active at a drag limit', () => {
    render(<ViewToggleControl fraction={0.85} onSelect={vi.fn()} />);
    for (const button of screen.getAllByRole('button')) {
      expect(button.getAttribute('aria-pressed')).toBe('false');
    }
  });
});
