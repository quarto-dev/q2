/**
 * Stale-capture overlay (Phase C.5, bd-kw93.5).
 *
 * Shown when the active page's IndexDocument sidecar entry has
 * `staleness === true`. The previous capture continues to drive the
 * preview render (preview stays responsive); this overlay surfaces
 * the staleness signal and lets the user opt into re-executing.
 *
 * Behaviour:
 *   - Click → POST `/api/preview/re-execute` with the active path.
 *   - 202 Accepted → button disables, label switches to "Executing…"
 *     until the sidecar's `state` transitions out of `running`. The
 *     SPA's render effect re-fires off the new `captureDocId` via
 *     the existing `onCapturesChange` channel; no extra polling.
 *   - 409 Conflict → another tab already kicked off a re-execute;
 *     leave the overlay in place and let the in-flight run complete.
 *   - 4xx / 5xx → show the error inline; clicking again retries.
 *
 * Positioning matches `ForceRefreshButton`: absolute, top-left
 * corner of the preview pane (so it doesn't collide with the
 * top-right refresh button).
 */

import { useState } from 'react';

interface StaleCaptureOverlayProps {
  /** Project-relative path of the active page. */
  activePath: string;
  /**
   * `state` from the sidecar's `CaptureRef`. When `'running'`, the
   * server is already re-executing — disable the button and show a
   * spinner-style label.
   */
  state?: 'idle' | 'running' | 'error';
  /** Latest error from the sidecar, if any. Surfaced inline. */
  lastError?: string;
}

export function StaleCaptureOverlay({
  activePath,
  state,
  lastError,
}: StaleCaptureOverlayProps) {
  const [postError, setPostError] = useState<string | null>(null);
  const [isPosting, setIsPosting] = useState(false);

  const disabled = isPosting || state === 'running';
  const label =
    state === 'running'
      ? 'Executing…'
      : isPosting
        ? 'Submitting…'
        : 'Re-execute';

  const handleClick = async () => {
    setPostError(null);
    setIsPosting(true);
    try {
      const resp = await fetch('/api/preview/re-execute', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ path: activePath }),
      });
      if (resp.status === 202) {
        // Accepted; sidecar will flip via samod sync.
        return;
      }
      if (resp.status === 409) {
        setPostError('Another re-execute is already in flight.');
        return;
      }
      const text = await resp.text();
      setPostError(`Re-execute failed (${resp.status}): ${text}`);
    } catch (e) {
      setPostError(`Network error: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setIsPosting(false);
    }
  };

  return (
    <div
      role="status"
      aria-live="polite"
      style={{
        position: 'absolute',
        top: '0.75rem',
        left: '0.75rem',
        zIndex: 10,
        maxWidth: 'calc(100% - 5rem)',
        padding: '0.5rem 0.75rem',
        display: 'inline-flex',
        alignItems: 'center',
        gap: '0.75rem',
        border: '1px solid rgba(0, 0, 0, 0.15)',
        borderRadius: '0.375rem',
        background: 'rgba(255, 248, 220, 0.95)',
        color: 'rgba(0, 0, 0, 0.8)',
        fontSize: '0.875rem',
        lineHeight: 1.4,
        boxShadow: '0 1px 4px rgba(0, 0, 0, 0.08)',
      }}
    >
      <span>Code has changed since the last capture.</span>
      <button
        type="button"
        onClick={handleClick}
        disabled={disabled}
        aria-label="Re-execute code cells"
        style={{
          padding: '0.25rem 0.625rem',
          border: '1px solid rgba(0, 0, 0, 0.2)',
          borderRadius: '0.25rem',
          background: disabled ? 'rgba(0, 0, 0, 0.05)' : '#fff',
          color: disabled ? 'rgba(0, 0, 0, 0.45)' : 'inherit',
          cursor: disabled ? 'default' : 'pointer',
          fontSize: '0.825rem',
        }}
      >
        {label}
      </button>
      {(postError || lastError) && (
        <span
          role="alert"
          style={{
            color: 'rgba(180, 30, 30, 0.95)',
            fontSize: '0.825rem',
          }}
        >
          {postError ?? lastError}
        </span>
      )}
    </div>
  );
}
