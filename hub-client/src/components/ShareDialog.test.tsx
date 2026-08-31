/**
 * Accessibility tests for ShareDialog.
 *
 * Regression coverage for the WCAG 2.2 audit findings:
 * - 4.1.2: the dialog exposes role/aria-modal/aria-labelledby and the
 *   close button has an accessible name (previously only "×" text)
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import ShareDialog from './ShareDialog';

afterEach(cleanup);

const defaultProps = {
  isOpen: true,
  shareableUrl: 'https://hub.example.com/p/abc#key',
  onClose: vi.fn(),
};

describe('ShareDialog accessibility', () => {
  it('exposes dialog semantics with the title as accessible name', () => {
    render(<ShareDialog {...defaultProps} />);
    const dialog = screen.getByRole('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    const labelId = dialog.getAttribute('aria-labelledby');
    expect(labelId).toBeTruthy();
    expect(document.getElementById(labelId!)?.textContent).toBe('Share project');
  });

  it('gives the close button an accessible name', () => {
    render(<ShareDialog {...defaultProps} />);
    expect(screen.getByRole('button', { name: 'Close' })).toBeDefined();
  });

  it('does not render when closed', () => {
    render(<ShareDialog {...defaultProps} isOpen={false} />);
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});

describe('ShareDialog Enter handling (GH #635 hardening)', () => {
  // Same contract as NewFileDialog (see ModalDialog's onKeyDown docs):
  // prevent Enter's default-action click, and leave button keydowns to
  // the button's own activation. ShareDialog's close is deferred 500ms
  // so the reopen bug can't bite today; these tests keep it that way if
  // the delay ever goes away.
  const stubClipboard = () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    return writeText;
  };

  it('copies on Enter in the URL input and prevents the default action', () => {
    const writeText = stubClipboard();
    render(<ShareDialog {...defaultProps} />);

    const urlInput = screen.getByDisplayValue(defaultProps.shareableUrl);
    const notPrevented = fireEvent.keyDown(urlInput, { key: 'Enter' });

    expect(writeText).toHaveBeenCalledWith(defaultProps.shareableUrl);
    expect(notPrevented).toBe(false);
  });

  it('does not copy when Enter is pressed on the close button', () => {
    const writeText = stubClipboard();
    render(<ShareDialog {...defaultProps} />);

    const closeButton = screen.getByRole('button', { name: 'Close' });
    closeButton.focus();
    fireEvent.keyDown(closeButton, { key: 'Enter' });

    expect(writeText).not.toHaveBeenCalled();
  });
});
