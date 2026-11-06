/**
 * The hub project the demos share, loaded through the preview runtime's
 * `connect`: it fills the WASM VFS, keeps the project synced, and backs the
 * runtime calls (`getFileContent`, `applyEditorOperations`, presence, ...)
 * the editor and preview components make.
 */

import { useEffect, useState } from 'react';
import type { Repo } from '@automerge/automerge-repo';
import { connect, getFileContent, getFileHandle, isConnected, type FileEntry } from '@quarto/preview-runtime';

export const SYNC_SERVER_URL = 'ws://127.0.0.1:8080/ws';
export const INDEX_DOC_ID = 'automerge:2t9ZYdN85oy3ubaoEUrJB6rcpEYc';
export const FILE_PATH = 'a.qmd';

// One project load per server, shared across demos and React's dev
// double-mount: two overlapping `connect` calls on the runtime's single
// sync client clobber each other. Left connected on unmount; the next
// `connect` (ours or App's) disconnects first anyway.
let load: { key: string; promise: Promise<FileEntry[]>; settled: boolean } | undefined;

/**
 * Connect the runtime to the project `indexDocId` on `url`. With `repo`
 * the connection runs on that repo (the caller's, with whatever adapters
 * it has attached); `key` names it so a different repo means a new
 * connection. Without one the client builds its own repo and adapters.
 */
export function loadProject(
  url: string = SYNC_SERVER_URL,
  indexDocId: string = INDEX_DOC_ID,
  repo?: Repo,
  key: string = '',
): Promise<FileEntry[]> {
  const loadKey = `${url}\n${indexDocId}\n${key}`;
  // Reuse the load while it is in flight or still connected.
  if (load !== undefined && load.key === loadKey && (!load.settled || isConnected())) return load.promise;
  const current = { key: loadKey, settled: false, promise: connect(url, indexDocId, undefined, undefined, undefined, repo === undefined ? {} : { repo }) };
  current.promise.finally(() => { current.settled = true; }).catch(() => {});
  load = current;
  return current.promise;
}

export interface HubProject {
  files: FileEntry[];
  fileContents: Map<string, string>;
  /** The entry for `filePath`, once the project has loaded. */
  currentFile: FileEntry | null;
  error: string | null;
}

/**
 * Load the project `indexDocId` from `url` (on `repo` when given) and keep
 * `filePath`'s content current. With a null path only the file list and
 * initial contents load; with a null url nothing loads.
 */
export function useHubProject(
  filePath: string | null,
  url: string | null = SYNC_SERVER_URL,
  indexDocId: string = INDEX_DOC_ID,
  repo?: Repo,
  repoKey: string = '',
): HubProject {
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [fileContents, setFileContents] = useState<Map<string, string>>(new Map());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (url === null) return;
    // Watch the file's own doc handle rather than the runtime's global sync
    // handlers: App registers those on mount and would overwrite ours.
    const refresh = () => {
      if (filePath === null) return;
      const text = getFileContent(filePath);
      if (text !== null) setFileContents((prev) => new Map(prev).set(filePath, text));
    };
    let handle: ReturnType<typeof getFileHandle>;
    let cancelled = false;
    void loadProject(url, indexDocId, repo, repoKey)
      .then((projectFiles) => {
        if (cancelled) return;
        const contents = new Map<string, string>();
        for (const file of projectFiles) {
          const text = getFileContent(file.path);
          if (text !== null) contents.set(file.path, text);
        }
        setFiles(projectFiles);
        setFileContents(contents);
        if (filePath !== null) {
          handle = getFileHandle(filePath);
          handle?.on('change', refresh);
        }
      })
      .catch((err: unknown) => setError(String(err)));
    return () => {
      cancelled = true;
      handle?.off('change', refresh);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `repo` is identified by repoKey
  }, [filePath, url, indexDocId, repoKey]);

  return { files, fileContents, currentFile: files.find((file) => file.path === filePath) ?? null, error };
}
