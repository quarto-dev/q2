/**
 * The "paused for git" banner (GE-7, G33): shown while a `q2 preview --git track` preview holds edits back during a git
 * operation. A banner only; there is no read-only mode. The wording is GE-6's `PAUSED_BANNER`, the one source.
 */
import { useEffect, useSyncExternalStore } from 'react';
import type { GitStatusPoller } from '@quarto/preview-runtime';
import { PAUSED_BANNER } from '@quarto/quarto-git-epochs';

const BANNER_STYLE: React.CSSProperties = {
  position: 'absolute',
  top: 44,
  left: '50%',
  transform: 'translateX(-50%)',
  zIndex: 1000,
  padding: '6px 14px',
  borderRadius: 6,
  background: '#fff8e1',
  border: '1px solid #e0d4a8',
  color: '#6b5d2e',
  font: '13px -apple-system, Segoe UI, sans-serif',
  boxShadow: '0 1px 4px rgba(0,0,0,0.15)',
};

export function PausedBanner({ poller }: { poller: GitStatusPoller }) {
  const snapshot = useSyncExternalStore(poller.subscribe, poller.getSnapshot);
  useEffect(() => {
    poller.start();
    return () => poller.stop();
  }, [poller]);
  if (!snapshot.paused) return null;
  return (
    <div role="status" aria-live="polite" style={BANNER_STYLE} data-testid="paused-banner">
      {PAUSED_BANNER}
    </div>
  );
}
