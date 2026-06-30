import { useState, useEffect } from 'react';

/**
 * Per-document "clear results" affordance (bd-sfet3264, D6).
 *
 * When the active document has a recorded engine capture, the preview shows
 * executed output spliced in from that capture. This control lets a user
 * remove that output — returning the document to its source-only state —
 * *without* re-executing. It is distinct from re-execution (which replaces a
 * capture) and from staleness (which keeps the capture but flags it).
 *
 * Clearing removes the `CaptureRef` sidecar entry, which is shared across the
 * session, so it affects every collaborator. To avoid an accidental
 * destructive click we use a two-step inline confirmation that names that
 * effect (rather than a browser `confirm()` dialog, which is harder to style
 * and to test, and blocks the event loop).
 *
 * The actual mutation is injected via `onClear` so this component stays
 * presentational and unit-testable; the wiring to `clearCapture` lives in the
 * parent.
 */
export interface ClearCaptureControlProps {
  /** Active document path, or null when no document is open. */
  path: string | null;
  /** Whether the active document currently has a capture entry. */
  hasCapture: boolean;
  /** Invoked with the active path when the user confirms the clear. */
  onClear: (path: string) => void;
}

export function ClearCaptureControl({ path, hasCapture, onClear }: ClearCaptureControlProps) {
  const [confirming, setConfirming] = useState(false);

  // Disarm the confirmation when the active document changes, so a pending
  // confirm never carries over to a different file.
  useEffect(() => {
    setConfirming(false);
  }, [path]);

  if (!path || !hasCapture) {
    return null;
  }

  if (confirming) {
    return (
      <div className="capture-results-bar" role="alertdialog" aria-label="Confirm clear results">
        <span className="capture-results-label">
          Clear executed output? This removes it for all collaborators until the
          document is run again.
        </span>
        <button
          type="button"
          className="capture-clear-confirm-btn"
          onClick={() => {
            onClear(path);
            setConfirming(false);
          }}
        >
          Clear
        </button>
        <button
          type="button"
          className="capture-clear-cancel-btn"
          onClick={() => setConfirming(false)}
        >
          Cancel
        </button>
      </div>
    );
  }

  return (
    <div className="capture-results-bar">
      <span className="capture-results-label">Showing executed output</span>
      <button
        type="button"
        className="capture-clear-btn"
        onClick={() => setConfirming(true)}
      >
        Clear results…
      </button>
    </div>
  );
}
