/**
 * A localStorage slot that remembers which hub project to open — its index
 * doc id, the way the app remembers the projects you have opened. Offered
 * on the "projectid" port for the hub project node. Seeded with the demo
 * project on first use; edit it to switch projects, clear it to forget.
 */

import { useMemo, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { INDEX_DOC_ID } from '../hubProject';
import { KeyValueIcon } from './icons';
import { useOutput, type IdStore } from './ports';
import TextBox from './TextBox';

const KEY = 'q2-demo-canvas-hub-project-id';
const bareId = (id: string) => id.replace(/^automerge:/, '');

function remembered(): string | null {
  const stored = localStorage.getItem(KEY);
  if (stored !== null) return stored;
  // First use: start on the demo project rather than on nothing.
  const seed = bareId(INDEX_DOC_ID);
  localStorage.setItem(KEY, seed);
  return seed;
}

export default function LocalStorageNode() {
  const [id, setId] = useState<string | null>(remembered);

  const store = useMemo<IdStore>(() => ({
    id,
    set: (next) => {
      const bare = bareId(next);
      localStorage.setItem(KEY, bare);
      setId(bare);
    },
  }), [id]);
  useOutput('projectid', store);

  const clear = () => {
    localStorage.removeItem(KEY);
    setId(null);
  };

  return (
    <div className="cd-node cd-localstorage">
      <div className="cd-title cd-title-icon"><KeyValueIcon /> localStorage</div>
      <div className="cd-kv">
        <span className="cd-key">{KEY}</span>
        <TextBox value={id ?? ''} placeholder="project (index doc) id" className="cd-project-id" onCommit={store.set} />
      </div>
      <button type="button" className="nodrag" onClick={clear} disabled={id === null}>clear</button>
      <Handle type="source" position={Position.Right} id="projectid" title="project id" />
    </div>
  );
}
