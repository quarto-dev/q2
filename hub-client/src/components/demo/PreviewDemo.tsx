/**
 * #/demo/preview — preview of a file in a hub project, rendered by the same
 * PreviewRouter the editor uses. The preview runtime's `connect` loads the
 * project into the WASM VFS and keeps it synced; in-preview block edits are
 * written back to the document as splices.
 */

import { useEffect, useRef, useState } from 'react';
import type * as Monaco from 'monaco-editor';
import { applyEditorOperations, connect, diffToEditorChanges, getFileContent, getFileHandle, isConnected, type FileEntry } from '@quarto/preview-runtime';
import PreviewRouter from '../render/PreviewRouter';
import DemoFrame from './DemoFrame';

const SYNC_SERVER_URL = 'ws://127.0.0.1:8080/ws';
const INDEX_DOC_ID = 'automerge:2t9ZYdN85oy3ubaoEUrJB6rcpEYc';
const FILE_PATH = 'a.qmd';

const noop = () => {};

/** In-preview edits arrive as the whole rewritten QMD; apply them as splices. */
function rewriteFile(newContent: string): void {
  const changes = diffToEditorChanges(getFileContent(FILE_PATH) ?? '', newContent);
  if (changes.length > 0) applyEditorOperations(FILE_PATH, changes);
}

// One project load shared across React's dev double-mount: two overlapping
// `connect` calls on the runtime's single sync client clobber each other.
// Left connected on unmount; the next `connect` (ours or App's) disconnects
// first anyway.
let loading: Promise<FileEntry[]> | undefined;
let loaded = false;

function loadProject(): Promise<FileEntry[]> {
  if (loaded && !isConnected()) { loading = undefined; loaded = false; }
  loading ??= connect(SYNC_SERVER_URL, INDEX_DOC_ID, undefined, undefined, undefined, { peerTimeoutMs: 30000 })
    .then((files) => { loaded = true; return files; });
  return loading;
}

export default function PreviewDemo() {
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [fileContents, setFileContents] = useState<Map<string, string>>(new Map());
  const [error, setError] = useState<string | null>(null);
  // PreviewRouter expects an editor to scroll-sync with; the demo has none.
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const editorHasFocusRef = useRef(false);

  useEffect(() => {
    // Watch the file's own doc handle rather than the runtime's global sync
    // handlers: App registers those on mount and would overwrite ours.
    const refresh = () => {
      const text = getFileContent(FILE_PATH);
      if (text !== null) setFileContents((prev) => new Map(prev).set(FILE_PATH, text));
    };
    let handle: ReturnType<typeof getFileHandle>;
    let cancelled = false;
    void loadProject()
      .then((projectFiles) => {
        if (cancelled) return;
        const contents = new Map<string, string>();
        for (const file of projectFiles) {
          const text = getFileContent(file.path);
          if (text !== null) contents.set(file.path, text);
        }
        setFiles(projectFiles);
        setFileContents(contents);
        handle = getFileHandle(FILE_PATH);
        handle?.on('change', refresh);
      })
      .catch((err: unknown) => setError(String(err)));
    return () => {
      cancelled = true;
      handle?.off('change', refresh);
    };
  }, []);

  const currentFile = files.find((file) => file.path === FILE_PATH) ?? null;

  return (
    <DemoFrame info={error ?? FILE_PATH}>
      {currentFile !== null && (
        <PreviewRouter
          content={fileContents.get(FILE_PATH) ?? ''}
          currentFile={currentFile}
          files={files}
          fileContents={fileContents}
          scrollSyncEnabled={false}
          editorRef={editorRef}
          editorReady={false}
          editorHasFocusRef={editorHasFocusRef}
          onFileChange={noop}
          onOpenNewFileDialog={noop}
          onDiagnosticsChange={noop}
          onContentRewrite={rewriteFile}
          attributionOn={false}
        />
      )}
    </DemoFrame>
  );
}
