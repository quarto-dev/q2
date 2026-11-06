/**
 * An Automerge websocket network adapter for the server whose URL is
 * wired into "server": the piece of a repo that speaks the sync protocol over one
 * WebSocket. A repo owns its adapter and the adapter's disconnect is
 * terminal (see the sync client's StoppableWebSocketClientAdapter), so
 * this node is the source of adapters for one repo: every connection that
 * takes one gets a fresh instance, counted here. Offered on "adapter".
 */

import { useMemo, useState } from 'react';
import { Handle, Position, useNodeId } from '@xyflow/react';
import { StoppableWebSocketClientAdapter } from '@quarto/quarto-sync-client';
import type { NetworkAdapter } from '@automerge/automerge-repo';
import RadioTower from '../RadioTower';
import { useInput, useOutput, type AdapterPort } from './ports';

export default function WebSocketAdapterNode() {
  const url = useInput<string>('server');
  const nodeId = useNodeId() ?? 'adapter';
  const [taken, setTaken] = useState(0);

  const port = useMemo<AdapterPort | undefined>(() => {
    if (url === null) return undefined;
    return {
      id: `${nodeId}:${url}`,
      url,
      take: (): NetworkAdapter => {
        setTaken((n) => n + 1);
        return new StoppableWebSocketClientAdapter(url) as unknown as NetworkAdapter;
      },
    };
  }, [url, nodeId]);
  useOutput('adapter', port);

  return (
    <div className="cd-node cd-wsadapter">
      <RadioTower lit={url !== null} />
      <div className="cd-title">WebSocket adapter</div>
      <div className="cd-meta">
        {url === null
          ? 'No server URL connected'
          : `BrowserWebSocketClientAdapter → ${url}`}
      </div>
      {url !== null && <div className="cd-meta">instances handed to a repo: {taken}</div>}
      <Handle type="target" position={Position.Top} id="server" title="server" />
      <Handle type="source" position={Position.Bottom} id="adapter" title="adapter" />
    </div>
  );
}
