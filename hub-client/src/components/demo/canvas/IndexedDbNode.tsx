/**
 * An IndexedDB database (`q2-demo-canvas`) behind an Automerge storage
 * adapter, offered on the "idb" port for repos to persist every document
 * they hold — exactly how hub-client's repo persists, and how several
 * tabs of it share one database. The cylinder lights while a repo writes;
 * the footer counts what the database holds.
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import type { StorageAdapterInterface } from '@automerge/automerge-repo';
import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb';
import { useOutput, type StorageAdapterPort } from './ports';

const DB_NAME = 'q2-demo-canvas';

interface Held { docs: number; bytes: number }

function Cylinder({ lit }: { lit: boolean }) {
  return (
    <svg className={`cd-cylinder ${lit ? 'lit' : ''}`} viewBox="0 0 64 80" width="64" height="80" aria-hidden="true">
      <path d="M8 16 v48 a24 8 0 0 0 48 0 v-48" />
      <ellipse cx="32" cy="16" rx="24" ry="8" />
      <path d="M8 32 a24 8 0 0 0 48 0" />
      <path d="M8 48 a24 8 0 0 0 48 0" />
    </svg>
  );
}

export default function IndexedDbNode() {
  const inner = useMemo(() => new IndexedDBStorageAdapter(DB_NAME), []);
  const [held, setHeld] = useState<Held | null>(null);
  const [writes, setWrites] = useState(0);
  const [lit, setLit] = useState(false);
  const litTimer = useRef<number | null>(null);

  // The adapter repos get: the real one, with a flash on every write.
  const adapter = useMemo<StorageAdapterInterface>(() => {
    const flash = () => {
      setWrites((n) => n + 1);
      setLit(true);
      if (litTimer.current !== null) window.clearTimeout(litTimer.current);
      litTimer.current = window.setTimeout(() => setLit(false), 300);
    };
    return {
      load: (key) => inner.load(key),
      save: (key, data) => { flash(); return inner.save(key, data); },
      remove: (key) => { flash(); return inner.remove(key); },
      loadRange: (prefix) => inner.loadRange(prefix),
      removeRange: (prefix) => { flash(); return inner.removeRange(prefix); },
    };
  }, [inner]);

  // What the database holds, re-counted a moment after writes settle.
  useEffect(() => {
    const timer = window.setTimeout(() => {
      void inner.loadRange([]).then((chunks) => {
        const docs = new Set(chunks.map((c) => c.key[0]));
        setHeld({ docs: docs.size, bytes: chunks.reduce((n, c) => n + (c.data?.byteLength ?? 0), 0) });
      });
    }, 500);
    return () => window.clearTimeout(timer);
  }, [inner, writes]);

  const port = useMemo<StorageAdapterPort>(() => ({ dbName: DB_NAME, adapter }), [adapter]);
  useOutput('idb', port);

  const clear = () => {
    if (!window.confirm(`Delete everything in the ${DB_NAME} database? Repos keep what they hold in memory.`)) return;
    void adapter.removeRange([]);
  };

  return (
    <div className="cd-node cd-idb">
      <Cylinder lit={lit} />
      <div className="cd-title">IndexedDB adapter</div>
      <div className="cd-meta">
        {held === null
          ? 'opening…'
          : held.docs === 0
            ? `${DB_NAME} · empty`
            : `${DB_NAME} · ${held.docs} doc${held.docs === 1 ? '' : 's'}, ${held.bytes} bytes`}
      </div>
      <button type="button" className="nodrag" onClick={clear} disabled={held === null || held.docs === 0}>clear</button>
      <Handle type="source" position={Position.Right} id="idb" title="idb" />
    </div>
  );
}
