/**
 * The replaced-file notice and the "this person switched versions" toast (GE-7, G25 and G26).
 *
 * When the project owner replaces files, the sync client reports it and `swapStatus` folds the events into one
 * aggregate notice per batch. That is a dismissible persistent notification (tier 2 in `notifications.css`): the user
 * should be able to read it and dismiss it. The text comes from GE-6's `NoticeAggregator`, the only source of the
 * wording. When a collaborator leaves the doc because of a switch, `presenceService` reports it and a transient toast
 * (tier 1) says so briefly.
 *
 * The live region stays mounted, so assistive technology announces the notice when its text appears. Render this on
 * editing surfaces only; a read-only view needs no notice, because its re-render already says everything.
 */
import { useCallback, useEffect, useState, useSyncExternalStore } from 'react';
import { swapStatus, type SwapStatusStore } from '@quarto/preview-runtime';
import { onPeerMoved as defaultOnPeerMoved, type PeerMovedEvent } from '../services/presenceService';
import { common } from '../strings';
import Toast from './Toast';
import './notifications.css';

/** How long the "switched versions" toast stays. Longer than the 2 s default: it carries a name to read. */
const MOVED_TOAST_MS = 4000;

interface Props {
  store?: SwapStatusStore;
  onPeerMoved?: (callback: (event: PeerMovedEvent) => void) => () => void;
}

export default function SwapNotices({ store = swapStatus, onPeerMoved = defaultOnPeerMoved }: Props) {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const [movedText, setMovedText] = useState<string | null>(null);

  useEffect(() => onPeerMoved((event) => setMovedText(event.text)), [onPeerMoved]);
  const hideMoved = useCallback(() => setMovedText(null), []);

  return (
    <div role="status" aria-live="polite" data-testid="swap-notice-region">
      {snapshot.notice !== null && (
        <div
          key={snapshot.noticeSeq}
          className="update-available-toast swap-notice"
          role="status"
          data-testid="swap-notice"
        >
          <span>{snapshot.notice}</span>
          <button
            type="button"
            className="update-available-toast-dismiss"
            aria-label={common.dismiss}
            onClick={() => store.dismissNotice()}
          >
            ×
          </button>
        </div>
      )}
      <Toast message={movedText ?? ''} visible={movedText !== null} onHide={hideMoved} duration={MOVED_TOAST_MS} />
    </div>
  );
}
