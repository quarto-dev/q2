/**
 * Tests for RunControl (bd-sfet3264, Phase 4b).
 *
 * The preview "Run" affordance: triggers an ephemeral exec request via `onRun`,
 * reflects the durable CaptureRef status (running/error/staleness), and clears
 * its local pending flag when a new capture arrives.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import type { CaptureRef } from '@quarto/preview-runtime';
import { RunControl } from './RunControl';

describe('RunControl (Phase 4b run affordance)', () => {
  it('renders nothing when there is no active document', () => {
    const { container } = render(<RunControl path={null} onRun={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('shows "Run" and calls onRun(path) when no capture exists', () => {
    const onRun = vi.fn();
    render(<RunControl path="doc.qmd" onRun={onRun} />);
    const btn = screen.getByRole('button', { name: /run code cells/i });
    expect(btn).toHaveTextContent('Run');
    fireEvent.click(btn);
    expect(onRun).toHaveBeenCalledWith('doc.qmd');
  });

  it('shows "Re-run" when an idle capture already exists', () => {
    const capture: CaptureRef = { captureDocId: 'cap-1', state: 'idle' };
    render(<RunControl path="doc.qmd" capture={capture} onRun={vi.fn()} />);
    expect(screen.getByRole('button')).toHaveTextContent('Re-run');
  });

  it('reflects a running capture: disabled "Executing…"', () => {
    const capture: CaptureRef = { captureDocId: 'cap-1', state: 'running' };
    render(<RunControl path="doc.qmd" capture={capture} onRun={vi.fn()} />);
    const btn = screen.getByRole('button');
    expect(btn).toHaveTextContent('Executing…');
    expect(btn).toBeDisabled();
  });

  it('surfaces the last error and re-enables the button', () => {
    const capture: CaptureRef = {
      captureDocId: 'cap-1',
      state: 'error',
      lastError: 'engine boom',
    };
    render(<RunControl path="doc.qmd" capture={capture} onRun={vi.fn()} />);
    expect(screen.getByRole('alert')).toHaveTextContent('engine boom');
    expect(screen.getByRole('button')).not.toBeDisabled();
  });

  it('shows a staleness note when the capture is stale', () => {
    const capture: CaptureRef = {
      captureDocId: 'cap-1',
      state: 'idle',
      staleness: true,
    };
    render(<RunControl path="doc.qmd" capture={capture} onRun={vi.fn()} />);
    expect(screen.getByText(/code changed since the last run/i)).toBeInTheDocument();
  });

  it('goes busy on click, then re-enables when a new capture arrives', () => {
    const onRun = vi.fn();
    const first: CaptureRef = { captureDocId: 'cap-1', state: 'idle' };
    const { rerender } = render(<RunControl path="doc.qmd" capture={first} onRun={onRun} />);

    fireEvent.click(screen.getByRole('button'));
    // Optimistic local pending → disabled "Executing…" before the sidecar moves.
    expect(screen.getByRole('button')).toHaveTextContent('Executing…');
    expect(screen.getByRole('button')).toBeDisabled();

    // A fresh capture (new doc id) arrives via sync → pending clears.
    const next: CaptureRef = { captureDocId: 'cap-2', state: 'idle' };
    rerender(<RunControl path="doc.qmd" capture={next} onRun={onRun} />);
    expect(screen.getByRole('button')).toHaveTextContent('Re-run');
    expect(screen.getByRole('button')).not.toBeDisabled();
  });
});
