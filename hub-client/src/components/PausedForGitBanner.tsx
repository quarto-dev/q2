/**
 * The "paused for git" banner (GE-7, G33): shown while a `q2 preview --git track` session holds edits back during a git
 * operation. A session banner (tier 3 in `notifications.css`) and nothing more: there is no read-only mode. The wording
 * is GE-6's `PAUSED_BANNER`, the one source.
 *
 * Mount it only when the serving server is a preview; it starts the status poller on mount and stops it on unmount.
 */
import { useEffect, useSyncExternalStore } from 'react';
import type { GitStatusPoller } from '@quarto/preview-runtime';
import { PAUSED_BANNER } from '@quarto/quarto-git-epochs';
import './notifications.css';

export default function PausedForGitBanner({ poller }: { poller: GitStatusPoller }) {
  const snapshot = useSyncExternalStore(poller.subscribe, poller.getSnapshot);
  useEffect(() => {
    poller.start();
    return () => poller.stop();
  }, [poller]);
  if (!snapshot.paused) return null;
  return (
    <div className="paused-for-git-banner" role="status">
      {PAUSED_BANNER}
    </div>
  );
}
