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
import type * as Monaco from 'monaco-editor';
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';
import type { FileEntry } from '@quarto/preview-renderer/types/project';

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

/**
 * What the document editor node offers on its "editor" port: the live
 * Monaco instance (recreated on every file switch) and the project path of
 * the file it shows, for nodes that attach behaviour to the editor.
 */
export interface EditorPort {
  editor: Monaco.editor.IStandaloneCodeEditor;
  monaco: typeof Monaco;
  path: string;
}

/**
 * What the render node offers on its "diagnostics" port: what the render
 * pipeline reported for the file at `path`, for the editor to underline.
 */
export interface DiagnosticsPort {
  path: string;
  diagnostics: Diagnostic[];
}

/** What the server node offers on its "server" port: where the hub is. */
export interface ServerPort {
  url: string;
}

/**
 * What the hub project node offers on its "project" port: the preview
 * runtime's connection to the project — its files, and the server and
 * index doc they came from. The runtime keeps one such connection.
 */
export interface ProjectPort {
  url: string;
  indexDocId: string;
  files: FileEntry[];
}

/** What the WASM node offers on its "wasm" port: the module and its VFS. */
export interface WasmPort {
  ready: boolean;
  /** Paths in the virtual filesystem the LSP and render read from. */
  files: string[];
}

/** What the user node offers on its "identity" port: who presence says we are. */
export interface IdentityPort {
  name: string;
  color: string;
}

/** What the language node offers on its "language" port: how qmd is shown. */
export interface LanguagePort {
  theme: 'light' | 'dark';
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
