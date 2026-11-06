/**
 * The hub-client document editor (`DocumentEditor`, the Monaco editor
 * `Editor.tsx` uses) on the document wired into its "doc" port.
 *
 * `DocumentEditor` is bound to the preview runtime's copy of the project —
 * the hub project wired into "project" — so the doc port's id is matched
 * to one of its files and that file is what the editor opens. The canvas's
 * own repo (the Automerge node) and the runtime's converge through the
 * hub, so edits here show up on the port and edits on the port show up
 * here. The theme comes from the language wired into "language".
 *
 * The editor's built-in presence, intelligence and diagnostics are off:
 * those are nodes of their own, wired to the Monaco instance this node
 * publishes on its "editor" port. Whatever arrives on the "diagnostics"
 * port for the open file is underlined.
 */

import { useCallback, useMemo, useState } from 'react';
import { Handle, NodeResizer, Position } from '@xyflow/react';
import type * as Monaco from 'monaco-editor';
import { applyEditorOperations } from '@quarto/preview-runtime';
import type { TextDocumentContent } from '@quarto/quarto-automerge-schema';
import DocumentEditor from '../../DocumentEditor';
import { diagnosticsToMarkers } from '../../../utils/diagnosticToMonaco';
import { useHubProject } from '../hubProject';
import { useInput, useOutput, type DiagnosticsPort, type DocPort, type EditorPort, type LanguagePort, type ProjectPort } from './ports';

const bareId = (id: string) => id.replace(/^automerge:/, '');

const NO_MARKERS: Monaco.editor.IMarkerData[] = [];

interface FileEditorProps {
  project: ProjectPort;
  path: string;
  theme: LanguagePort['theme'];
  diagnostics: DiagnosticsPort | null;
  onEditor: (port: EditorPort | undefined) => void;
}

/** The editor on the project file at `path`, with its content kept current. */
function FileEditor({ project, path, theme, diagnostics, onEditor }: FileEditorProps) {
  const { fileContents, currentFile, error } = useHubProject(path, project.url);
  const onMount = useCallback(
    (editor: Monaco.editor.IStandaloneCodeEditor, monaco: typeof Monaco) => onEditor({ editor, monaco, path }),
    [onEditor, path],
  );
  // Only diagnostics reported for this file; none wired → no underlines.
  const markers = useMemo(
    () => (diagnostics !== null && diagnostics.path === path ? diagnosticsToMarkers(diagnostics.diagnostics).markers : NO_MARKERS),
    [diagnostics, path],
  );
  if (error !== null) return <div className="cd-empty">{error}</div>;
  if (currentFile === null) return <div className="cd-empty">connecting…</div>;
  return (
    <DocumentEditor
      file={currentFile}
      fileContents={fileContents}
      onContentOperations={applyEditorOperations}
      theme={theme}
      markers={markers}
      presenceEnabled={false}
      intelligenceEnabled={false}
      onMount={onMount}
    />
  );
}

interface PortEditorProps {
  project: ProjectPort;
  docId: string;
  theme: LanguagePort['theme'];
  diagnostics: DiagnosticsPort | null;
  onEditor: (port: EditorPort | undefined) => void;
}

/** Resolve the doc port's id to a project file, then edit it. */
function PortEditor({ project, docId, theme, diagnostics, onEditor }: PortEditorProps) {
  const file = project.files.find((f) => bareId(f.docId) === bareId(docId));
  if (file === undefined) return <div className="cd-empty">{bareId(docId)} is not a file of the hub project</div>;
  return <FileEditor project={project} path={file.path} theme={theme} diagnostics={diagnostics} onEditor={onEditor} />;
}

export default function MonacoNode() {
  const port = useInput<DocPort<TextDocumentContent>>('doc');
  const project = useInput<ProjectPort>('project');
  const language = useInput<LanguagePort>('language');
  const diagnostics = useInput<DiagnosticsPort>('diagnostics');
  // The live Monaco instance; republished whenever the editor is recreated.
  const [editorPort, setEditorPort] = useState<EditorPort | undefined>(undefined);
  useOutput('editor', port === null || project === null ? undefined : editorPort);
  return (
    <div className="cd-node cd-monaco">
      <NodeResizer minWidth={320} minHeight={160} />
      <div className="cd-title">Document editor</div>
      <div className="cd-editor nodrag nowheel">
        {port === null
          ? <div className="cd-empty">No doc connected to edit</div>
          : project === null
            ? <div className="cd-empty">No hub project connected to open it in</div>
            : <PortEditor project={project} docId={port.id} theme={language?.theme ?? 'light'} diagnostics={diagnostics} onEditor={setEditorPort} />}
      </div>
      <Handle type="target" position={Position.Left} id="doc" title="doc" style={{ top: '35%' }} />
      <Handle type="target" position={Position.Left} id="project" title="project" style={{ top: '65%' }} />
      <Handle type="target" position={Position.Top} id="language" title="language" />
      <Handle type="source" position={Position.Right} id="editor" title="editor" />
      <Handle type="target" position={Position.Bottom} id="diagnostics" title="diagnostics" />
    </div>
  );
}
