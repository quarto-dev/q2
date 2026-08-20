/**
 * Modal Dialog
 *
 * Shared modal container for hub-client dialogs. Owns the WCAG 2.2
 * dialog contract so individual dialogs cannot drift:
 *
 * - 4.1.2 Name/Role/Value: role="dialog", aria-modal="true", and
 *   aria-labelledby pointing at the rendered title; the close button
 *   always has an accessible name.
 * - 2.1.1 Keyboard: Escape closes. Dialog-specific keys (e.g. Enter to
 *   submit) are delegated via the onKeyDown prop.
 * - 2.4.3 Focus Order: Tab/Shift+Tab cycle within the dialog while it
 *   is open, and focus returns to the element that held it before the
 *   dialog opened.
 */

import { useEffect, useId, useRef } from 'react';
import type { HTMLAttributes, KeyboardEvent, ReactNode } from 'react';

export interface ModalDialogProps {
  /** Visible title; also the dialog's accessible name. */
  title: string;
  onClose: () => void;
  /** Extra class for the dialog element, e.g. 'new-file-dialog'. */
  className?: string;
  /** Dialog-specific key handling (e.g. Enter to submit). Escape and Tab are owned by ModalDialog. */
  onKeyDown?: (e: KeyboardEvent<HTMLDivElement>) => void;
  /** Extra attributes forwarded to the dialog element (e.g. drag/drop handlers). */
  dialogProps?: HTMLAttributes<HTMLDivElement>;
  children?: ReactNode;
}

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export default function ModalDialog({
  title,
  onClose,
  className,
  onKeyDown,
  dialogProps,
  children,
}: ModalDialogProps) {
  const titleId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);

  // Capture the element focused when the dialog opens and return focus
  // to it when the dialog closes (unmounts).
  const restoreFocusTo = useRef<Element | null>(null);
  useEffect(() => {
    restoreFocusTo.current = document.activeElement;
    return () => {
      if (restoreFocusTo.current instanceof HTMLElement) {
        restoreFocusTo.current.focus();
      }
    };
  }, []);

  const handleKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      onClose();
      return;
    }
    if (e.key === 'Tab') {
      const focusables =
        dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR);
      if (!focusables || focusables.length === 0) {
        e.preventDefault();
        return;
      }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
      return;
    }
    onKeyDown?.(e);
  };

  return (
    <div className="ph-dialog-backdrop" onClick={onClose}>
      <div
        {...dialogProps}
        ref={dialogRef}
        className={`ph-dialog${className ? ` ${className}` : ''}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="dialog-header">
          <h2 id={titleId}>{title}</h2>
          <button className="close-btn" onClick={onClose} aria-label="Close">
            &times;
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}
