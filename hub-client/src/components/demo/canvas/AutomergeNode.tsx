/**
 * An Automerge document: a CRDT held in memory by this node's repo. That
 * is all it needs to function — the two things wired into it are extras,
 * attached and detached in place while the document stays put:
 *
 * - "idb": a storage adapter. While attached, the repo writes every change
 *   through it (as hub-client's repo does), and on attach whatever the
 *   store already holds for the document is merged in. Detached, changes
 *   stay in memory only.
 * - "adapter": a network adapter. While attached, the repo syncs the
 *   document over it. Detached, edits stay local — nothing leaves this
 *   node — and merge when a network is attached again, as CRDTs do.
 *
 * Which document: the id wired into "docid" (the file explorer's pick) if
 * that holds one, else the id typed here. A document that is in neither
 * memory nor the attached store needs a network to arrive over. The doc
 * goes out on the "doc" port.
 */

import { useEffect, useMemo, useState, type KeyboardEvent } from 'react';
import { Handle, Position } from '@xyflow/react';
import { next as A } from '@automerge/automerge';
import {
  Repo,
  type AnyDocumentId,
  type DocHandle,
  type NetworkAdapter,
  type StorageAdapterInterface,
  type StorageKey,
} from '@automerge/automerge-repo';
import type { TextDocumentContent } from '@quarto/quarto-automerge-schema';
import AutomergeLogo from './AutomergeLogo';
import { useInput, useOutput, type AdapterPort, type DocPort, type IdStore, type StorageAdapterPort } from './ports';

/**
 * The repo's storage: whatever adapter is attached right now. A repo's
 * storage is fixed at construction, so this stands in for it and forwards
 * to the attached adapter — or, with none attached, keeps nothing (reads
 * find nothing, writes are dropped).
 */
class SwitchableStorage implements StorageAdapterInterface {
  attached: StorageAdapterInterface | null = null;
  load(key: StorageKey) { return this.attached?.load(key) ?? Promise.resolve(undefined); }
  save(key: StorageKey, data: Uint8Array) { return this.attached?.save(key, data) ?? Promise.resolve(); }
  remove(key: StorageKey) { return this.attached?.remove(key) ?? Promise.resolve(); }
  loadRange(prefix: StorageKey) { return this.attached?.loadRange(prefix) ?? Promise.resolve([]); }
  removeRange(prefix: StorageKey) { return this.attached?.removeRange(prefix) ?? Promise.resolve(); }
}

// One repo for the page's lifetime (survives React's dev double-mount).
const storage = new SwitchableStorage();
const repo = new Repo({ network: [], storage });

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
  id: string;
  handle: DocHandle<TextDocumentContent> | null;
  error: string | null;
}

export default function AutomergeNode() {
  const adapter = useInput<AdapterPort>('adapter');
  const idb = useInput<StorageAdapterPort>('idb');
  const idStore = useInput<IdStore>('docid');
  // The id typed here.
  const [localId, setLocalId] = useState<string | null>(null);
  // What `find` returned for `opened.id`; stale once the id moves on.
  const [opened, setOpened] = useState<Opened | null>(null);
  const [doc, setDoc] = useState<A.Doc<TextDocumentContent> | null>(null);
  // A store or network attaching is the cue to retry a doc that was unavailable.
  const stored = idb !== null;
  const networked = adapter !== null;

  // A wired-in slot wins over the local one.
  const docId = idStore?.id ?? localId;
  const current = opened !== null && opened.id === docId ? opened : null;
  const handle = current?.handle ?? null;
  const error = current?.error ?? null;

  // Storage: attach the wired-in adapter to the repo's storage, merge in
  // what it holds for the open document, and write the document to it so
  // it is there from now on. Detach when the edge goes.
  useEffect(() => {
    if (idb === null) return;
    storage.attached = idb.adapter;
    let cancelled = false;
    if (handle !== null) {
      const id = handle.documentId;
      void idb.adapter.loadRange([id]).then((chunks) => {
        if (cancelled) return;
        for (const chunk of chunks) {
          if (chunk.data !== undefined) handle.update((d) => A.loadIncremental(d, chunk.data as Uint8Array));
        }
        const latest = handle.doc();
        if (latest !== undefined) void idb.adapter.save([id, 'snapshot', `attach-${Date.now()}`], A.save(latest));
      });
    }
    return () => {
      cancelled = true;
      if (storage.attached === idb.adapter) storage.attached = null;
    };
  }, [idb, handle]);

  // Network: attach the wired-in adapter to the repo; detach it when the
  // edge goes. The repo and its documents are untouched either way.
  useEffect(() => {
    if (adapter === null) return;
    const network: NetworkAdapter = adapter.take();
    repo.networkSubsystem.addNetworkAdapter(network);
    return () => {
      repo.networkSubsystem.removeNetworkAdapter(network);
      network.disconnect();
    };
  }, [adapter]);

  // A wired-in slot that holds nothing yet learns the typed id.
  useEffect(() => {
    if (idStore !== null && idStore.id === null && localId !== null) idStore.set(localId);
  }, [idStore, localId]);

  // Open whichever doc the id names; once it is held, keep it, whatever
  // attaches or detaches. One not held anywhere is retried when something
  // attaches that could supply it.
  useEffect(() => {
    if (docId === null) return;
    if (opened !== null && opened.id === docId && opened.handle !== null) return;
    let cancelled = false;
    void repo.find<TextDocumentContent>(asUrl(docId))
      .then((h) => {
        if (cancelled) return;
        setOpened({ id: docId, handle: h, error: null });
        setDoc(h.doc() ?? null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        const message = String(err).includes('unavailable')
          ? adapter !== null
            ? `${docId} is not on the hub — pick another file, or enter another id`
            : `${docId} is not held here or in the attached store, and no network adapter is attached to fetch it over`
          : String(err);
        setOpened({ id: docId, handle: null, error: message });
      });
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `opened`/`adapter` are read; a store or network attaching retries
  }, [docId, stored, networked]);

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
    docId === null
      ? 'no doc id — pick a file in the explorer, or enter an id'
      : handle === null ? 'opening…' : null
  );
  const heads = doc === null ? null : A.getHeads(doc).length;

  return (
    <div className="cd-node cd-automerge">
      <AutomergeLogo />
      <IdBox id={docId} onCommit={onCommitId} />
      {status !== null && <div className="cd-meta">{status}</div>}
      <div className="cd-meta">
        {doc === null ? 'no document' : `in memory · ${doc.text.length} chars · ${heads} head${heads === 1 ? '' : 's'}`}
      </div>
      <div className="cd-meta">
        {idb === null ? 'not stored' : `stored in ${idb.dbName}`} · {adapter !== null ? 'syncing' : 'offline'}
      </div>
      <Handle type="target" position={Position.Top} id="adapter" title="adapter" style={{ left: '35%' }} />
      <Handle type="target" position={Position.Top} id="docid" title="doc id" style={{ left: '65%' }} />
      <Handle type="target" position={Position.Bottom} id="idb" title="idb" />
      <Handle type="source" position={Position.Right} id="doc" title="doc" />
    </div>
  );
}
