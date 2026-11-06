/**
 * The hub project the demos share, loaded through the preview runtime's
 * `connect`: it fills the WASM VFS, keeps the project synced, and backs the
 * runtime calls (`getFileContent`, `applyEditorOperations`, presence, ...)
 * the editor and preview components make.
 */

import { useEffect, useState } from 'react';
import { connect, getFileContent, getFileHandle, isConnected, type FileEntry } from '@quarto/preview-runtime';

export const SYNC_SERVER_URL = 'ws://127.0.0.1:8080/ws';
export const INDEX_DOC_ID = 'automerge:2t9ZYdN85oy3ubaoEUrJB6rcpEYc';
export const FILE_PATH = 'a.qmd';

// One project load shared across demos and React's dev double-mount: two
// overlapping `connect` calls on the runtime's single sync client clobber
// each other. Left connected on unmount; the next `connect` (ours or App's)
// disconnects first anyway.
let loading: Promise<FileEntry[]> | undefined;
let loaded = false;

export function loadProject(): Promise<FileEntry[]> {
  if (loaded && !isConnected()) { loading = undefined; loaded = false; }
  loading ??= connect(SYNC_SERVER_URL, INDEX_DOC_ID, undefined, undefined, undefined, { peerTimeoutMs: 30000 })
    .then((files) => { loaded = true; return files; });
  return loading;
}

export interface HubProject {
  files: FileEntry[];
  fileContents: Map<string, string>;
  /** The entry for `filePath`, once the project has loaded. */
  currentFile: FileEntry | null;
  error: string | null;
}

/** Load the shared project and keep `filePath`'s content current. */
export function useHubProject(filePath: string): HubProject {
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [fileContents, setFileContents] = useState<Map<string, string>>(new Map());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Watch the file's own doc handle rather than the runtime's global sync
    // handlers: App registers those on mount and would overwrite ours.
    const refresh = () => {
      const text = getFileContent(filePath);
      if (text !== null) setFileContents((prev) => new Map(prev).set(filePath, text));
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
        handle = getFileHandle(filePath);
        handle?.on('change', refresh);
      })
      .catch((err: unknown) => setError(String(err)));
    return () => {
      cancelled = true;
      handle?.off('change', refresh);
    };
  }, [filePath]);

  return { files, fileContents, currentFile: files.find((file) => file.path === filePath) ?? null, error };
}
