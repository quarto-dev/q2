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
import FileExplorerNode from './canvas/FileExplorerNode';
import MonacoNode from './canvas/MonacoNode';
import PresenceNode from './canvas/PresenceNode';
import LspNode from './canvas/LspNode';
import RenderNode from './canvas/RenderNode';
import ServerNode from './canvas/ServerNode';
import WebSocketAdapterNode from './canvas/WebSocketAdapterNode';
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
  explorer: FileExplorerNode,
  monaco: MonacoNode,
  presence: PresenceNode,
  lsp: LspNode,
  render: RenderNode,
  server: ServerNode,
  wsadapter: WebSocketAdapterNode,
  hubproject: HubProjectNode,
  wasm: WasmNode,
  user: UserNode,
  language: LanguageNode,
  rawpreview: RawPreviewNode,
};

// Laid out left to right by dependency: sources on the left, the editor
// in the middle, the nodes that attach to it on the right. Every edge runs
// rightwards (or straight down) between neighbouring columns, so none
// passes behind a node.
const INITIAL_NODES: Node[] = [
  // column A: what everything starts from
  { id: 'server', type: 'server', position: { x: -1400, y: -120 }, data: {}, deletable: false },
  { id: 'ls', type: 'localstorage', position: { x: -1400, y: 300 }, data: {}, deletable: false },
  // column B: the adapters
  { id: 'ws-doc', type: 'wsadapter', position: { x: -1050, y: -420 }, data: {}, deletable: false },
  { id: 'ws-hub', type: 'wsadapter', position: { x: -1050, y: -120 }, data: {}, deletable: false },
  { id: 'idb', type: 'indexeddb', position: { x: -1050, y: 460 }, data: {}, deletable: false },
  // column C: the project connection
  { id: 'hub', type: 'hubproject', position: { x: -700, y: 40 }, data: {}, deletable: false },
  // column D: what the project yields
  { id: 'explorer', type: 'explorer', position: { x: -340, y: -520 }, data: {}, deletable: false, style: { width: 260, height: 240 } },
  { id: 'doc', type: 'automerge', position: { x: -340, y: -220 }, data: {}, deletable: false },
  { id: 'wasm', type: 'wasm', position: { x: -340, y: 560 }, data: {}, deletable: false },
  // column E: views of the document
  { id: 'view', type: 'rawview', position: { x: 60, y: -520 }, data: {}, deletable: false },
  { id: 'editor', type: 'monaco', position: { x: 60, y: -60 }, data: {}, deletable: false, style: { width: 460, height: 300 } },
  { id: 'rawpreview', type: 'rawpreview', position: { x: 60, y: 600 }, data: {}, deletable: false, style: { width: 460, height: 380 } },
  // column F: what attaches to the editor
  { id: 'user', type: 'user', position: { x: 620, y: -640 }, data: {}, deletable: false },
  { id: 'presence', type: 'presence', position: { x: 620, y: -480 }, data: {}, deletable: false },
  { id: 'language', type: 'language', position: { x: 620, y: -260 }, data: {}, deletable: false },
  { id: 'lsp', type: 'lsp', position: { x: 620, y: 260 }, data: {}, deletable: false },
  { id: 'render', type: 'render', position: { x: 620, y: 420 }, data: {}, deletable: false },
];

const INITIAL_EDGES: Edge[] = [
  { id: 'server-ws-doc', source: 'server', sourceHandle: 'server', target: 'ws-doc', targetHandle: 'server' },
  { id: 'server-ws-hub', source: 'server', sourceHandle: 'server', target: 'ws-hub', targetHandle: 'server' },
  { id: 'ws-doc-doc', source: 'ws-doc', sourceHandle: 'adapter', target: 'doc', targetHandle: 'adapter' },
  { id: 'ws-hub-hub', source: 'ws-hub', sourceHandle: 'adapter', target: 'hub', targetHandle: 'adapter' },
  { id: 'ls-hub', source: 'ls', sourceHandle: 'projectid', target: 'hub', targetHandle: 'projectid' },
  { id: 'idb-hub', source: 'idb', sourceHandle: 'idb', target: 'hub', targetHandle: 'idb' },
  { id: 'idb-doc', source: 'idb', sourceHandle: 'idb', target: 'doc', targetHandle: 'idb' },
  { id: 'hub-explorer', source: 'hub', sourceHandle: 'project', target: 'explorer', targetHandle: 'project' },
  { id: 'hub-editor', source: 'hub', sourceHandle: 'project', target: 'editor', targetHandle: 'project' },
  { id: 'hub-wasm', source: 'hub', sourceHandle: 'project', target: 'wasm', targetHandle: 'project' },
  { id: 'hub-presence', source: 'hub', sourceHandle: 'project', target: 'presence', targetHandle: 'project' },
  { id: 'explorer-doc', source: 'explorer', sourceHandle: 'docid', target: 'doc', targetHandle: 'docid' },
  { id: 'doc-view', source: 'doc', sourceHandle: 'doc', target: 'view', targetHandle: 'doc' },
  { id: 'doc-editor', source: 'doc', sourceHandle: 'doc', target: 'editor', targetHandle: 'doc' },
  { id: 'doc-rawpreview', source: 'doc', sourceHandle: 'doc', target: 'rawpreview', targetHandle: 'doc' },
  { id: 'wasm-lsp', source: 'wasm', sourceHandle: 'wasm', target: 'lsp', targetHandle: 'wasm' },
  { id: 'wasm-render', source: 'wasm', sourceHandle: 'wasm', target: 'render', targetHandle: 'wasm' },
  { id: 'wasm-rawpreview', source: 'wasm', sourceHandle: 'wasm', target: 'rawpreview', targetHandle: 'wasm' },
  { id: 'user-presence', source: 'user', sourceHandle: 'identity', target: 'presence', targetHandle: 'identity' },
  { id: 'language-editor', source: 'language', sourceHandle: 'language', target: 'editor', targetHandle: 'language' },
  { id: 'editor-presence', source: 'editor', sourceHandle: 'editor', target: 'presence', targetHandle: 'editor' },
  { id: 'editor-lsp', source: 'editor', sourceHandle: 'editor', target: 'lsp', targetHandle: 'editor' },
  { id: 'editor-render', source: 'editor', sourceHandle: 'editor', target: 'render', targetHandle: 'editor' },
];

const FIT_VIEW = { padding: 0.1 };
const PRO_OPTIONS = { hideAttribution: true };

function Canvas() {
  const { getEdges, setEdges } = useReactFlow();
  // False while an edge end is being dragged and has not landed on a handle.
  const reconnected = useRef(true);

  // Ports only pair with their namesakes, and an input takes one edge
  // (an output may fan out, e.g. one doc into both the raw view and the
  // editor) — except an adapter, which serves exactly one repo.
  const isValidConnection = useCallback((c: Connection | Edge) =>
    c.sourceHandle === c.targetHandle
      && !getEdges().some((e) => e.target === c.target && e.targetHandle === c.targetHandle && e.source !== c.source)
      && !(c.sourceHandle === 'adapter'
        && getEdges().some((e) => e.source === c.source && e.sourceHandle === 'adapter' && e.target !== c.target)),
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
