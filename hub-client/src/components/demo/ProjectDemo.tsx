/**
 * #/demo/project — the hub-client file tree (no toolbar) for a hub project,
 * fed from the project's index doc (file paths and folders). Selection is
 * local; the file actions are stubs.
 */

import { useEffect, useState } from 'react';
import { Repo, type AnyDocumentId, type DocHandle } from '@automerge/automerge-repo';
import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import FileTree from '../FileTree';
import DemoFrame from './DemoFrame';
import './ProjectDemo.css';

const SYNC_SERVER_URL = 'ws://127.0.0.1:8080/ws';
const INDEX_DOC_ID = 'automerge:2t9ZYdN85oy3ubaoEUrJB6rcpEYc' as AnyDocumentId;

const noop = () => {};

let repo: Repo | undefined;

function getRepo(): Repo {
  repo ??= new Repo({
    storage: new IndexedDBStorageAdapter(),
    network: [new BrowserWebSocketClientAdapter(SYNC_SERVER_URL)],
  });
  return repo;
}

function filesOf(index: IndexDocument | undefined): FileEntry[] {
  return Object.entries(index?.files ?? {}).map(([path, docId]) => ({ path, docId }));
}

export default function ProjectDemo() {
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [folders, setFolders] = useState<string[]>([]);
  const [currentFile, setCurrentFile] = useState<FileEntry | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let handle: DocHandle<IndexDocument> | undefined;
    const refresh = () => {
      const index = handle?.doc();
      setFiles(filesOf(index));
      setFolders(Object.keys(index?.folders ?? {}));
    };
    void getRepo().find<IndexDocument>(INDEX_DOC_ID)
      .then((h) => {
        handle = h;
        refresh();
        h.on('change', refresh);
      })
      .catch((err: unknown) => setError(String(err)));
    return () => { handle?.off('change', refresh); };
  }, []);

  return (
    <DemoFrame info={error ?? (currentFile?.path ?? 'No file selected')}>
      <div className="pd-column">
        <FileTree
          files={files}
          folders={folders}
          currentFile={currentFile}
          onSelectFile={setCurrentFile}
          onUploadFiles={noop}
        />
      </div>
    </DemoFrame>
  );
}
