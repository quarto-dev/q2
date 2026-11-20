/**
 * An Automerge document: a handle in the repo wired into "repo", the way
 * hub-client holds every file it edits. The repo owns storage and network;
 * this node is the document itself — its text and history, in memory,
 * kept whatever the network does. Which document: the id wired into
 * "docid" (the file explorer's pick) if that holds one, else the id typed
 * here. The doc goes out on the "doc" port.
 */

import { useEffect, useMemo, useState, type KeyboardEvent } from 'react';
import { Handle, Position } from '@xyflow/react';
import { next as A } from '@automerge/automerge';
import type { AnyDocumentId, DocHandle } from '@automerge/automerge-repo';
import type { TextDocumentContent } from '@quarto/quarto-automerge-schema';
import AutomergeLogo from './AutomergeLogo';
import { useInput, useOutput, type DocPort, type IdStore, type RepoPort } from './ports';

const bareId = (id: string) => id.replace(/^automerge:/, '');
const asUrl = (id: string) => `automerge:${bareId(id)}` as AnyDocumentId;

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

interface Opened {
  repo: RepoPort;
  id: string;
  handle: DocHandle<TextDocumentContent> | null;
  error: string | null;
}

export default function AutomergeNode() {
  const repoPort = useInput<RepoPort>('repo');
  const idStore = useInput<IdStore>('docid');
  // The id typed here.
  const [localId, setLocalId] = useState<string | null>(null);
  // What the repo returned for `opened.id`; stale once either moves on.
  const [opened, setOpened] = useState<Opened | null>(null);
  const [doc, setDoc] = useState<A.Doc<TextDocumentContent> | null>(null);

  // A wired-in slot wins over the local one.
  const docId = idStore?.id ?? localId;
  const current = opened !== null && opened.id === docId && opened.repo === repoPort ? opened : null;
  const handle = current?.handle ?? null;
  const error = current?.error ?? null;

  // A wired-in slot that holds nothing yet learns the typed id.
  useEffect(() => {
    if (idStore !== null && idStore.id === null && localId !== null) idStore.set(localId);
  }, [idStore, localId]);

  // Find the document in the repo; once held, keep the handle. One that
  // is in neither the repo's storage nor reachable over its network is
  // retried when a network attaches.
  const networked = repoPort?.networkAttached ?? false;
  useEffect(() => {
    if (repoPort === null || docId === null) return;
    if (opened !== null && opened.id === docId && opened.repo === repoPort && opened.handle !== null) return;
    let cancelled = false;
    void repoPort.repo.find<TextDocumentContent>(asUrl(docId))
      .then((h) => {
        if (cancelled) return;
        setOpened({ repo: repoPort, id: docId, handle: h, error: null });
        setDoc(h.doc() ?? null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        const message = String(err).includes('unavailable')
          ? networked
            ? `${docId} is not on the hub — pick another file, or enter another id`
            : `${docId} is not in the repo's storage, and no network is attached to fetch it over`
          : String(err);
        setOpened({ repo: repoPort, id: docId, handle: null, error: message });
      });
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `opened` is read, not a trigger
  }, [repoPort, docId, networked]);

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

  const onCommitId = (id: string) => {
    setLocalId(id);
    idStore?.set(id);
  };

  const status = error ?? (
    repoPort === null
      ? 'No repo connected to hold the document'
      : docId === null
        ? 'no doc id — pick a file in the explorer, or enter an id'
        : handle === null ? 'opening…' : null
  );
  const heads = doc === null ? null : A.getHeads(doc).length;

  return (
    <div className="cd-node cd-automerge">
      <div className="cd-title cd-title-logo"><AutomergeLogo size={28} /> Document</div>
      <IdBox id={docId} onCommit={onCommitId} />
      {status !== null && <div className="cd-meta">{status}</div>}
      <div className="cd-meta">
        {doc === null ? 'no document' : `handle in the repo · ${doc.text.length} chars · ${heads} head${heads === 1 ? '' : 's'}`}
      </div>
      <Handle type="target" position={Position.Top} id="repo" title="repo" />
      <Handle type="target" position={Position.Left} id="docid" title="doc id" />
      <Handle type="source" position={Position.Right} id="doc" title="doc" />
    </div>
  );
}
