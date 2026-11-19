/**
 * A headless project owner: test code that stands in for the project owner (a
 * hub with `--git track`, a browser, or the real owner GE-12 builds). It writes
 * `files[path]` changes on the index with plain Automerge changes, the way an
 * owner flips a path to a different doc when it checks out another version.
 *
 * Plain projects only: there is no registry sidecar here. GE-12's owner adds the
 * GE-5 registry operations (register, move forward, retire) in the same change.
 */
import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import { createIndexDocumentContent, type IndexDocument } from '@quarto/quarto-automerge-schema';
import { findDoc, webSocketAdapter } from '../core/repo';
import type { TextDoc } from '../core/text-binding';

/** The part of the in-process test hub the owner needs. */
export interface HubLike {
  url: string;
  hubHasHeads(docId: string, heads: string[], timeoutMs?: number): Promise<boolean>;
}

export interface BatchChange {
  /** Paths to point at a freshly created doc holding `text`. */
  flips?: { path: string; text: string }[];
  /** Paths to point at an existing doc id (a flip back, or a doc created earlier with `createDocFor`). */
  relists?: { path: string; docId: string }[];
  removals?: string[];
  additions?: { path: string; text: string }[];
}

export class HeadlessOwner {
  private readonly docs = new Map<string, DocHandle<TextDoc>>();
  private offline: Repo | null = null;

  private constructor(
    private readonly hub: HubLike,
    private readonly repo: Repo,
    private readonly index: DocHandle<IndexDocument>,
  ) {}

  /** Create a new project holding `files` and flush it to the hub. */
  static async create(hub: HubLike, files: Record<string, string>): Promise<HeadlessOwner> {
    const repo = new Repo({ network: [webSocketAdapter(hub.url)] });
    const index = repo.create<IndexDocument>(createIndexDocumentContent());
    const owner = new HeadlessOwner(hub, repo, index);
    for (const [p, text] of Object.entries(files)) {
      const docId = owner.createDocFor(p, text);
      index.change((d) => void (d.files[p] = docId));
    }
    await owner.flush();
    return owner;
  }

  /** Act as the owner of an existing project. */
  static async open(hub: HubLike, indexDocId: string): Promise<HeadlessOwner> {
    const repo = new Repo({ network: [webSocketAdapter(hub.url)] });
    const index = await findDoc<IndexDocument>(repo, indexDocId);
    const owner = new HeadlessOwner(hub, repo, index);
    for (const docId of Object.values(index.doc()?.files ?? {})) {
      owner.docs.set(docId, await findDoc<TextDoc>(repo, docId));
    }
    return owner;
  }

  get indexDocId(): string {
    return this.index.documentId;
  }

  indexHeads(): string[] {
    return this.index.heads() as string[];
  }

  /** The text of the doc currently listed at `path` as this owner sees it. */
  textOf(path: string): string | undefined {
    const docId = this.docIdOf(path);
    return docId ? this.docs.get(docId)?.doc()?.text : undefined;
  }

  docIdOf(path: string): string | undefined {
    return this.index.doc()?.files[path];
  }

  /** Create a doc holding `text`. The index does not list it until a flip, a relist, or an addition does. */
  createDocFor(_path: string, text: string): string {
    const handle = this.repo.create<TextDoc>({ text });
    this.docs.set(handle.documentId, handle);
    return handle.documentId;
  }

  /**
   * A doc that exists only in a repo with no network: listing it makes a follower's load of it hang
   * (the hub has never seen it). `publishOffline` connects that repo, and the doc "arrives".
   */
  createDocOffline(text: string): string {
    this.offline ??= new Repo({ network: [] });
    const handle = this.offline.create<TextDoc>({ text });
    return handle.documentId;
  }

  /** Connect the offline repo to the hub and wait until the hub holds every doc in it. */
  async publishOffline(): Promise<void> {
    const repo = this.offline;
    if (!repo) return;
    repo.networkSubsystem.addNetworkAdapter(webSocketAdapter(this.hub.url));
    for (const id of Object.keys(repo.handles)) {
      const h = repo.handles[id as never] as DocHandle<unknown>;
      if (!(await this.hub.hubHasHeads(h.documentId, h.heads() as string[], 15_000))) {
        throw new Error(`hub never received offline doc ${h.documentId}`);
      }
    }
  }

  flipPath(path: string, newDocId: string): void {
    this.index.change((d) => void (d.files[path] = newDocId));
  }

  removePath(path: string): void {
    this.index.change((d) => void delete d.files[path]);
  }

  relistPath(path: string, docId: string): void {
    this.index.change((d) => void (d.files[path] = docId));
  }

  /** Every listed operation as one change on the index. */
  applyBatch(batch: BatchChange): void {
    const flips = (batch.flips ?? []).map((f) => [f.path, this.createDocFor(f.path, f.text)] as const);
    const additions = (batch.additions ?? []).map((f) => [f.path, this.createDocFor(f.path, f.text)] as const);
    this.index.change((d) => {
      for (const [p, docId] of flips) d.files[p] = docId;
      for (const [p, docId] of additions) d.files[p] = docId;
      for (const r of batch.relists ?? []) d.files[r.path] = r.docId;
      for (const p of batch.removals ?? []) delete d.files[p];
    });
  }

  /** Send an ephemeral message on the doc currently listed at `path`, as another peer would. */
  broadcast(path: string, message: unknown): void {
    const docId = this.docIdOf(path);
    const handle = docId ? this.docs.get(docId) : undefined;
    if (!handle) throw new Error(`owner holds no doc for ${path}`);
    handle.broadcast(message);
  }

  /** Edit the text of the doc currently listed at `path` (what a browser user typing does). */
  editText(path: string, text: string): void {
    const docId = this.docIdOf(path);
    const handle = docId ? this.docs.get(docId) : undefined;
    if (!handle) throw new Error(`owner holds no doc for ${path}`);
    handle.change((d) => void (d.text = text));
  }

  /** Wait until the hub holds every doc this owner holds, at its current heads. Presence is not delivery. */
  async flush(): Promise<void> {
    const handles = [this.index as DocHandle<unknown>, ...this.docs.values()];
    for (const h of handles) {
      if (!(await this.hub.hubHasHeads(h.documentId, h.heads() as string[], 15_000))) {
        throw new Error(`hub never received ${h.documentId}`);
      }
    }
  }

  async stop(): Promise<void> {
    await this.offline?.shutdown().catch(() => {});
    await this.repo.shutdown().catch(() => {});
  }
}
