/**
 * Ports: how canvas nodes depend on each other without knowing each other.
 *
 * A node publishes a value on one of its output handles with `useOutput`
 * and reads whatever is wired into an input handle with `useInput`. The
 * edge is the only coupling: `useInput` follows the React Flow connection
 * targeting the handle and looks up the source node's published value.
 * Nothing is wired → `useInput` returns null. The registry lives in
 * PortsProvider.tsx.
 */

import { createContext, useContext, useEffect } from 'react';
import { useNodeConnections, useNodeId } from '@xyflow/react';
import type { Doc } from '@automerge/automerge';

export interface Ports {
  values: Map<string, unknown>;
  publish: (key: string, value: unknown) => void;
}

export const PortsContext = createContext<Ports | null>(null);

function usePorts(): Ports {
  const ports = useContext(PortsContext);
  if (ports === null) throw new Error('ports hooks must be used inside <PortsProvider>');
  return ports;
}

/** Publish `value` on this node's output handle `handleId`. */
export function useOutput(handleId: string, value: unknown): void {
  const nodeId = useNodeId();
  const { publish } = usePorts();
  const key = `${nodeId}:${handleId}`;
  useEffect(() => {
    publish(key, value);
  }, [key, value, publish]);
  useEffect(() => () => publish(key, undefined), [key, publish]);
}

/** The value wired into this node's input handle `handleId`, or null. */
export function useInput<T>(handleId: string): T | null {
  const connections = useNodeConnections({ handleType: 'target', handleId });
  const { values } = usePorts();
  const connection = connections[0];
  if (connection === undefined) return null;
  const value = values.get(`${connection.source}:${connection.sourceHandle}`) as T | undefined;
  return value ?? null;
}

/** What an Automerge node offers on its "doc" port. */
export interface DocPort<T = unknown> {
  /** The document's id, as the hub's index doc would record it. */
  id: string;
  doc: Doc<T>;
  change(fn: (d: T) => void): void;
}

/** What a local storage node offers on its "docid" port. */
export interface IdStore {
  /** The remembered doc id, or null when nothing has been stored yet. */
  id: string | null;
  set(id: string): void;
}

/** What an IndexedDB node offers on its "storage" port; keyed by doc id. */
export interface DocStore {
  load(id: string): Promise<Uint8Array | undefined>;
  save(id: string, bytes: Uint8Array): Promise<void>;
}
