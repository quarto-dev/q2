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
import { STRING_PORTS } from './canvas/ports';
import { SYNC_SERVER_URL } from './hubProject';
import AutomergeNode from './canvas/AutomergeNode';
import RawViewNode from './canvas/RawViewNode';
import IndexedDbNode from './canvas/IndexedDbNode';
import LocalStorageNode from './canvas/LocalStorageNode';
import FileExplorerNode from './canvas/FileExplorerNode';
import MonacoNode from './canvas/MonacoNode';
import PresenceNode from './canvas/PresenceNode';
import LspNode from './canvas/LspNode';
import PreviewNode from './canvas/PreviewNode';
import StringNode from './canvas/StringNode';
import WebSocketAdapterNode from './canvas/WebSocketAdapterNode';
import HubProjectNode from './canvas/HubProjectNode';
import WasmNode from './canvas/WasmNode';
import VfsNode from './canvas/VfsNode';
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
  preview: PreviewNode,
  string: StringNode,
  wsadapter: WebSocketAdapterNode,
  hubproject: HubProjectNode,
  wasm: WasmNode,
  vfs: VfsNode,
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
  { id: 'server', type: 'string', position: { x: -1500, y: -120 }, data: { label: 'Server URL', value: SYNC_SERVER_URL }, deletable: false },
  { id: 'ls', type: 'localstorage', position: { x: -1500, y: 300 }, data: {}, deletable: false },
  // column B: the adapters
  { id: 'ws-hub', type: 'wsadapter', position: { x: -1150, y: -120 }, data: {}, deletable: false },
  { id: 'idb', type: 'indexeddb', position: { x: -1150, y: 460 }, data: {}, deletable: false },
  { id: 'wasm', type: 'wasm', position: { x: -1150, y: 900 }, data: {}, deletable: false },
  // column C: the project connection
  { id: 'hub', type: 'hubproject', position: { x: -800, y: 40 }, data: {}, deletable: false },
  { id: 'vfs', type: 'vfs', position: { x: -800, y: 560 }, data: {}, deletable: false, style: { width: 300, height: 260 } },
  // column D: what sits between the project and the document / editor
  { id: 'explorer', type: 'explorer', position: { x: -460, y: -200 }, data: {}, deletable: false, style: { width: 260, height: 240 } },
  { id: 'filename', type: 'string', position: { x: -460, y: 80 }, data: { label: 'File name' }, deletable: false },
  { id: 'presence', type: 'presence', position: { x: -460, y: 220 }, data: {}, deletable: false },
  { id: 'user', type: 'user', position: { x: -460, y: 400 }, data: {}, deletable: false },
  // column E: the document
  { id: 'doc', type: 'automerge', position: { x: 140, y: -220 }, data: {}, deletable: false },
  // column F: views of the document
  { id: 'view', type: 'rawview', position: { x: 520, y: -560 }, data: {}, deletable: false },
  { id: 'editor', type: 'monaco', position: { x: 520, y: -60 }, data: {}, deletable: false, style: { width: 460, height: 300 } },
  { id: 'rawpreview', type: 'rawpreview', position: { x: 520, y: 600 }, data: {}, deletable: false, style: { width: 460, height: 380 } },
  // column G: what attaches to the editor
  { id: 'language', type: 'language', position: { x: 1080, y: -260 }, data: {}, deletable: false },
  { id: 'lsp', type: 'lsp', position: { x: 1080, y: -80 }, data: {}, deletable: false },
  { id: 'preview', type: 'preview', position: { x: 1080, y: 100 }, data: {}, deletable: false, style: { width: 460, height: 420 } },
];

