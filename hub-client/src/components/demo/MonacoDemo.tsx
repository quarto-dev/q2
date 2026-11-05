/**
 * #/demo/monaco — the hub-client Monaco editor bound to a file of a hub
 * project: the project's index doc maps the file path to its text doc.
 * Documents are cached in IndexedDB and synced through the local hub.
 */

import { useEffect, useState } from 'react';
import { Repo, type AnyDocumentId, type DocHandle } from '@automerge/automerge-repo';
import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import type { IndexDocument, TextDocumentContent } from '@quarto/quarto-automerge-schema';
import AutomergeEditor from '../AutomergeEditor';
import DemoFrame from './DemoFrame';

const SYNC_SERVER_URL = 'ws://127.0.0.1:8080/ws';
const INDEX_DOC_ID = 'automerge:2t9ZYdN85oy3ubaoEUrJB6rcpEYc' as AnyDocumentId;
const FILE_PATH = 'a.qmd';

let repo: Repo | undefined;

function getRepo(): Repo {
  repo ??= new Repo({
    storage: new IndexedDBStorageAdapter(),
    network: [new BrowserWebSocketClientAdapter(SYNC_SERVER_URL)],
  });
  return repo;
}

/** Resolve the file's text doc through the project's index doc. */
async function openFile(): Promise<DocHandle<TextDocumentContent>> {
  const index = await getRepo().find<IndexDocument>(INDEX_DOC_ID);
  const docId = index.doc()?.files[FILE_PATH];
  if (docId === undefined) throw new Error(`${FILE_PATH} is not in the project`);
  const url = docId.startsWith('automerge:') ? docId : `automerge:${docId}`;
  return getRepo().find<TextDocumentContent>(url as AnyDocumentId);
}

export default function MonacoDemo() {
  const [handle, setHandle] = useState<DocHandle<TextDocumentContent> | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void openFile().then(setHandle).catch((err: unknown) => setError(String(err)));
  }, []);

  return (
    <DemoFrame info={error ?? FILE_PATH}>
      {handle !== null && <AutomergeEditor handle={handle} path={['text']} />}
    </DemoFrame>
  );
}
