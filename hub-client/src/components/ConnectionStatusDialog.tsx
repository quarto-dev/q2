/**
 * Connection Status Dialog
 *
 * Read-only modal opened by clicking the header's online/offline
 * indicator. Shows the browser's own network status, the sync-server
 * connection state, per-document activity stats (current file vs. the
 * project index doc), and the diff of the last remote change to each.
 * Refreshes once per second while open.
 */

import { useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import {
  getDocSyncActivity,
  getDocRemoteChange,
  getConnectionLog,
  type DocSyncActivity,
  type RemoteChangeSummary,
} from '@quarto/quarto-sync-client';
import { getFileHandle, getIndexHandle, getConnectionInfo } from '@quarto/preview-runtime';
import type { Patch } from '@automerge/automerge-repo';
import fastDiff from 'fast-diff';
import ModalDialog from './ModalDialog';
import { dialogs } from '../strings';
import './ConnectionStatusDialog.css';

interface ConnectionStatusDialogProps {
  /** Path of the currently open file, for per-document stats. */
  currentFilePath?: string | null;
  onClose: () => void;
}

/** Same badge look as the header's connection indicator. */
function StatusBadge({ ok, label }: { ok: boolean; label: string }) {
  return (
    <span className={`connection-status-badge ${ok ? 'online' : 'offline'}`}>
      <span className="connection-dot" aria-hidden="true" />
      {label}
    </span>
  );
}

function preview(value: unknown, max = 40): string {
  const text = typeof value === 'string' ? value : JSON.stringify(value);
  const s = text ?? String(value);
  return JSON.stringify(s.length > max ? `${s.slice(0, max)}…` : s);
}

/** One human-readable line per Automerge patch. */
function formatPatch(p: Patch): string {
  const path = p.path.join('.');
  switch (p.action) {
    case 'splice':
      return `${path}: insert ${preview(p.value)}`;
    case 'del':
      return `${path}: delete${p.length !== undefined ? ` ${p.length} chars` : ''}`;
    case 'put':
      return `${path} = ${preview(p.value)}`;
    case 'insert':
      return `${path}: insert ${p.values.length} item(s)`;
    case 'inc':
      return `${path}: +${p.value}`;
    default:
      return `${path}: ${p.action}`;
  }
}

/**
 * Slice both texts down to the changed lines plus one line of context
 * above and below, so the diff view fits without scrolling.
 */
function trimToChangedRegion(
  before: string,
  after: string,
): { before: string; after: string; afterStartLine: number } {
  const b = before.split('\n');
  const a = after.split('\n');
  let prefix = 0;
  while (prefix < b.length && prefix < a.length && b[prefix] === a[prefix]) prefix++;
  let suffix = 0;
  while (
    suffix < b.length - prefix &&
    suffix < a.length - prefix &&
    b[b.length - 1 - suffix] === a[a.length - 1 - suffix]
  ) {
    suffix++;
  }
  const start = Math.max(0, prefix - 1);
  return {
    before: b.slice(start, b.length - suffix + 1).join('\n'),
    after: a.slice(start, a.length - suffix + 1).join('\n'),
    /** 1-based line number in the full "after" doc of the slice's first line. */
    afterStartLine: start + 1,
  };
}

interface DiffLine {
  /** Line number in the full "after" doc; null for deleted-only lines. */
  num: number | null;
  spans: Array<{ op: -1 | 0 | 1; text: string }>;
}

/**
 * Character-level inline diff of the trimmed region, folded into
 * rendered lines: deletions and insertions appear within the line
 * they touch rather than as separate red/green line blocks.
 */
function buildInlineDiffLines(
  before: string,
  after: string,
  afterStartLine: number,
): DiffLine[] {
  const lines: DiffLine[] = [{ num: null, spans: [] }];
  let afterLine = afterStartLine;
  for (const [op, text] of fastDiff(before, after)) {
    const parts = text.split('\n');
    parts.forEach((part, i) => {
      if (i > 0) {
        // A newline in equal/inserted text advances the after-doc line;
        // a deleted newline just breaks the rendered line.
        if (op !== fastDiff.DELETE) afterLine++;
        lines.push({ num: null, spans: [] });
      }
      if (part) {
        const line = lines[lines.length - 1];
        line.spans.push({ op, text: part });
        // A line earns its number from its first surviving content.
        if (op !== fastDiff.DELETE && line.num === null) line.num = afterLine;
      }
    });
  }
  if (lines.length > 1 && lines[lines.length - 1].spans.length === 0) lines.pop();
  return lines;
}

/** "42s ago" under a minute, then "3m ago", then "2h ago". */
function formatAgo(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.floor(minutes / 60)}h ago`;
}

function formatTimestamp(at: number | null, now: number): string {
  if (at === null) return dialogs.connectionStatus.never;
  return `${new Date(at).toLocaleTimeString()} (${formatAgo(now - at)})`;
}

/** Last remote change to one document: header + inline diff or patch list. */
function RemoteChangeSection({
  title,
  change,
  now,
}: {
  title: string;
  change: RemoteChangeSummary | null;
  now: number;
}) {
  const s = dialogs.connectionStatus;
  const diffLines = useMemo(() => {
    if (!change || change.beforeText === undefined || change.afterText === undefined) {
      return null;
    }
    const trimmed = trimToChangedRegion(change.beforeText, change.afterText);
    return buildInlineDiffLines(trimmed.before, trimmed.after, trimmed.afterStartLine);
  }, [change]);

  return (
    <div className="connection-status-diff">
      <div className="connection-status-diff-header">
        {s.lastRemoteChange} — {title} ·{' '}
        {change ? formatTimestamp(change.at, now) : s.never}
      </div>
      {change &&
        (diffLines ? (
          <div className="connection-status-inline-diff">
            {diffLines.map((line, i) => (
              <div className="inline-diff-line" key={i}>
                <span className="inline-diff-gutter">{line.num ?? ''}</span>
                <span className="inline-diff-content">
                  {line.spans.map((sp, j) => (
                    <span
                      key={j}
                      className={sp.op === -1 ? 'del' : sp.op === 1 ? 'ins' : ''}
                    >
                      {sp.text}
                    </span>
                  ))}
                </span>
              </div>
            ))}
          </div>
        ) : (
          <ul className="connection-status-patches">
            {change.patches.map((p, i) => (
              <li key={i}>{formatPatch(p)}</li>
            ))}
            {change.patchCount > change.patches.length && (
              <li>{s.morePatches(change.patchCount - change.patches.length)}</li>
            )}
          </ul>
        ))}
    </div>
  );
}

export default function ConnectionStatusDialog({
  currentFilePath,
  onClose,
}: ConnectionStatusDialogProps) {
  const [now, setNow] = useState(() => Date.now());
  const [browserOnline, setBrowserOnline] = useState(() => navigator.onLine);

  useEffect(() => {
    const refresh = () => {
      setNow(Date.now());
      setBrowserOnline(navigator.onLine);
    };
    const intervalId = setInterval(refresh, 1000);
    window.addEventListener('online', refresh);
    window.addEventListener('offline', refresh);
    return () => {
      clearInterval(intervalId);
      window.removeEventListener('online', refresh);
      window.removeEventListener('offline', refresh);
    };
  }, []);

  // Resolve the two documents of interest; re-evaluated every tick
  // because handles appear as the project loads / files open.
  let fileDocId: string | null = null;
  let indexDocId: string | null = null;
  try {
    fileDocId = currentFilePath ? (getFileHandle(currentFilePath)?.documentId ?? null) : null;
  } catch {
    // not connected yet
  }
  try {
    indexDocId = getIndexHandle()?.documentId ?? null;
  } catch {
    // not connected yet
  }
  const fileStats = fileDocId ? getDocSyncActivity(fileDocId) : null;
  const indexStats = indexDocId ? getDocSyncActivity(indexDocId) : null;
  const fileChange = fileDocId ? getDocRemoteChange(fileDocId) : null;
  const indexChange = indexDocId ? getDocRemoteChange(indexDocId) : null;

  const connInfo = getConnectionInfo();
  const connLog = getConnectionLog();
  const wsState = connInfo?.wsReadyState ?? null;
  const peers = connInfo?.peers ?? [];
  const WS_STATE_NAMES = ['Connecting', 'Open', 'Closing', 'Closed'];

  const s = dialogs.connectionStatus;
  const rows: Array<[string, ReactNode]> = [
    [
      s.browserNetwork,
      <StatusBadge ok={browserOnline} label={browserOnline ? s.browserOnline : s.browserOffline} />,
    ],
    [
      s.webSocket,
      <StatusBadge
        ok={wsState === WebSocket.OPEN}
        label={wsState === null ? s.noSocket : (WS_STATE_NAMES[wsState] ?? String(wsState))}
      />,
    ],
    [
      s.peerHandshake,
      <StatusBadge
        ok={peers.length > 0}
        label={peers.length > 0 ? s.peerEstablished : s.peerNone}
      />,
    ],
  ];

  const statDefs: Array<[string, keyof DocSyncActivity]> = [
    [s.lastEphemeralMessage, 'lastEphemeralMessageAt'],
    [s.lastRemoteChange, 'lastRemoteChangeAt'],
  ];

  const statCell = (stats: DocSyncActivity | null, key: keyof DocSyncActivity): ReactNode => {
    if (!stats) return '—';
    const at = stats[key];
    if (at === null) return s.never;
    return <span title={new Date(at).toLocaleTimeString()}>{formatAgo(now - at)}</span>;
  };

  return (
    <ModalDialog title={s.title} className="connection-status-dialog wide" onClose={onClose}>
      <div className="dialog-content">
        <dl className="connection-status-list">
          {rows.map(([label, value]) => (
            <div className="connection-status-row" key={label}>
              <dt>{label}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        </dl>
        <table className="connection-status-table">
          <thead>
            <tr>
              <th />
              <th>{s.thisFile}</th>
              <th>{s.project}</th>
            </tr>
          </thead>
          <tbody>
            {statDefs.map(([label, key]) => (
              <tr key={key}>
                <th>{label}</th>
                <td>{statCell(fileStats, key)}</td>
                <td>{statCell(indexStats, key)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <RemoteChangeSection title={s.thisFile} change={fileChange} now={now} />
        <RemoteChangeSection title={s.project} change={indexChange} now={now} />
        <div className="connection-status-diff">
          <div className="connection-status-diff-header">{s.connectionLog}</div>
          <ul className="connection-status-patches">
            {connLog.length === 0 && <li>{s.never}</li>}
            {connLog.slice(0, 12).map((e, i) => (
              <li key={i}>
                {new Date(e.at).toLocaleTimeString()} · {e.kind}
                {e.detail ? ` · ${e.detail}` : ''}
              </li>
            ))}
          </ul>
        </div>
      </div>
    </ModalDialog>
  );
}