const INITIAL_EDGES: Edge[] = [
  { id: 'server-ws-hub', source: 'server', sourceHandle: 'string', target: 'ws-hub', targetHandle: 'server' },
  { id: 'ws-hub-hub', source: 'ws-hub', sourceHandle: 'adapter', target: 'hub', targetHandle: 'adapter' },
  { id: 'ls-hub', source: 'ls', sourceHandle: 'projectid', target: 'hub', targetHandle: 'projectid' },
  { id: 'idb-hub', source: 'idb', sourceHandle: 'idb', target: 'hub', targetHandle: 'idb' },
  { id: 'hub-doc', source: 'hub', sourceHandle: 'project', target: 'doc', targetHandle: 'project' },
  { id: 'hub-explorer', source: 'hub', sourceHandle: 'project', target: 'explorer', targetHandle: 'project' },
  { id: 'hub-vfs', source: 'hub', sourceHandle: 'project', target: 'vfs', targetHandle: 'project' },
  { id: 'wasm-vfs', source: 'wasm', sourceHandle: 'wasm', target: 'vfs', targetHandle: 'wasm' },
  { id: 'hub-presence', source: 'hub', sourceHandle: 'project', target: 'presence', targetHandle: 'project' },
  { id: 'explorer-doc', source: 'explorer', sourceHandle: 'docid', target: 'doc', targetHandle: 'docid' },
  { id: 'explorer-filename', source: 'explorer', sourceHandle: 'path', target: 'filename', targetHandle: 'string' },
  { id: 'filename-editor', source: 'filename', sourceHandle: 'string', target: 'editor', targetHandle: 'path' },
  { id: 'filename-preview', source: 'filename', sourceHandle: 'string', target: 'preview', targetHandle: 'path' },
  { id: 'editor-vfs', source: 'editor', sourceHandle: 'file', target: 'vfs', targetHandle: 'file' },
  { id: 'doc-view', source: 'doc', sourceHandle: 'doc', target: 'view', targetHandle: 'doc' },
  { id: 'doc-editor', source: 'doc', sourceHandle: 'doc', target: 'editor', targetHandle: 'doc' },
  { id: 'doc-rawpreview', source: 'doc', sourceHandle: 'doc', target: 'rawpreview', targetHandle: 'doc' },
  { id: 'vfs-lsp', source: 'vfs', sourceHandle: 'vfs', target: 'lsp', targetHandle: 'vfs' },
  { id: 'vfs-preview', source: 'vfs', sourceHandle: 'vfs', target: 'preview', targetHandle: 'vfs' },
  { id: 'hub-preview', source: 'hub', sourceHandle: 'project', target: 'preview', targetHandle: 'project' },
  { id: 'wasm-rawpreview', source: 'wasm', sourceHandle: 'wasm', target: 'rawpreview', targetHandle: 'wasm' },
  { id: 'user-presence', source: 'user', sourceHandle: 'identity', target: 'presence', targetHandle: 'identity' },
  { id: 'language-editor', source: 'language', sourceHandle: 'language', target: 'editor', targetHandle: 'language' },
  { id: 'editor-presence', source: 'editor', sourceHandle: 'editor', target: 'presence', targetHandle: 'editor' },
  { id: 'editor-lsp', source: 'editor', sourceHandle: 'editor', target: 'lsp', targetHandle: 'editor' },
  { id: 'doc-preview', source: 'doc', sourceHandle: 'doc', target: 'preview', targetHandle: 'doc' },
  { id: 'preview-editor', source: 'preview', sourceHandle: 'diagnostics', target: 'editor', targetHandle: 'diagnostics' },
];

const FIT_VIEW = { padding: 0.1 };
const PRO_OPTIONS = { hideAttribution: true };

function Canvas() {
  const { getEdges, setEdges } = useReactFlow();
  // False while an edge end is being dragged and has not landed on a handle.
  const reconnected = useRef(true);

  // Ports only pair with their namesakes (string ports with any string
  // port), and an input takes one edge (an output may fan out, e.g. one
  // doc into both the raw view and the editor) — except an adapter, which
  // serves exactly one repo.
  const pairs = (a: string | null | undefined, b: string | null | undefined) =>
    a === b || (a != null && b != null && STRING_PORTS.has(a) && STRING_PORTS.has(b));
  const isValidConnection = useCallback((c: Connection | Edge) =>
    pairs(c.sourceHandle, c.targetHandle)
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
