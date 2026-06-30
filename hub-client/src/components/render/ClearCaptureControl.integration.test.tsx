/**
 * Tests for ClearCaptureControl (bd-sfet3264, Phase 1F / D6).
 *
 * The per-document "clear results" affordance: visible only when the active
 * document has a recorded capture; a two-step inline confirmation (because
 * clearing affects every collaborator) before invoking the clear action.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ClearCaptureControl } from './ClearCaptureControl';

describe('ClearCaptureControl (D6 clear affordance)', () => {
  it('renders nothing when the active document has no capture', () => {
    const { container } = render(
      <ClearCaptureControl path="doc.qmd" hasCapture={false} onClear={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('renders nothing when there is no active document path', () => {
    const { container } = render(
      <ClearCaptureControl path={null} hasCapture={true} onClear={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('shows the clear affordance when the active document has a capture', () => {
    render(<ClearCaptureControl path="doc.qmd" hasCapture={true} onClear={vi.fn()} />);
    expect(screen.getByRole('button', { name: /clear results/i })).toBeInTheDocument();
  });

  it('requires confirmation before clearing, then calls onClear with the path', () => {
    const onClear = vi.fn();
    render(<ClearCaptureControl path="doc.qmd" hasCapture={true} onClear={onClear} />);

    // First click only arms the confirmation — must NOT clear yet.
    fireEvent.click(screen.getByRole('button', { name: /clear results/i }));
    expect(onClear).not.toHaveBeenCalled();

    // The confirmation must name the collaborator-wide effect.
    expect(screen.getByText(/collaborator/i)).toBeInTheDocument();

    // Confirming clears with the active path.
    fireEvent.click(screen.getByRole('button', { name: /^clear$/i }));
    expect(onClear).toHaveBeenCalledTimes(1);
    expect(onClear).toHaveBeenCalledWith('doc.qmd');
  });

  it('cancelling the confirmation does not clear', () => {
    const onClear = vi.fn();
    render(<ClearCaptureControl path="doc.qmd" hasCapture={true} onClear={onClear} />);

    fireEvent.click(screen.getByRole('button', { name: /clear results/i }));
    fireEvent.click(screen.getByRole('button', { name: /cancel/i }));
    expect(onClear).not.toHaveBeenCalled();
    // Back to the initial affordance.
    expect(screen.getByRole('button', { name: /clear results/i })).toBeInTheDocument();
  });
});
