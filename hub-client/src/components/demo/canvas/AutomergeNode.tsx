/**
 * A quarto hub text document held in memory by a repo connected to the
 * local hub. Which document: the id wired into the "docid" port if that
 * holds one, else the id typed into the node, else the project's `a.qmd`
 * resolved through the index doc, the way the Monaco demo does. The doc
 * goes out on the "doc" port; if a store is wired into "storage", a
 * snapshot of the doc is loaded from and saved to it.
 */

import { useEffect, useMemo, useState, type KeyboardEvent } from 'react';
import { Handle, Position } from '@xyflow/react';
import { next as A } from '@automerge/automerge';
import { Repo, type AnyDocumentId, type DocHandle } from '@automerge/automerge-repo';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import type { IndexDocument, TextDocumentContent } from '@quarto/quarto-automerge-schema';
import AutomergeLogo from './AutomergeLogo';
import { useInput, useOutput, type DocPort, type DocStore, type IdStore } from './ports';

const SYNC_SERVER_URL = 'ws://127.0.0.1:8080/ws';
const INDEX_DOC_ID = 'automerge:2t9ZYdN85oy3ubaoEUrJB6rcpEYc' as AnyDocumentId;
const FILE_PATH = 'a.qmd';

// One in-memory repo for the page's lifetime (survives React's dev double-mount).
let repo: Repo | undefined;
let projectIdPromise: Promise<string> | undefined;

function getRepo(): Repo {
  repo ??= new Repo({ network: [new BrowserWebSocketClientAdapter(SYNC_SERVER_URL)] });
  return repo;
}

const bareId = (id: string) => id.replace(/^automerge:/, '');
const asUrl = (id: string) => `automerge:${bareId(id)}` as AnyDocumentId;

/** The id of the project file, through the project's index doc. */
async function resolveProjectFile(): Promise<string> {
  const index = await getRepo().find<IndexDocument>(INDEX_DOC_ID);
  const docId = index.doc()?.files[FILE_PATH];
  if (docId === undefined) throw new Error(`${FILE_PATH} is not in the project`);
  return bareId(docId);
}

/** The doc id, as a text box; Enter or blur commits, Escape reverts. */
function IdBox({ id, onCommit }: { id: string | null; onCommit: (id: string) => void }) {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft !== null && draft.trim() !== '' && bareId(draft.trim()) !== id) onCommit(bareId(draft.trim()));
    setDraft(null);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') e.currentTarget.blur();
    if (e.key === 'Escape') { setDraft(null); e.currentTarget.blur(); }
  };
  return (
    <input
      className="cd-id cd-id-box nodrag"
      spellCheck={false}
      placeholder="doc id"
      value={draft ?? id ?? ''}
      onChange={(e) => setDraft(e.target.value)}
      onFocus={() => setDraft(id ?? '')}
      onBlur={commit}
      onKeyDown={onKeyDown}
    />
  );
}

export default function AutomergeNode() {
  const idStore = useInput<IdStore>('docid');
  const store = useInput<DocStore>('storage');
  // The id chosen here (typed, or resolved from the project).
  const [localId, setLocalId] = useState<string | null>(null);
  // What `find` returned for `opened.id`; stale once the id moves on.
  const [opened, setOpened] = useState<{ id: string; handle: DocHandle<TextDocumentContent> | null; error: string | null } | null>(null);
  const [doc, setDoc] = useState<A.Doc<TextDocumentContent> | null>(null);
  const [resolveError, setResolveError] = useState<string | null>(null);

  // A wired-in remembered id wins over the local one.
  const docId = idStore?.id ?? localId;
  const current = opened !== null && opened.id === docId ? opened : null;
  const handle = current?.handle ?? null;
  const error = resolveError ?? current?.error ?? null;

  // Nothing remembered anywhere: start from the project's file.
  useEffect(() => {
    if (localId !== null) return;
    projectIdPromise ??= resolveProjectFile();
    void projectIdPromise.then(setLocalId).catch((err: unknown) => setResolveError(String(err)));
  }, [localId]);

  // A wired-in slot that holds nothing yet learns the current id.
  useEffect(() => {
    if (idStore !== null && idStore.id === null && localId !== null) idStore.set(localId);
  }, [idStore, localId]);

  // Open whichever doc the id names.
  useEffect(() => {
    if (docId === null) return;
    let cancelled = false;
    void getRepo().find<TextDocumentContent>(asUrl(docId))
      .then((h) => {
        if (cancelled) return;
        setOpened({ id: docId, handle: h, error: null });
        setDoc(h.doc() ?? null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        const message = String(err).includes('unavailable')
          ? `${docId} is not on the hub — enter another id, or clear the localStorage node to fall back to ${FILE_PATH}`
          : String(err);
        setOpened({ id: docId, handle: null, error: message });
      });
    return () => { cancelled = true; };
  }, [docId]);

  // Follow the handle so the port always carries the current doc.
  useEffect(() => {
    if (handle === null) return;
    const onChange = ({ doc: next }: { doc?: A.Doc<TextDocumentContent> }) => setDoc(next ?? null);
    handle.on('change', onChange);
    return () => { handle.off('change', onChange); };
  }, [handle]);

  const port = useMemo<DocPort<TextDocumentContent> | undefined>(() => {
    if (handle === null || doc === null) return undefined;
    return { id: handle.documentId, doc, change: (fn) => handle.change(fn) };
  }, [handle, doc]);
  useOutput('doc', port);

  // Storage attached: merge in the snapshot it holds for this document.
  useEffect(() => {
    if (store === null || handle === null) return;
    let cancelled = false;
    void store.load(handle.documentId).then((bytes) => {
      if (cancelled || bytes === undefined) return;
      handle.update((d) => A.merge(d, A.load<TextDocumentContent>(bytes)));
    });
    return () => { cancelled = true; };
  }, [store, handle]);

  // While attached, every version of the doc goes to the store.
  useEffect(() => {
    if (store === null || handle === null || doc === null) return;
    void store.save(handle.documentId, A.save(doc));
  }, [store, handle, doc]);

  const onCommitId = (id: string) => {
    setLocalId(id);
    idStore?.set(id);
  };

  const status = error ?? (docId === null || handle === null ? 'connecting…' : null);

  return (
    <div className="cd-node cd-automerge">
      <AutomergeLogo />
      <IdBox id={docId} onCommit={onCommitId} />
      {status !== null && <div className="cd-meta">{status}</div>}
      <Handle type="target" position={Position.Left} id="docid" title="doc id" />
      <Handle type="source" position={Position.Right} id="doc" title="doc" />
      <Handle type="target" position={Position.Bottom} id="storage" title="storage" />
    </div>
  );
}
