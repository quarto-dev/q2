/**
 * Tests for PreviewStatusBar (bd-yai4w8ly).
 *
 * The single, merged preview status line that replaces the former three
 * pieces (RunControl, the inline `.executor-online-bar`, and
 * ClearCaptureControl). It selectively shows executor liveness, the
 * "showing executed output" message, and both the Clear and Run/Re-run
 * buttons — as a function of `executorsOnline`, `hasExecutableCells`, and
 * the active document's `CaptureRef`.
 *
 * The state -> rendering contract mirrors the table in
 * claude-notes/plans/2026-07-01-merge-preview-status-line.md.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import type { CaptureRef } from '@quarto/preview-runtime';
import { PreviewStatusBar, PENDING_TIMEOUT_MS } from './PreviewStatusBar';

const idle: CaptureRef = { captureDocId: 'cap-1', state: 'idle' };

/** The Run/Re-run button, matched by its stable aria-label. */
const runButton = () => screen.queryByRole('button', { name: /run code cells/i });
/** The initial "Clear results…" button (not the confirm-state "Clear"). */
const clearButton = () => screen.queryByRole('button', { name: /clear results/i });

describe('PreviewStatusBar (bd-yai4w8ly merged status line)', () => {
  // ---- visibility -------------------------------------------------------

  it('renders nothing when no executor is online and there is no capture', () => {
    const { container } = render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={false}
        hasExecutableCells={true}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  // ---- executor axis, no capture ---------------------------------------

  it('shows "Executor online" with a dot and no buttons when online but doc has no executable cells', () => {
    const { container } = render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={false}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    expect(screen.getByText(/executor online/i)).toBeInTheDocument();
    expect(container.querySelector('.executor-online-dot')).toBeInTheDocument();
    expect(runButton()).toBeNull();
    expect(clearButton()).toBeNull();
  });

  it('shows "Run" and calls onRun(path) when online with executable cells and no capture', () => {
    const onRun = vi.fn();
    render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        onRun={onRun}
        onClear={vi.fn()}
      />,
    );
    const btn = runButton()!;
    expect(btn).toHaveTextContent('Run');
    expect(clearButton()).toBeNull();
    fireEvent.click(btn);
    expect(onRun).toHaveBeenCalledWith('doc.qmd');
  });

  // ---- capture present, executor online --------------------------------

  it('shows "Showing executed output", Clear, and "Re-run" for an idle capture', () => {
    render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={idle}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    expect(screen.getByText(/showing executed output/i)).toBeInTheDocument();
    expect(clearButton()).toBeInTheDocument();
    expect(runButton()).toHaveTextContent('Re-run');
  });

  it('reflects a running capture: status "Executing…" and a disabled Run button; Clear stays available', () => {
    const capture: CaptureRef = { captureDocId: 'cap-1', state: 'running' };
    const { container } = render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={capture}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    // Both the status label and the Run button read "Executing…" while busy.
    expect(container.querySelector('.preview-status-label')).toHaveTextContent('Executing…');
    expect(runButton()).toBeDisabled();
    expect(runButton()).toHaveTextContent('Executing…');
    expect(clearButton()).toBeInTheDocument();
  });

  it('surfaces the last error (as an alert) and re-enables the Run button', () => {
    const capture: CaptureRef = {
      captureDocId: 'cap-1',
      state: 'error',
      lastError: 'engine boom',
    };
    render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={capture}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('engine boom');
    expect(runButton()).not.toBeDisabled();
  });

  it('shows BOTH facts for a stale capture: "Showing executed output" and "code changed"', () => {
    const capture: CaptureRef = { captureDocId: 'cap-1', state: 'idle', staleness: true };
    render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={capture}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    expect(screen.getByText(/showing executed output/i)).toBeInTheDocument();
    expect(screen.getByText(/code changed/i)).toBeInTheDocument();
    expect(runButton()).toHaveTextContent('Re-run');
  });

  // ---- capture present, executor OFFLINE (Clear is executor-independent) -

  it('still shows the capture status + Clear (but no Run) when no executor is online', () => {
    render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={false}
        hasExecutableCells={true}
        capture={idle}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    expect(screen.getByText(/showing executed output/i)).toBeInTheDocument();
    expect(clearButton()).toBeInTheDocument();
    expect(runButton()).toBeNull();
  });

  // ---- button order: Clear before Run so Run stays pinned far right -----

  it('renders Clear before Run in DOM order so Run is pinned to the far right', () => {
    render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={idle}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    const buttons = screen.getAllByRole('button');
    expect(buttons[0]).toHaveAccessibleName(/clear results/i);
    expect(buttons[buttons.length - 1]).toHaveAccessibleName(/run code cells/i);
  });

  // ---- run pending state machine (ported from RunControl) ---------------

  it('goes busy on Run click, then re-enables when a new capture arrives', () => {
    const onRun = vi.fn();
    const { rerender } = render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={idle}
        onRun={onRun}
        onClear={vi.fn()}
      />,
    );

    fireEvent.click(runButton()!);
    expect(runButton()).toHaveTextContent('Executing…');
    expect(runButton()).toBeDisabled();

    const next: CaptureRef = { captureDocId: 'cap-2', state: 'idle' };
    rerender(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={next}
        onRun={onRun}
        onClear={vi.fn()}
      />,
    );
    expect(runButton()).toHaveTextContent('Re-run');
    expect(runButton()).not.toBeDisabled();
  });

  it('clears a stuck pending flag after PENDING_TIMEOUT_MS (request found no executor)', () => {
    vi.useFakeTimers();
    try {
      render(
        <PreviewStatusBar
          path="doc.qmd"
          executorsOnline={true}
          hasExecutableCells={true}
          capture={idle}
          onRun={vi.fn()}
          onClear={vi.fn()}
        />,
      );
      fireEvent.click(runButton()!);
      expect(runButton()).toBeDisabled();
      act(() => {
        vi.advanceTimersByTime(PENDING_TIMEOUT_MS + 1);
      });
      expect(runButton()).not.toBeDisabled();
      expect(runButton()).toHaveTextContent('Re-run');
    } finally {
      vi.useRealTimers();
    }
  });

  it('disarms a pending Run when the active document changes', () => {
    const { rerender } = render(
      <PreviewStatusBar
        path="a.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={idle}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    fireEvent.click(runButton()!);
    expect(runButton()).toBeDisabled();

    rerender(
      <PreviewStatusBar
        path="b.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    // New doc, no capture -> back to "Run", not disabled.
    expect(runButton()).toHaveTextContent('Run');
    expect(runButton()).not.toBeDisabled();
  });

  // ---- clear confirm state machine (ported from ClearCaptureControl) ----

  it('requires confirmation before clearing, then calls onClear with the path', () => {
    const onClear = vi.fn();
    render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={idle}
        onRun={vi.fn()}
        onClear={onClear}
      />,
    );

    fireEvent.click(clearButton()!);
    expect(onClear).not.toHaveBeenCalled();
    // The confirmation must name the collaborator-wide effect.
    expect(screen.getByText(/collaborator/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /^clear$/i }));
    expect(onClear).toHaveBeenCalledTimes(1);
    expect(onClear).toHaveBeenCalledWith('doc.qmd');
  });

  it('cancelling the confirmation does not clear and restores the affordance', () => {
    const onClear = vi.fn();
    render(
      <PreviewStatusBar
        path="doc.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={idle}
        onRun={vi.fn()}
        onClear={onClear}
      />,
    );

    fireEvent.click(clearButton()!);
    fireEvent.click(screen.getByRole('button', { name: /cancel/i }));
    expect(onClear).not.toHaveBeenCalled();
    expect(clearButton()).toBeInTheDocument();
  });

  it('disarms a pending clear confirmation when the active document changes', () => {
    const { rerender } = render(
      <PreviewStatusBar
        path="a.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={idle}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    fireEvent.click(clearButton()!);
    expect(screen.getByText(/collaborator/i)).toBeInTheDocument();

    rerender(
      <PreviewStatusBar
        path="b.qmd"
        executorsOnline={true}
        hasExecutableCells={true}
        capture={idle}
        onRun={vi.fn()}
        onClear={vi.fn()}
      />,
    );
    // Confirmation is gone; back to the plain "Clear results…" affordance.
    expect(screen.queryByText(/collaborator/i)).toBeNull();
    expect(clearButton()).toBeInTheDocument();
  });
});
