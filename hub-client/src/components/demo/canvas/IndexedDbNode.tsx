/**
 * An IndexedDB-backed store, offered on its "storage" port. Owns the
 * adapter; reports what it holds and when it was last written.
 */

import { useEffect, useMemo, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb';
import { useOutput, type DocStore } from './ports';

const DB_NAME = 'q2-demo-canvas';
const PREFIX = ['canvas'];
const keyFor = (id: string) => [...PREFIX, id];

interface Stored { bytes: number; at: number | null }

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
  const adapter = useMemo(() => new IndexedDBStorageAdapter(DB_NAME), []);
  const [stored, setStored] = useState<Stored | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void adapter.loadRange(PREFIX).then((chunks) => {
      const bytes = chunks.reduce((n, c) => n + (c.data?.byteLength ?? 0), 0);
      setStored(bytes === 0 ? null : { bytes, at: null });
    });
  }, [adapter]);

  const store = useMemo<DocStore>(() => ({
    load: (id) => adapter.load(keyFor(id)),
    save: async (id, bytes) => {
      setBusy(true);
      await adapter.save(keyFor(id), bytes);
      setStored({ bytes: bytes.byteLength, at: Date.now() });
      setBusy(false);
    },
  }), [adapter]);
  useOutput('storage', store);

  const clear = () => {
    void adapter.removeRange(PREFIX).then(() => setStored(null));
  };

  return (
    <div className="cd-node cd-idb">
      <Handle type="source" position={Position.Top} id="storage" title="storage" />
      <Cylinder lit={busy} />
      <div className="cd-title">IndexedDB</div>
      <div className="cd-meta">
        {stored === null
          ? 'empty'
          : `${stored.bytes} bytes${stored.at !== null ? `, saved ${new Date(stored.at).toLocaleTimeString()}` : ''}`}
      </div>
      <button type="button" className="nodrag" onClick={clear} disabled={stored === null}>clear</button>
    </div>
  );
}
