/**
 * The hub-client document editor (`DocumentEditor`, the Monaco editor
 * `Editor.tsx` uses) bound to the Automerge document wired into its "doc"
 * port: the text comes from that document, edits are spliced into it, and
 * its changes (from storage, from the network, from the raw view) are
 * diffed into the editor. Nothing here touches the hub project's own
 * copy of the file — if the Automerge node is offline, so are the edits.
 *
 * The file name wired into "path" is what the document is edited as.
 * Without one it is edited under `<id>.qmd`. The text, under that name,
 * goes out on the "file" port for the virtual filesystem; the live Monaco
 * instance, with that name, goes out on "editor" for the nodes that attach
 * behaviour to it. The language and theme come from the language node
 * wired into "language"; without it the file opens as plain text.
 *
 * The editor's built-in presence, intelligence and diagnostics are off:
 * those are nodes of their own. Diagnostics arrive on "diagnostics" (from
 * the preview, as in the app) and are underlined when they are for the
 * file being edited.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Handle, NodeResizer, Position } from '@xyflow/react';
import type * as Monaco from 'monaco-editor';
import { next as A } from '@automerge/automerge';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import type { TextDocumentContent } from '@quarto/quarto-automerge-schema';
import DocumentEditor, { type DocumentSource } from '../../DocumentEditor';
import { diagnosticsToMarkers } from '../../../utils/diagnosticToMonaco';
import { useInput, useOutput, type DiagnosticsPort, type DocPort, type EditorPort, type FilePort, type LanguagePort } from './ports';

const bareId = (id: string) => id.replace(/^automerge:/, '');

// Always a markers prop, so the editor never runs its own render diagnostics.
const NO_MARKERS: Monaco.editor.IMarkerData[] = [];

/** A DocumentSource over the doc port: stable identity, latest doc through a ref. */
function useDocSource(port: DocPort<TextDocumentContent>): DocumentSource {
  const portRef = useRef(port);
  const listeners = useRef(new Set<(text: string) => void>());
  // Every new doc version from the port is a change made elsewhere (the
  // editor's own edits come back too, as the same text, which the sync
  // ignores).
  useEffect(() => {
    portRef.current = port;
    for (const listener of listeners.current) listener(port.doc.text);
  }, [port]);
  return useMemo<DocumentSource>(() => ({
    getText: () => portRef.current.doc.text,
    subscribe: (listener) => {
      listeners.current.add(listener);
      return () => { listeners.current.delete(listener); };
    },
    applyChanges: (changes) => {
      portRef.current.change((d) => {
        for (const c of changes) A.splice(d, ['text'], c.rangeOffset, c.rangeLength, c.text);
      });
    },
  }), []);
}

interface BoundEditorProps {
  port: DocPort<TextDocumentContent>;
  file: FileEntry;
  language: LanguagePort | null;
  diagnostics: DiagnosticsPort | null;
  onEditor: (editor: EditorPort | undefined) => void;
}

function BoundEditor({ port, file, language, diagnostics, onEditor }: BoundEditorProps) {
  const source = useDocSource(port);
  // Only diagnostics reported for this file; none wired → no underlines.
  const markers = useMemo(
    () => (diagnostics !== null && diagnostics.path === file.path ? diagnosticsToMarkers(diagnostics.diagnostics).markers : NO_MARKERS),
    [diagnostics, file.path],
  );
  // The reconciliation trigger: a new map whenever the doc's text changes.
  const fileContents = useMemo(() => new Map([[file.path, port.doc.text]]), [file.path, port.doc.text]);
  const onMount = useCallback(
    (editor: Monaco.editor.IStandaloneCodeEditor, monaco: typeof Monaco) => onEditor({ editor, monaco, path: file.path }),
    [onEditor, file.path],
  );
  return (
    <DocumentEditor
      file={file}
      fileContents={fileContents}
      source={source}
      theme={language?.theme ?? 'none'}
      languageId={language?.languageId ?? 'plaintext'}
      markers={markers}
      presenceEnabled={false}
      intelligenceEnabled={false}
      onMount={onMount}
    />
  );
}

export default function MonacoNode() {
  const port = useInput<DocPort<TextDocumentContent>>('doc');
  const pathInput = useInput<string>('path');
  const language = useInput<LanguagePort>('language');
  const diagnostics = useInput<DiagnosticsPort>('diagnostics');
  // The live Monaco instance; republished whenever the editor is recreated.
  const [editorPort, setEditorPort] = useState<EditorPort | undefined>(undefined);
  useOutput('editor', port === null ? undefined : editorPort);

  // The file the document is edited as: the wired-in name, else a stand-in
  // so the VFS, LSP and render have a path.
  const path = port === null ? null : (pathInput ?? `${bareId(port.id)}.qmd`);
  const file = useMemo<FileEntry | null>(
    () => (port === null || path === null ? null : { path, docId: port.id }),
    [port, path],
  );
  // The text under its name, for the VFS.
  const text = port?.doc.text;
  const filePort = useMemo<FilePort | undefined>(
    () => (path === null || text === undefined ? undefined : { path, text }),
    [path, text],
  );
  useOutput('file', filePort);

  return (
    <div className="cd-node cd-monaco">
      <NodeResizer minWidth={320} minHeight={160} />
      <div className="cd-title">Document editor</div>
      {/* nokey: Monaco takes keys through an EditContext element React Flow
          would not recognise as an input, so Space (pan) and Backspace
          (delete selection) would be swallowed before the editor saw them. */}
      <div className="cd-editor nodrag nowheel nokey">
        {port === null || file === null
          ? <div className="cd-empty">No doc connected to edit</div>
          : <BoundEditor port={port} file={file} language={language} diagnostics={diagnostics} onEditor={setEditorPort} />}
      </div>
      <Handle type="target" position={Position.Left} id="doc" title="doc" style={{ top: '25%' }} />
      <Handle type="target" position={Position.Left} id="path" title="path" style={{ top: '50%' }} />
      <Handle type="source" position={Position.Left} id="editor" title="editor" style={{ top: '75%' }} />
      <Handle type="target" position={Position.Right} id="language" title="language" style={{ top: '35%' }} />
      <Handle type="target" position={Position.Right} id="diagnostics" title="diagnostics" style={{ top: '65%' }} />
      <Handle type="source" position={Position.Bottom} id="file" title="file" />
    </div>
  );
}
