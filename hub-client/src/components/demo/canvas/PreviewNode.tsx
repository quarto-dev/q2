/**
 * The preview: the document wired into "doc", under the file name wired
 * into "path", rendered by the same PreviewRouter the app's preview pane
 * uses, from the virtual filesystem wired into "vfs" with the project
 * wired into "project" for
 * what the app's preview takes from it: the file list for links and the
 * contents for custom render components. Re-renders after each change to
 * the document. Block edits made
 * in the preview are spliced straight into the document. Whatever the
 * render reports (parse errors, links to files that don't exist,
 * unresolved cross-references, unknown shortcodes, ...) goes out on the
 * "diagnostics" port, for the editor to underline — which is where the
 * app's underlines come from.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Handle, NodeResizer, Position } from '@xyflow/react';
import type * as Monaco from 'monaco-editor';
import { next as A } from '@automerge/automerge';
import { diffToEditorChanges, getFileContent } from '@quarto/preview-runtime';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';
import type { TextDocumentContent } from '@quarto/quarto-automerge-schema';
import PreviewRouter from '../../render/PreviewRouter';
import type { PreviewMode } from '../../render/getQ2Format';
import { useInput, useOutput, type DiagnosticsPort, type DocPort, type ProjectPort, type VfsPort } from './ports';

const noop = () => {};
const bareId = (id: string) => id.replace(/^automerge:/, '');

interface PreviewProps {
  doc: DocPort<TextDocumentContent>;
  path: string;
  project: ProjectPort;
  onDiagnostics: (port: DiagnosticsPort) => void;
  onStatus: (status: string) => void;
}

function Preview({ doc, path, project, onDiagnostics, onStatus }: PreviewProps) {
  // As in the standalone preview demo: no editor for the router to
  // scroll-sync with.
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const editorHasFocusRef = useRef(false);

  const currentFile = useMemo<FileEntry>(() => ({ path, docId: doc.id }), [path, doc.id]);
  const content = doc.doc.text;
  const fileContents = useMemo(() => {
    const map = new Map<string, string>();
    for (const f of project.files) {
      const text = getFileContent(f.path);
      if (text !== null) map.set(f.path, text);
    }
    map.set(path, content);
    return map;
  }, [project, path, content]);

  // What the router is doing, for the node's footer.
  const [wasm, setWasm] = useState<{ status: string; error: string | null }>({ status: 'loading', error: null });
  const [mode, setMode] = useState<PreviewMode | null>(null);
  const [renders, setRenders] = useState(0);
  useEffect(() => {
    const modeText = mode === null ? 'classifying' : mode.mode === 'react' ? `react · ${mode.format}` : mode.mode === 'dom' ? 'dom' : `${mode.mode} · ${mode.formatKey}`;
    onStatus(`${path} · wasm ${wasm.error ?? wasm.status} · ${modeText} · ${renders} render${renders === 1 ? '' : 's'} reported`);
  }, [path, wasm, mode, renders, onStatus]);
  const onWasmStatusChange = useCallback((status: 'loading' | 'ready' | 'error', error: string | null) => setWasm({ status, error }), []);

  // The render's diagnostics, out on the port.
  const onDiagnosticsChange = useCallback((diagnostics: Diagnostic[]) => {
    setRenders((n) => n + 1);
    onDiagnostics({ path, diagnostics });
  }, [onDiagnostics, path]);

  // A block edit in the preview: the rewritten document, spliced into the doc.
  const docRef = useRef(doc);
  useEffect(() => { docRef.current = doc; }, [doc]);
  const onContentRewrite = useCallback((next: string) => {
    const current = docRef.current;
    const changes = diffToEditorChanges(current.doc.text, next);
    if (changes.length === 0) return;
    current.change((d) => {
      for (const c of changes) A.splice(d, ['text'], c.rangeOffset, c.rangeLength, c.text);
    });
  }, []);

  return (
    <PreviewRouter
      content={content}
      currentFile={currentFile}
      files={project.files}
      fileContents={fileContents}
      scrollSyncEnabled={false}
      editorRef={editorRef}
      editorReady={false}
      editorHasFocusRef={editorHasFocusRef}
      onFileChange={noop}
      onOpenNewFileDialog={noop}
      onDiagnosticsChange={onDiagnosticsChange}
      onWasmStatusChange={onWasmStatusChange}
      onPreviewModeChange={setMode}
      onContentRewrite={onContentRewrite}
      attributionOn={false}
    />
  );
}

export default function PreviewNode() {
  const doc = useInput<DocPort<TextDocumentContent>>('doc');
  const pathInput = useInput<string>('path');
  const project = useInput<ProjectPort>('project');
  const vfs = useInput<VfsPort>('vfs');
  // The file the document is edited as; a bare document gets the stand-in
  // name the editor uses.
  const path = doc === null ? null : (pathInput ?? `${bareId(doc.id)}.qmd`);
  const [status, setStatus] = useState<string | null>(null);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsPort | undefined>(undefined);
  // As in the app: once the project has connected (its port is only
  // published after `connect` resolves, which waits for every file to be
  // mirrored into the VFS), the preview can render.
  const ready = doc !== null && path !== null && project !== null && vfs !== null;
  useOutput('diagnostics', ready ? diagnostics : undefined);
  return (
    <div className="cd-node cd-previewnode">
      <NodeResizer minWidth={320} minHeight={200} />
      <div className="cd-title">Preview</div>
      <div className="cd-preview nodrag nowheel nokey">
        {doc === null
          ? <div className="cd-empty">No doc connected to preview</div>
          : vfs === null
            ? <div className="cd-empty">No VFS connected to render from</div>
            : project === null
              ? <div className="cd-empty">No project connected for the other files</div>
              : !ready || path === null
                ? <div className="cd-empty">waiting for the project</div>
                : <Preview doc={doc} path={path} project={project} onDiagnostics={setDiagnostics} onStatus={setStatus} />}
      </div>
      {ready && status !== null && <div className="cd-meta">{status}</div>}
      <Handle type="target" position={Position.Left} id="doc" title="doc" style={{ top: '16%' }} />
      <Handle type="target" position={Position.Left} id="path" title="path" style={{ top: '33%' }} />
      <Handle type="target" position={Position.Left} id="vfs" title="vfs" style={{ top: '50%' }} />
      <Handle type="target" position={Position.Left} id="project" title="project" style={{ top: '67%' }} />
      <Handle type="source" position={Position.Left} id="diagnostics" title="diagnostics" style={{ top: '84%' }} />
    </div>
  );
}
