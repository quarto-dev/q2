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

import { useCallback, useRef, useState } from 'react';
import {
  Background,
  Controls,
  Panel,
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
import RepoNode from './canvas/RepoNode';
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
  repo: RepoNode,
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
// Three bands, grouped by family. Top-left: the sources and the Automerge
// nodes (yellow), left to right by dependency. Middle and right: the
// editor family (blue). Bottom: the WASM family (purple). Edges run
// rightwards or down between neighbours where they can.
const INITIAL_NODES: Node[] = [
  // sources
  { id: 'server', type: 'string', position: { x: -1488, y: -236 }, data: { label: 'Server URL', value: SYNC_SERVER_URL }, deletable: false },
  { id: 'ls', type: 'localstorage', position: { x: -1048, y: 52 }, data: {}, deletable: false },
  // Automerge: adapters → repo → project / document → raw view
  { id: 'ws-hub', type: 'wsadapter', position: { x: -1150, y: -300 }, data: {}, deletable: false },
  { id: 'idb', type: 'indexeddb', position: { x: -1150, y: -516 }, data: {}, deletable: false },
  { id: 'repo', type: 'repo', position: { x: -800, y: -520 }, data: {}, deletable: false },
  { id: 'hub', type: 'hubproject', position: { x: -696, y: 38 }, data: {}, deletable: false },
  { id: 'doc', type: 'automerge', position: { x: 172, y: -218 }, data: {}, deletable: false },
  { id: 'view', type: 'rawview', position: { x: 520, y: -560 }, data: {}, deletable: false },
  // editor family — presence sits between the project (left), the editor
  // (right) and the user (below it)
  { id: 'explorer', type: 'explorer', position: { x: -200, y: -280 }, data: {}, deletable: false, style: { width: 260, height: 220 } },
  { id: 'filename', type: 'string', position: { x: -200, y: -30 }, data: { label: 'File name' }, deletable: false },
  { id: 'presence', type: 'presence', position: { x: -66, y: 200 }, data: {}, deletable: false },
  { id: 'user', type: 'user', position: { x: -156, y: 260 }, data: {}, deletable: false },
  { id: 'editor', type: 'monaco', position: { x: 534, y: 242 }, data: {}, deletable: false, style: { width: 460, height: 300 } },
  { id: 'language', type: 'language', position: { x: 1080, y: -260 }, data: {}, deletable: false },
  { id: 'preview', type: 'preview', position: { x: 1080, y: -60 }, data: {}, deletable: false, style: { width: 460, height: 420 } },
  { id: 'rawpreview', type: 'rawpreview', position: { x: 1080, y: 390 }, data: {}, deletable: false, style: { width: 460, height: 380 } },
  // WASM family
  { id: 'wasm', type: 'wasm', position: { x: -754, y: 901 }, data: {}, deletable: false },
  { id: 'vfs', type: 'vfs', position: { x: -480, y: 560 }, data: {}, deletable: false, style: { width: 300, height: 260 } },
  { id: 'lsp', type: 'lsp', position: { x: -24, y: 368 }, data: {}, deletable: false },
];

const INITIAL_EDGES: Edge[] = [
  { id: 'server-ws-hub', source: 'server', sourceHandle: 'string', target: 'ws-hub', targetHandle: 'server' },
  { id: 'ws-hub-repo', source: 'ws-hub', sourceHandle: 'adapter', target: 'repo', targetHandle: 'adapter' },
  { id: 'idb-repo', source: 'idb', sourceHandle: 'idb', target: 'repo', targetHandle: 'idb' },
  { id: 'repo-hub', source: 'repo', sourceHandle: 'repo', target: 'hub', targetHandle: 'repo' },
  { id: 'repo-doc', source: 'repo', sourceHandle: 'repo', target: 'doc', targetHandle: 'repo' },
  { id: 'ls-hub', source: 'ls', sourceHandle: 'projectid', target: 'hub', targetHandle: 'projectid' },
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

/** The current layout as one line per node, for pasting back into INITIAL_NODES. */
function describeLayout(nodes: Node[]): string {
  return nodes
    .map((n) => {
      const size = n.measured?.width !== undefined && n.style?.width !== undefined
        ? `, size: { width: ${Math.round(Number(n.style.width))}, height: ${Math.round(Number(n.style.height))} }`
        : '';
      return `${n.id}: { x: ${Math.round(n.position.x)}, y: ${Math.round(n.position.y)}${size} }`;
    })
    .join('\n');
}

function Canvas() {
  const { getEdges, setEdges, getNodes } = useReactFlow();
  const [copied, setCopied] = useState(false);
  const copyLayout = useCallback(() => {
    const text = describeLayout(getNodes());
    console.log(`[canvas layout]\n${text}`);
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    });
  }, [getNodes]);
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
      <Panel position="top-right">
        <button type="button" className="cd-layout-btn" onClick={copyLayout} title="Copy every node's position (and size) to the clipboard and the console">
          {copied ? 'layout copied' : 'copy layout'}
        </button>
      </Panel>
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
