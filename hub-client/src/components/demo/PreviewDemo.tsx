/**
 * #/demo/preview — preview of a file in a hub project, rendered by the same
 * PreviewRouter the editor uses. The preview runtime's `connect` loads the
 * project into the WASM VFS and keeps it synced; in-preview block edits are
 * written back to the document as splices.
 */

import { useRef } from 'react';
import type * as Monaco from 'monaco-editor';
import { applyEditorOperations, diffToEditorChanges, getFileContent } from '@quarto/preview-runtime';
import PreviewRouter from '../render/PreviewRouter';
import DemoFrame from './DemoFrame';
import { FILE_PATH, useHubProject } from './hubProject';

const noop = () => {};

/** In-preview edits arrive as the whole rewritten QMD; apply them as splices. */
function rewriteFile(newContent: string): void {
  const changes = diffToEditorChanges(getFileContent(FILE_PATH) ?? '', newContent);
  if (changes.length > 0) applyEditorOperations(FILE_PATH, changes);
}

export default function PreviewDemo() {
  const { files, fileContents, currentFile, error } = useHubProject(FILE_PATH);
  // PreviewRouter expects an editor to scroll-sync with; the demo has none.
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const editorHasFocusRef = useRef(false);

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
