/**
 * In-context live inspector panel (`quartoDebug.openInspector()`).
 *
 * Mounts the /debug.html document displays (`DocumentList`,
 * `QuickPick`, `DocumentViewer`, `MessageLog`) against the **live
 * sync-client Repo** via `RepoContext`, plus JSON panes over the
 * `quartoDebug.am` reports (sync status, presence, doctor) and the
 * message tap. Unlike /debug.html — which runs in a separate browsing
 * context with its own server-connected Repo — everything shown here
 * is the editor's own in-memory state.
 *
 * Observation-only by convention: the panel adds no write affordances,
 * but it does hold live handles via the repo hooks — treat it like a
 * DevTools pane, not an editor surface. Loaded as a lazy chunk by
 * `services/debugInspector.ts`; never part of the main bundle.
 *
 * Tracking: bd-lb1cxprv. Plan:
 * `claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md`.
 */

import { useCallback, useEffect, useState } from 'react';
import { RepoContext } from '@automerge/automerge-repo-react-hooks';
import {
  isValidAutomergeUrl,
  type AutomergeUrl,
  type Repo,
} from '@automerge/automerge-repo';
import { DocumentList } from '../../debug/components/DocumentList';
import { DocumentViewer } from '../../debug/components/DocumentViewer';
import { QuickPick } from '../../debug/components/QuickPick';
import { MessageLog } from '../../debug/components/MessageLog';
import type { MessageLogEntry } from '../../debug/types/messages';
import type { QuartoDebugAutomergeApi } from '../../services/debugAutomerge';
import { clearTapMessages } from '../../services/debugMessageTap';
import type { TapMessage } from '../../services/debugMessageTap';
import './DebugInspectorPanel.css';

export interface DebugInspectorPanelProps {
  repo: Repo;
  am: QuartoDebugAutomergeApi;
  onClose: () => void;
}

type Tab = 'documents' | 'sync' | 'presence' | 'doctor' | 'messages';

const TABS: { id: Tab; label: string }[] = [
  { id: 'documents', label: 'Documents' },
  { id: 'sync', label: 'Sync' },
  { id: 'presence', label: 'Presence' },
  { id: 'doctor', label: 'Doctor' },
  { id: 'messages', label: 'Messages' },
];

function toUrl(docId: string): AutomergeUrl | null {
  const prefixed = `automerge:${docId}`;
  return isValidAutomergeUrl(prefixed) ? (prefixed as AutomergeUrl) : null;
}

/** JSON pane over a live report getter, refreshed on an interval. */
function RefreshPane({
  get,
  intervalMs = 2000,
}: {
  get: () => unknown;
  intervalMs?: number;
}) {
  const read = useCallback((): unknown => {
    try {
      return get();
    } catch (e) {
      return { error: String(e) };
    }
  }, [get]);
  const [value, setValue] = useState<unknown>(read);
  useEffect(() => {
    const t = setInterval(() => setValue(read()), intervalMs);
    return () => clearInterval(t);
  }, [read, intervalMs]);
  return <pre className="json-content">{JSON.stringify(value, null, 2)}</pre>;
}

/** Feed the reused MessageLog from the tap's ring buffer. */
function MessagesPane({ am }: { am: QuartoDebugAutomergeApi }) {
  const read = useCallback((): MessageLogEntry[] => {
    const tapMessages: TapMessage[] = am.messages({ limit: 200 }).messages;
    // Tap order is newest first; MessageLog appends chronologically.
    return [...tapMessages].reverse().map((m, i) => ({
      id: `${m.at}-${i}`,
      timestamp: new Date(m.at),
      direction: m.direction === 'incoming' ? 'incoming' : 'outgoing',
      type: m.type,
      senderId: (m.senderId ?? '') as MessageLogEntry['senderId'],
      targetId: (m.targetId ?? '') as MessageLogEntry['targetId'],
      documentId: m.documentId as MessageLogEntry['documentId'],
      dataSize: m.byteLength,
    }));
  }, [am]);
  const [entries, setEntries] = useState<MessageLogEntry[]>(read);
  useEffect(() => {
    const t = setInterval(() => setEntries(read()), 1000);
    return () => clearInterval(t);
  }, [read]);
  return (
    <MessageLog
      messages={entries}
      onClear={() => {
        clearTapMessages();
        setEntries([]);
      }}
    />
  );
}

export function DebugInspectorPanel({
  repo,
  am,
  onClose,
}: DebugInspectorPanelProps) {
  const [tab, setTab] = useState<Tab>('documents');
  const [docs, setDocs] = useState<AutomergeUrl[]>([]);

  // Seed the viewer with the project's index doc; its files map then
  // offers per-file subscribe buttons (DocumentViewer handles that).
  useEffect(() => {
    const indexEntry = am.docs().find((e) => e.role === 'index');
    const url = indexEntry ? toUrl(indexEntry.docId) : null;
    if (!url) return;
    let cancelled = false;
    void repo.find(url).then(
      () => {
        if (!cancelled) {
          setDocs((prev) => (prev.includes(url) ? prev : [...prev, url]));
        }
      },
      () => {
        /* Seed failure is non-fatal; docs can be added manually. */
      },
    );
    return () => {
      cancelled = true;
    };
  }, [repo, am]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const addDoc = (url: AutomergeUrl) =>
    setDocs((prev) => (prev.includes(url) ? prev : [...prev, url]));
  const removeDoc = (url: AutomergeUrl) =>
    setDocs((prev) => prev.filter((d) => d !== url));

  return (
    <RepoContext.Provider value={repo}>
      <div className="quarto-debug-inspector" role="dialog" aria-label="Live Inspector">
        <header className="inspector-header">
          <h1>Quarto Hub — Live Inspector</h1>
          <div role="tablist" aria-label="Inspector panes" className="inspector-tabs">
            {TABS.map((t) => (
              <button
                key={t.id}
                role="tab"
                aria-selected={tab === t.id}
                className={tab === t.id ? 'active' : ''}
                onClick={() => setTab(t.id)}
              >
                {t.label}
              </button>
            ))}
          </div>
          <button
            className="inspector-close"
            aria-label="Close inspector"
            title="Close inspector (Esc)"
            onClick={onClose}
          >
            ×
          </button>
        </header>

        {tab === 'documents' && (
          <div className="inspector-documents">
            <aside className="inspector-sidebar">
              <DocumentList
                documents={docs}
                onAdd={addDoc}
                onRemove={removeDoc}
              />
              <QuickPick documents={docs} onAdd={addDoc} />
            </aside>
            <section className="inspector-content">
              <DocumentViewer
                documents={docs}
                onAdd={addDoc}
                onRemove={removeDoc}
              />
            </section>
          </div>
        )}
        {tab === 'sync' && <RefreshPane get={() => am.syncStatus()} />}
        {tab === 'presence' && <RefreshPane get={() => am.presence()} />}
        {tab === 'doctor' && <RefreshPane get={() => am.doctor()} />}
        {tab === 'messages' && <MessagesPane am={am} />}
      </div>
    </RepoContext.Provider>
  );
}
