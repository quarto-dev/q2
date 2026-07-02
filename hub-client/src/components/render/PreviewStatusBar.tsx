import { useEffect, useState } from 'react';
import type { CaptureRef } from '@quarto/preview-runtime';

/**
 * The preview pane's single execution status line (bd-yai4w8ly).
 *
 * This merges what used to be three separate strips stacked above the preview
 * iframe:
 *   - `RunControl` — the "Executor online" + Run/Re-run affordance,
 *   - the inline `.executor-online-bar` read-only indicator, and
 *   - `ClearCaptureControl` — "Showing executed output" + Clear results.
 *
 * It renders **at most one row**: a green liveness dot (when an executor is
 * online), a single status label chosen by precedence, and a right-aligned
 * action group. The buttons are gated independently — **Clear** whenever a
 * capture exists, **Run/Re-run** whenever an executor is online and the doc has
 * executable cells — and rendered in DOM order `[Clear] [Run]` so Run stays
 * pinned to the far right as state transitions (see the plan,
 * claude-notes/plans/2026-07-01-merge-preview-status-line.md).
 *
 * Both mutations are injected (`onRun` = ephemeral `exec/request`, `onClear` =
 * shared `CaptureRef` sidecar delete) so this component stays presentational
 * and unit-testable.
 *
 * Two small local state machines are preserved from the former components:
 *   - a **run pending** snapshot (the `captureDocId` at request time) that
 *     shows "Executing…" optimistically and clears when a new capture arrives,
 *     the provider reports an error, the doc changes, or after
 *     {@link PENDING_TIMEOUT_MS} (an ephemeral request may reach no executor);
 *   - a **clear confirmation** (two-step, because clearing affects every
 *     collaborator), disarmed when the active document changes.
 */

/** How long to show "Executing…" before assuming the request was lost. */
export const PENDING_TIMEOUT_MS = 30_000;

export interface PreviewStatusBarProps {
  /** Active document path, or null when no document is open. */
  path: string | null;
  /** Whether at least one `q2` executor is online (a live capability beacon). */
  executorsOnline: boolean;
  /** Whether the active document has executable code cells. */
  hasExecutableCells: boolean;
  /** The active document's capture sidecar entry, if any. */
  capture?: CaptureRef;
  /** Broadcast an execute request for `path`. */
  onRun: (path: string) => void;
  /** Clear the capture for `path` (removes the shared sidecar entry). */
  onClear: (path: string) => void;
}

export function PreviewStatusBar({
  path,
  executorsOnline,
  hasExecutableCells,
  capture,
  onRun,
  onClear,
}: PreviewStatusBarProps) {
  // The `captureDocId` snapshot taken when we sent a run request, or null when
  // no request is in flight. `''` means "no capture existed at request time".
  const [pendingSnapshot, setPendingSnapshot] = useState<string | null>(null);
  // Two-step clear confirmation.
  const [confirming, setConfirming] = useState(false);

  const state = capture?.state;
  const captureDocId = capture?.captureDocId;

  // Disarm both local state machines when the active document changes, so a
  // pending run or a mid-confirm clear never carries over to a different file.
  useEffect(() => {
    setPendingSnapshot(null);
    setConfirming(false);
  }, [path]);

  // Clear the pending flag once a run resolves: a new capture arrived (doc id
  // changed) or the provider reported an error.
  useEffect(() => {
    if (pendingSnapshot === null) return;
    if (state === 'error' || (captureDocId ?? '') !== pendingSnapshot) {
      setPendingSnapshot(null);
    }
  }, [captureDocId, state, pendingSnapshot]);

  // Safety net: an ephemeral request may find no executor, so never stay
  // "Executing…" forever.
  useEffect(() => {
    if (pendingSnapshot === null) return;
    const timer = setTimeout(() => setPendingSnapshot(null), PENDING_TIMEOUT_MS);
    return () => clearTimeout(timer);
  }, [pendingSnapshot]);

  const hasCapture = !!capture;

  // Nothing to say and nothing to do — hide the bar entirely.
  if (!executorsOnline && !hasCapture) {
    return null;
  }

  const busy = pendingSnapshot !== null || state === 'running';
  const canRun = executorsOnline && hasExecutableCells && !!path;
  const canClear = hasCapture && !!path;
  const runLabel = busy ? 'Executing…' : hasCapture ? 'Re-run' : 'Run';

  const handleRun = () => {
    if (!path) return;
    setPendingSnapshot(captureDocId ?? '');
    onRun(path);
  };

  const handleConfirmClear = () => {
    if (path) onClear(path);
    setConfirming(false);
  };

  return (
    <div className="preview-status-bar" role="group" aria-label="Execution status">
      {executorsOnline && <span className="executor-online-dot" aria-hidden="true" />}

      {confirming ? (
        <span className="preview-status-label" role="alert">
          Clear executed output? This removes it for all collaborators until the
          document is run again.
        </span>
      ) : (
        <StatusLabel busy={busy} capture={capture} executorsOnline={executorsOnline} />
      )}

      <div className="preview-status-actions">
        {confirming ? (
          <>
            <button
              type="button"
              className="preview-status-clear-confirm-btn"
              onClick={handleConfirmClear}
            >
              Clear
            </button>
            <button
              type="button"
              className="preview-status-cancel-btn"
              onClick={() => setConfirming(false)}
            >
              Cancel
            </button>
          </>
        ) : (
          <>
            {canClear && (
              <button
                type="button"
                className="preview-status-clear-btn"
                onClick={() => setConfirming(true)}
              >
                Clear results…
              </button>
            )}
            {canRun && (
              <button
                type="button"
                className="preview-status-run-btn"
                onClick={handleRun}
                disabled={busy}
                aria-label={hasCapture ? 'Re-run code cells' : 'Run code cells'}
              >
                {runLabel}
              </button>
            )}
          </>
        )}
      </div>
    </div>
  );
}

/**
 * The single status label, chosen by precedence so transient run states win
 * over the steady-state "showing output" / "executor online" messages.
 */
function StatusLabel({
  busy,
  capture,
  executorsOnline,
}: {
  busy: boolean;
  capture?: CaptureRef;
  executorsOnline: boolean;
}) {
  if (busy) {
    return <span className="preview-status-label">Executing…</span>;
  }
  if (capture?.state === 'error' && capture.lastError) {
    return (
      <span className="preview-status-label preview-status-error" role="alert">
        {capture.lastError}
      </span>
    );
  }
  if (capture) {
    // Show BOTH facts when the capture is stale (decision 2): the output is
    // still displayed, and the code has changed since it was produced.
    return (
      <span className="preview-status-label">
        Showing executed output{capture.staleness ? ' · code changed' : ''}
      </span>
    );
  }
  // Guaranteed reachable only when an executor is online (the bar is hidden
  // when there is neither a capture nor an executor).
  return <span className="preview-status-label">{executorsOnline ? 'Executor online' : null}</span>;
}
