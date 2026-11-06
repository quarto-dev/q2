/**
 * #/demo/canvas — standalone nodes on an infinite canvas, wired together
 * with edges. Each node is its own small demo; dependencies live only in
 * the edges (see canvas/ports.ts). Drag an edge end off a handle to
 * disconnect it, or select an edge and press Backspace; drag between
 * matching handles to reconnect.
 *
 * The flow is uncontrolled: React Flow owns node positions and edges, so
 * dragging never re-renders this component.
 */

import { useCallback, useRef } from 'react';
import {
  Background,
  Controls,
  ReactFlow,
  ReactFlowProvider,
  addEdge,
  reconnectEdge,
  useReactFlow,
  type Connection,
  type Edge,
  type Node,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import DemoFrame from './DemoFrame';
import PortsProvider from './canvas/PortsProvider';
import AutomergeNode from './canvas/AutomergeNode';
import RawViewNode from './canvas/RawViewNode';
import IndexedDbNode from './canvas/IndexedDbNode';
import LocalStorageNode from './canvas/LocalStorageNode';
import MonacoNode from './canvas/MonacoNode';
import PresenceNode from './canvas/PresenceNode';
import LspNode from './canvas/LspNode';
import RenderNode from './canvas/RenderNode';
import ServerNode from './canvas/ServerNode';
import HubProjectNode from './canvas/HubProjectNode';
import WasmNode from './canvas/WasmNode';
import UserNode from './canvas/UserNode';
import LanguageNode from './canvas/LanguageNode';
import RawPreviewNode from './canvas/RawPreviewNode';
import './CanvasDemo.css';

const NODE_TYPES = {
  automerge: AutomergeNode,
  rawview: RawViewNode,
  indexeddb: IndexedDbNode,
  localstorage: LocalStorageNode,
  monaco: MonacoNode,
  presence: PresenceNode,
  lsp: LspNode,
  render: RenderNode,
  server: ServerNode,
  hubproject: HubProjectNode,
  wasm: WasmNode,
  user: UserNode,
  language: LanguageNode,
  rawpreview: RawPreviewNode,
};

const INITIAL_NODES: Node[] = [
  { id: 'server', type: 'server', position: { x: 0, y: -200 }, data: {}, deletable: false },
  { id: 'doc', type: 'automerge', position: { x: 0, y: 0 }, data: {}, deletable: false },
  { id: 'view', type: 'rawview', position: { x: 340, y: -40 }, data: {}, deletable: false },
  { id: 'idb', type: 'indexeddb', position: { x: 60, y: 220 }, data: {}, deletable: false },
  { id: 'ls', type: 'localstorage', position: { x: -320, y: 20 }, data: {}, deletable: false },
  { id: 'editor', type: 'monaco', position: { x: 340, y: 200 }, data: {}, deletable: false, style: { width: 460, height: 300 } },
  { id: 'presence', type: 'presence', position: { x: 880, y: 200 }, data: {}, deletable: false },
  { id: 'lsp', type: 'lsp', position: { x: 880, y: 340 }, data: {}, deletable: false },
  { id: 'render', type: 'render', position: { x: 880, y: 500 }, data: {}, deletable: false },
  { id: 'hub', type: 'hubproject', position: { x: -320, y: 420 }, data: {}, deletable: false },
  { id: 'wasm', type: 'wasm', position: { x: 60, y: 600 }, data: {}, deletable: false },
  { id: 'user', type: 'user', position: { x: 880, y: 40 }, data: {}, deletable: false },
  { id: 'language', type: 'language', position: { x: 880, y: -140 }, data: {}, deletable: false },
  { id: 'rawpreview', type: 'rawpreview', position: { x: 340, y: -460 }, data: {}, deletable: false, style: { width: 460, height: 380 } },
];

const INITIAL_EDGES: Edge[] = [
  { id: 'doc-view', source: 'doc', sourceHandle: 'doc', target: 'view', targetHandle: 'doc' },
  { id: 'idb-doc', source: 'idb', sourceHandle: 'storage', target: 'doc', targetHandle: 'storage' },
  { id: 'ls-doc', source: 'ls', sourceHandle: 'docid', target: 'doc', targetHandle: 'docid' },
  { id: 'doc-editor', source: 'doc', sourceHandle: 'doc', target: 'editor', targetHandle: 'doc' },
  { id: 'editor-presence', source: 'editor', sourceHandle: 'editor', target: 'presence', targetHandle: 'editor' },
  { id: 'editor-lsp', source: 'editor', sourceHandle: 'editor', target: 'lsp', targetHandle: 'editor' },
  { id: 'editor-render', source: 'editor', sourceHandle: 'editor', target: 'render', targetHandle: 'editor' },
  { id: 'render-editor', source: 'render', sourceHandle: 'diagnostics', target: 'editor', targetHandle: 'diagnostics' },
  { id: 'server-doc', source: 'server', sourceHandle: 'server', target: 'doc', targetHandle: 'server' },
  { id: 'server-hub', source: 'server', sourceHandle: 'server', target: 'hub', targetHandle: 'server' },
  { id: 'hub-editor', source: 'hub', sourceHandle: 'project', target: 'editor', targetHandle: 'project' },
  { id: 'hub-wasm', source: 'hub', sourceHandle: 'project', target: 'wasm', targetHandle: 'project' },
  { id: 'wasm-lsp', source: 'wasm', sourceHandle: 'wasm', target: 'lsp', targetHandle: 'wasm' },
  { id: 'wasm-render', source: 'wasm', sourceHandle: 'wasm', target: 'render', targetHandle: 'wasm' },
  { id: 'user-presence', source: 'user', sourceHandle: 'identity', target: 'presence', targetHandle: 'identity' },
  { id: 'language-editor', source: 'language', sourceHandle: 'language', target: 'editor', targetHandle: 'language' },
  { id: 'doc-rawpreview', source: 'doc', sourceHandle: 'doc', target: 'rawpreview', targetHandle: 'doc' },
  { id: 'wasm-rawpreview', source: 'wasm', sourceHandle: 'wasm', target: 'rawpreview', targetHandle: 'wasm' },
];

const FIT_VIEW = { padding: 0.15 };
const PRO_OPTIONS = { hideAttribution: true };

function Canvas() {
  const { getEdges, setEdges } = useReactFlow();
  // False while an edge end is being dragged and has not landed on a handle.
  const reconnected = useRef(true);

  // Ports only pair with their namesakes, and an input takes one edge
  // (an output may fan out, e.g. one doc into both the raw view and the editor).
  const isValidConnection = useCallback((c: Connection | Edge) =>
    c.sourceHandle === c.targetHandle
      && !getEdges().some((e) => e.target === c.target && e.targetHandle === c.targetHandle && e.source !== c.source),
  [getEdges]);

  const onConnect = useCallback((c: Connection) => setEdges((eds) => addEdge(c, eds)), [setEdges]);
  const onReconnectStart = useCallback(() => { reconnected.current = false; }, []);
  const onReconnect = useCallback((old: Edge, c: Connection) => {
    reconnected.current = true;
    setEdges((eds) => reconnectEdge(old, c, eds));
  }, [setEdges]);
  // Dropped in empty space: the edge is gone.
  const onReconnectEnd = useCallback((_: unknown, edge: Edge) => {
    if (!reconnected.current) setEdges((eds) => eds.filter((e) => e.id !== edge.id));
    reconnected.current = true;
  }, [setEdges]);

  return (
    <ReactFlow
      className="cd-flow"
      defaultNodes={INITIAL_NODES}
      defaultEdges={INITIAL_EDGES}
      nodeTypes={NODE_TYPES}
      onConnect={onConnect}
      onReconnect={onReconnect}
      onReconnectStart={onReconnectStart}
      onReconnectEnd={onReconnectEnd}
      isValidConnection={isValidConnection}
      edgesReconnectable
      fitView
      fitViewOptions={FIT_VIEW}
      proOptions={PRO_OPTIONS}
    >
      <Background />
      <Controls />
    </ReactFlow>
  );
}

export default function CanvasDemo() {
  return (
    <DemoFrame info="drag an edge end off a handle to disconnect">
      <ReactFlowProvider>
        <PortsProvider>
          <Canvas />
        </PortsProvider>
      </ReactFlowProvider>
    </DemoFrame>
  );
}
