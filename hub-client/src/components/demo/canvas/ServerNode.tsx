/**
 * The hub server, offered on its "server" port as a websocket URL. Both
 * repos on the canvas — the Automerge node's and the hub project node's —
 * sync through it, which is why they see the same documents.
 */

import { useMemo, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { SYNC_SERVER_URL } from '../hubProject';
import { useOutput, type ServerPort } from './ports';
import TextBox from './TextBox';

export default function ServerNode() {
  const [url, setUrl] = useState(SYNC_SERVER_URL);
  // One object per url, so consumers reconnect only when it changes.
  const port = useMemo<ServerPort>(() => ({ url }), [url]);
  useOutput('server', port);
  return (
    <div className="cd-node cd-server">
      <div className="cd-title">Hub server</div>
      <TextBox value={url} placeholder="ws://host/ws" onCommit={setUrl} />
      <Handle type="source" position={Position.Right} id="server" title="server" />
    </div>
  );
}
