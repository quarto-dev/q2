/**
 * Sync Status Badge
 *
 * Dot + status line shown in the project and document bottom bars.
 * `scope: 'project'` watches the project index doc; `scope: 'document'`
 * watches the currently open file's doc. Clicking opens the
 * ConnectionStatusDialog (the affordance the old header online/offline
 * indicator provided).
 *
 * States:
 * - Disconnected (browser offline, websocket not open, or no peer
 *   handshake): yellow dot, "Saving locally — synced n minutes ago".
 * - Connected, sync activity this session within the last 15s (a remote
 *   change received, or a local change the hub confirmed delivered):
 *   green dot, "Synced just now".
 * - Connected, < 1 minute: yellow-green dot, "Synced <1 minute ago".
 * - Connected, otherwise: yellow-green dot, "Synced n minutes/hours/days ago".
 *
 * The doc's last-synced timestamp is mirrored to localStorage (keyed by
 * documentId) so a page that reloads while offline can still say how
 * stale its local copy is.
 */

import { useEffect, useState } from 'react';
import { getDocSyncActivity } from '@quarto/quarto-sync-client';
import { getFileHandle, getIndexHandle, getConnectionInfo } from '@quarto/preview-runtime';
import ConnectionStatusDialog from './ConnectionStatusDialog';
import { syncStatus as s } from '../strings';
import './SyncStatusBadge.css';

const SYNCING_WINDOW_MS = 15_000;
const WS_OPEN = 1; // WebSocket.OPEN

function storageKey(docId: string): string {
  return `q2hub.lastSyncedAt.${docId}`;
}

function readPersisted(docId: string): number | null {
  try {
    const n = Number(localStorage.getItem(storageKey(docId)));
    return Number.isFinite(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}

/** "less than a minute ago", then n minutes / hours / days ago. */
function agoText(deltaMs: number): string {
  const minutes = Math.floor(deltaMs / 60_000);
  if (minutes < 1) return s.underMinuteAgo;
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? '' : 's'} ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? '' : 's'} ago`;
  const days = Math.floor(hours / 24);
  return `${days} day${days === 1 ? '' : 's'} ago`;
}

interface SyncStatusBadgeProps {
  scope: 'project' | 'document';
  /** Path of the open file; only used when scope is 'document'. */
  currentFilePath?: string | null;
}

export default function SyncStatusBadge({ scope, currentFilePath }: SyncStatusBadgeProps) {
  const [now, setNow] = useState(() => Date.now());
  const [showDialog, setShowDialog] = useState(false);

  useEffect(() => {
    const refresh = () => setNow(Date.now());
    const intervalId = setInterval(refresh, 1000);
    window.addEventListener('online', refresh);
    window.addEventListener('offline', refresh);
    return () => {
      clearInterval(intervalId);
      window.removeEventListener('online', refresh);
      window.removeEventListener('offline', refresh);
    };
  }, []);

  // Resolve the watched doc on every tick; handles appear as the
  // project loads / files open.
  let docId: string | null = null;
  try {
    docId =
      scope === 'project'
        ? (getIndexHandle()?.documentId ?? null)
        : currentFilePath
          ? (getFileHandle(currentFilePath)?.documentId ?? null)
          : null;
  } catch {
    // not connected yet
  }

  // Sync activity = remote changes received, plus local changes the hub
  // has confirmed delivered (remote-heads ack). Raw sync-protocol
  // messages are deliberately excluded: a page refresh floods handshake
  // sync messages for every doc, which would misreport freshness on load.
  const activity = docId ? getDocSyncActivity(docId) : null;
  const inMemory =
    Math.max(activity?.lastRemoteChangeAt ?? 0, activity?.lastLocalDeliveredAt ?? 0) || null;
  const lastSyncedAt = Math.max(inMemory ?? 0, (docId && readPersisted(docId)) || 0) || null;

  const connInfo = getConnectionInfo();
  const connected =
    navigator.onLine &&
    connInfo?.wsReadyState === WS_OPEN &&
    (connInfo?.peers.length ?? 0) > 0;

  // Mirror the freshest in-memory timestamp to localStorage. A connected
  // client with no history at all (fresh browser / cleared storage) seeds
  // "now" as its baseline: the initial load just brought the doc up to
  // date, so that's a real last-synced time.
  useEffect(() => {
    if (!docId) return;
    const stored = readPersisted(docId) ?? 0;
    const next = inMemory && inMemory > stored ? inMemory : !stored && connected ? Date.now() : null;
    if (next) {
      try {
        localStorage.setItem(storageKey(docId), String(next));
      } catch {
        // storage full/unavailable — staleness just won't survive reload
      }
    }
  }, [docId, inMemory, connected]);

  let dotClass: string;
  let prefix: string;
  let detail: string;
  if (!connected) {
    dotClass = 'yellow';
    prefix = `${s.savingLocally} — `;
    detail = lastSyncedAt ? s.syncedAgo(agoText(now - lastSyncedAt)) : s.neverSynced;
  } else if (inMemory && now - inMemory < SYNCING_WINDOW_MS) {
    // Green only for sync activity observed *this session* — the
    // persisted/seeded timestamp must not light "just now" on load.
    dotClass = 'green';
    prefix = `${s.synced} `;
    detail = s.justNow;
  } else if (lastSyncedAt && now - lastSyncedAt < 60_000) {
    dotClass = 'yellow-green';
    prefix = `${s.synced} `;
    detail = s.underMinuteAgo;
  } else {
    dotClass = 'yellow-green';
    prefix = `${s.synced} `;
    detail = lastSyncedAt ? agoText(now - lastSyncedAt) : s.neverSynced;
  }

  return (
    <>
      <button
        className="sync-status-badge"
        onClick={() => setShowDialog(true)}
        title={s.tooltip}
      >
        <span className={`sync-status-dot ${dotClass}`} aria-hidden="true" />
        <span className="sync-status-text">
          {prefix}
          <em>{detail}</em>
        </span>
      </button>
      {showDialog && (
        <ConnectionStatusDialog
          currentFilePath={scope === 'document' ? currentFilePath : null}
          onClose={() => setShowDialog(false)}
        />
      )}
    </>
  );
}
