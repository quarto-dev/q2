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
import './CanvasDemo.css';

const NODE_TYPES = { automerge: AutomergeNode, rawview: RawViewNode, indexeddb: IndexedDbNode, localstorage: LocalStorageNode };

const INITIAL_NODES: Node[] = [
  { id: 'doc', type: 'automerge', position: { x: 0, y: 0 }, data: {}, deletable: false },
  { id: 'view', type: 'rawview', position: { x: 340, y: -40 }, data: {}, deletable: false },
  { id: 'idb', type: 'indexeddb', position: { x: 60, y: 220 }, data: {}, deletable: false },
  { id: 'ls', type: 'localstorage', position: { x: -320, y: 20 }, data: {}, deletable: false },
];

const INITIAL_EDGES: Edge[] = [
  { id: 'doc-view', source: 'doc', sourceHandle: 'doc', target: 'view', targetHandle: 'doc' },
  { id: 'idb-doc', source: 'idb', sourceHandle: 'storage', target: 'doc', targetHandle: 'storage' },
  { id: 'ls-doc', source: 'ls', sourceHandle: 'docid', target: 'doc', targetHandle: 'docid' },
];

const FIT_VIEW = { padding: 0.3 };
const PRO_OPTIONS = { hideAttribution: true };

function Canvas() {
  const { getEdges, setEdges } = useReactFlow();
  // False while an edge end is being dragged and has not landed on a handle.
  const reconnected = useRef(true);

  // Ports only pair with their namesakes, and an input takes one edge.
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
