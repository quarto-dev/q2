/**
 * The replaced-file notice (GE-7, G26): one aggregate sentence per batch when the project owner replaces files, in an
 * `aria-live` region so assistive technology announces it. Editing surfaces only: the default read-only viewer shows
 * none, because its re-render already says everything.
 */
import { useSyncExternalStore } from 'react';
import { swapStatus, type SwapStatusStore } from '@quarto/preview-runtime';

const NOTICE_STYLE: React.CSSProperties = {
  position: 'absolute',
  bottom: 12,
  left: '50%',
  transform: 'translateX(-50%)',
  zIndex: 1000,
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  maxWidth: 'min(640px, calc(100% - 24px))',
  padding: '8px 14px',
  borderRadius: 6,
  background: '#e8f1fb',
  border: '1px solid #b6cfe8',
  color: '#1f3b57',
  font: '13px -apple-system, Segoe UI, sans-serif',
  boxShadow: '0 1px 4px rgba(0,0,0,0.15)',
};

const DISMISS_STYLE: React.CSSProperties = {
  border: 'none',
  background: 'transparent',
  color: 'inherit',
  font: 'inherit',
  cursor: 'pointer',
  textDecoration: 'underline',
};

export function SwapNotice({ store = swapStatus }: { store?: SwapStatusStore }) {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  return (
    // The region stays mounted so a live region exists before its text changes; screen readers announce the change.
    <div role="status" aria-live="polite" data-testid="swap-notice-region">
      {snapshot.notice !== null && (
        <div key={snapshot.noticeSeq} style={NOTICE_STYLE} data-testid="swap-notice">
          <span>{snapshot.notice}</span>
          <button type="button" style={DISMISS_STYLE} onClick={() => store.dismissNotice()}>
            Dismiss
          </button>
        </div>
      )}
    </div>
  );
}
