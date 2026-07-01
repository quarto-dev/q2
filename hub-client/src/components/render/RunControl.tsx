import { useEffect, useState } from 'react';
import type { CaptureRef } from '@quarto/preview-runtime';

/**
 * Preview "Run" affordance (bd-sfet3264, Phase 4b).
 *
 * When a `q2` executor is online (a live capability beacon) and the active
 * document has executable cells, this control lets a collaborator ask the
 * executor to run the document. The parent (Editor) gates rendering on those
 * two conditions; this component owns the run UX and reflects the durable
 * `CaptureRef` status the provider writes back.
 *
 * The trigger is an ephemeral `exec/request` (via `onRun`), not q2-preview's
 * loopback HTTP POST — but the UX mirrors q2-preview-spa's `StaleCaptureOverlay`:
 * a state-reflecting label, disabled while a run is in flight, and inline errors.
 *
 * Because the request is a best-effort ephemeral broadcast (it may reach no
 * executor), the local "pending" flag doesn't wait forever: it clears when a
 * new capture arrives (the `captureDocId` changes), when the provider reports
 * an error, or after {@link PENDING_TIMEOUT_MS}.
 */

/** How long to show "Executing…" before assuming the request was lost. */
export const PENDING_TIMEOUT_MS = 30_000;

export interface RunControlProps {
  /** Active document path, or null when no document is open. */
  path: string | null;
  /** The active document's capture sidecar entry, if any. */
  capture?: CaptureRef;
  /** Broadcast an execute request for `path`. */
  onRun: (path: string) => void;
}

export function RunControl({ path, capture, onRun }: RunControlProps) {
  // The `captureDocId` snapshot taken when we sent a request, or null when no
  // request is in flight. `''` means "no capture existed at request time".
  const [pendingSnapshot, setPendingSnapshot] = useState<string | null>(null);

  const state = capture?.state;
  const captureDocId = capture?.captureDocId;

  // Disarm a pending request when the active document changes.
  useEffect(() => {
    setPendingSnapshot(null);
  }, [path]);

  // Clear the pending flag once the run resolves: a new capture arrived (doc id
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

  if (!path) return null;

  const busy = pendingSnapshot !== null || state === 'running';
  const hasCapture = !!capture;
  const label = busy ? 'Executing…' : hasCapture ? 'Re-run' : 'Run';

  const handleClick = () => {
    setPendingSnapshot(captureDocId ?? '');
    onRun(path);
  };

  return (
    <div className="run-control" role="group" aria-label="Execute document">
      <span className="executor-online-dot" aria-hidden="true" />
      {state === 'error' && capture?.lastError ? (
        <span className="run-control-error" role="alert">
          {capture.lastError}
        </span>
      ) : capture?.staleness && !busy ? (
        <span className="run-control-label">Code changed since the last run.</span>
      ) : (
        <span className="run-control-label">Executor online</span>
      )}
      <button
        type="button"
        className="run-control-btn"
        onClick={handleClick}
        disabled={busy}
        aria-label={hasCapture ? 'Re-run code cells' : 'Run code cells'}
      >
        {label}
      </button>
    </div>
  );
}
