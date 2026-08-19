/**
 * Tests for the shared ModalDialog container.
 *
 * Verifies the WCAG 2.2 requirements the container owns:
 * - 4.1.2 Name/Role/Value: role="dialog", aria-modal, aria-labelledby
 * - 2.4.3 Focus Order: Tab containment and focus restore on close
 * - 2.1.1 Keyboard: Escape closes; other keys pass through
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import type { KeyboardEvent } from 'react';
import ModalDialog from './ModalDialog';

afterEach(cleanup);

function renderDialog(
  onClose = vi.fn(),
  onKeyDown?: (e: KeyboardEvent<HTMLDivElement>) => void,
) {
  return render(
    <ModalDialog
      title="Test dialog"
      className="test-dialog"
      onClose={onClose}
      onKeyDown={onKeyDown}
    >
      <div className="dialog-content">
        <button className="first-action">First</button>
        <button className="second-action">Second</button>
      </div>
    </ModalDialog>,
  );
}

describe('ModalDialog semantics', () => {
  it('exposes role="dialog" with aria-modal and aria-labelledby pointing at the title', () => {
    renderDialog();
    const dialog = screen.getByRole('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    const labelId = dialog.getAttribute('aria-labelledby');
    expect(labelId).toBeTruthy();
    expect(document.getElementById(labelId!)?.textContent).toBe('Test dialog');
  });

  it('gives the close button an accessible name', () => {
    renderDialog();
    expect(screen.getByRole('button', { name: 'Close' })).toBeDefined();
  });

  it('applies the extra class to the dialog element', () => {
    renderDialog();
    expect(screen.getByRole('dialog').className).toContain('test-dialog');
  });
});

describe('ModalDialog keyboard behavior', () => {
  it('closes on Escape', () => {
    const onClose = vi.fn();
    renderDialog(onClose);
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('forwards other keys to the dialog-specific handler', () => {
    const onKeyDown = vi.fn();
    renderDialog(vi.fn(), onKeyDown);
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Enter' });
    expect(onKeyDown).toHaveBeenCalled();
  });

  it('keeps Tab cycling inside the dialog', () => {
    renderDialog();
    const closeBtn = screen.getByRole('button', { name: 'Close' });
    const last = screen.getByText('Second');

    last.focus();
    expect(document.activeElement).toBe(last);

    // Tab past the last focusable element wraps to the first.
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Tab' });
    expect(document.activeElement).toBe(closeBtn);

    // Shift+Tab before the first wraps to the last.
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Tab', shiftKey: true });
    expect(document.activeElement).toBe(last);
  });

  it('restores focus to the previously focused element on unmount', () => {
    const trigger = document.createElement('button');
    trigger.textContent = 'Open dialog';
    document.body.appendChild(trigger);
    trigger.focus();

    const { unmount } = renderDialog();
    unmount();

    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });
});

describe('ModalDialog backdrop', () => {
  it('closes on backdrop click but not on dialog click', () => {
    const onClose = vi.fn();
    const { container } = renderDialog(onClose);

    fireEvent.click(container.querySelector('.ph-dialog-backdrop')!);
    expect(onClose).toHaveBeenCalledTimes(1);

    onClose.mockClear();
    fireEvent.click(screen.getByRole('dialog'));
    expect(onClose).not.toHaveBeenCalled();
  });
});
