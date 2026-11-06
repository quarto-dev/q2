/**
 * A localStorage slot for a document id, offered on its "docid" port. An
 * Automerge node wired to it opens the remembered id, and writes its id
 * here whenever the id changes.
 */

import { useMemo, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { useOutput, type IdStore } from './ports';

const KEY = 'q2-demo-canvas-hub-doc-id';

export default function LocalStorageNode() {
  const [id, setId] = useState<string | null>(() => localStorage.getItem(KEY));

  const store = useMemo<IdStore>(() => ({
    id,
    set: (next) => {
      localStorage.setItem(KEY, next);
      setId(next);
    },
  }), [id]);
  useOutput('docid', store);

  const clear = () => {
    localStorage.removeItem(KEY);
    setId(null);
  };

  return (
    <div className="cd-node cd-localstorage">
      <div className="cd-title">localStorage</div>
      <div className="cd-kv">
        <span className="cd-key">{KEY}</span>
        <span className="cd-id">{id ?? 'empty'}</span>
      </div>
      <button type="button" className="nodrag" onClick={clear} disabled={id === null}>clear</button>
      <Handle type="source" position={Position.Right} id="docid" title="doc id" />
    </div>
  );
}
