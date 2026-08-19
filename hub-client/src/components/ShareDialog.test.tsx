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
import { render, screen, cleanup } from '@testing-library/react';
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
    expect(document.getElementById(labelId!)?.textContent).toBe('Share Project');
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
