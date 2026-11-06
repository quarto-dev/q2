/**
 * The hub-client document editor (`DocumentEditor`, the Monaco editor
 * `Editor.tsx` uses) bound to the Automerge document wired into its "doc"
 * port: the text comes from that document, edits are spliced into it, and
 * its changes (from storage, from the network, from the raw view) are
 * diffed into the editor. Nothing here touches the hub project's own
 * copy of the file — if the Automerge node is offline, so are the edits.
 *
 * The hub project wired into "project" only names the file (its path, for
 * the VFS the LSP and render read, and for presence); without it the
 * document is edited under `<id>.qmd`. The language and theme come from
 * the language node wired into "language"; without it the file opens as
 * plain text.
 *
 * The editor's built-in presence, intelligence and diagnostics are off:
 * those are nodes of their own, wired to the Monaco instance this node
 * publishes on its "editor" port.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Handle, NodeResizer, Position } from '@xyflow/react';
import type * as Monaco from 'monaco-editor';
import { next as A } from '@automerge/automerge';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import type { TextDocumentContent } from '@quarto/quarto-automerge-schema';
import DocumentEditor, { type DocumentSource } from '../../DocumentEditor';
import { useInput, useOutput, type DocPort, type EditorPort, type LanguagePort, type ProjectPort } from './ports';

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
  onEditor: (editor: EditorPort | undefined) => void;
}

function BoundEditor({ port, file, language, onEditor }: BoundEditorProps) {
  const source = useDocSource(port);
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
      markers={NO_MARKERS}
      presenceEnabled={false}
      intelligenceEnabled={false}
      onMount={onMount}
    />
  );
}

export default function MonacoNode() {
  const port = useInput<DocPort<TextDocumentContent>>('doc');
  const project = useInput<ProjectPort>('project');
  const language = useInput<LanguagePort>('language');
  // The live Monaco instance; republished whenever the editor is recreated.
  const [editorPort, setEditorPort] = useState<EditorPort | undefined>(undefined);
  useOutput('editor', port === null ? undefined : editorPort);

  // The file the document is: the project's entry for this id, else a
  // stand-in name so the VFS, LSP and render have a path.
  const file = useMemo<FileEntry | null>(() => {
    if (port === null) return null;
    const known = project?.files.find((f) => bareId(f.docId) === bareId(port.id));
    return known ?? { path: `${bareId(port.id)}.qmd`, docId: port.id };
  }, [port, project]);

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
          : <BoundEditor port={port} file={file} language={language} onEditor={setEditorPort} />}
      </div>
      <Handle type="target" position={Position.Left} id="doc" title="doc" style={{ top: '35%' }} />
      <Handle type="target" position={Position.Left} id="project" title="project" style={{ top: '65%' }} />
      <Handle type="source" position={Position.Right} id="editor" title="editor" style={{ top: '35%' }} />
      <Handle type="target" position={Position.Right} id="language" title="language" style={{ top: '65%' }} />
    </div>
  );
}
