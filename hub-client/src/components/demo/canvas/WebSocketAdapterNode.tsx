/**
 * An Automerge websocket network adapter for the server wired into
 * "server": the piece of a repo that speaks the sync protocol over one
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
import { useInput, useOutput, type AdapterPort, type ServerPort } from './ports';

export default function WebSocketAdapterNode() {
  const server = useInput<ServerPort>('server');
  const nodeId = useNodeId() ?? 'adapter';
  const [taken, setTaken] = useState(0);

  const port = useMemo<AdapterPort | undefined>(() => {
    if (server === null) return undefined;
    const url = server.url;
    return {
      id: `${nodeId}:${url}`,
      url,
      take: (): NetworkAdapter => {
        setTaken((n) => n + 1);
        return new StoppableWebSocketClientAdapter(url) as unknown as NetworkAdapter;
      },
    };
  }, [server, nodeId]);
  useOutput('adapter', port);

  return (
    <div className="cd-node cd-wsadapter">
      <RadioTower lit={server !== null} />
      <div className="cd-title">WebSocket adapter</div>
      <div className="cd-meta">
        {server === null
          ? 'No server connected'
          : `BrowserWebSocketClientAdapter → ${server.url}`}
      </div>
      {server !== null && <div className="cd-meta">instances handed to a repo: {taken}</div>}
      <Handle type="target" position={Position.Top} id="server" title="server" />
      <Handle type="source" position={Position.Bottom} id="adapter" title="adapter" />
    </div>
  );
}
