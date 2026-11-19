/**
 * The extension's view of the registry sidecar (GE-10; contract: "Sidecar doc", "Operations"). It finds the sidecar from
 * the index key `epochs`, creates it lazily, reads and writes entries with the GE-5 registry operations (one Automerge
 * change each), and notices when another peer's sidecar won the index key.
 */
import type { DocHandle, Repo } from '@automerge/automerge-repo/slim';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import {
  asString,
  canonical,
  moveForward,
  nextGeneration,
  register,
  repair,
  retire,
  viewOf,
  type Entry,
  type RegistryView,
  type Sidecar,
} from '@quarto/quarto-git-epochs';
import { findDoc } from '../repo';

/** How long a sidecar another peer just created may stay unavailable before a read gives up (the contract's 5 s rule is per file; a sidecar is worth waiting longer for). */
const SIDECAR_WAIT_MS = 30_000;

export class RegistryClient {
  private handle: DocHandle<Sidecar> | null = null;
  private refreshing: Promise<void> | null = null;
  private disposed = false;
  /** One-line diagnostics (the extension's output channel). */
  log: (line: string) => void = () => {};
  private changeListeners = new Set<() => void>();
  private readonly onSidecarChange = () => this.changeListeners.forEach((cb) => cb());
  /** Called when the index key resolved to a sidecar other than the one this client held: re-register attached docs. */
  onSidecarSwitched: ((newSidecarId: string, previousId: string | null) => void) | null = null;

  constructor(
    private readonly repo: Repo,
    private readonly index: DocHandle<IndexDocument>,
  ) {}

  sidecarId(): string | null {
    return this.handle?.documentId ?? null;
  }

  private indexKey(): string | null {
    const v = (this.index.doc() as { epochs?: unknown } | undefined)?.epochs;
    return v == null ? null : asString(v);
  }

  /** The sidecar handle: the one the index names, or a new one written into the index. */
  async ensure(): Promise<DocHandle<Sidecar>> {
    if (this.handle) return this.handle;
    const key = this.indexKey();
    if (key) {
      this.use(await this.findSidecar(key));
      return this.handle!;
    }
    // Both maps are created in this one change by this one writer (contract: "Creating the maps").
    const created = this.repo.create<Sidecar>({ version: 1, entries: {}, retired: {} } as Sidecar);
    this.use(created);
    this.log(`registry: created sidecar ${created.documentId}`);
    this.index.change((d) => {
      if ((d as { epochs?: unknown }).epochs == null) (d as { epochs?: string }).epochs = created.documentId;
    });
    await this.refresh(); // a concurrent writer's key may already have won
    return this.handle!;
  }

  /**
   * A sidecar another peer created a moment ago can be rejected as `unavailable` at once, because the server has not
   * received it yet (contract: "Unavailable canonical doc"). Ask again until it arrives.
   */
  private async findSidecar(id: string): Promise<DocHandle<Sidecar>> {
    const deadline = Date.now() + SIDECAR_WAIT_MS;
    for (;;) {
      try {
        return await findDoc<Sidecar>(this.repo, id, AbortSignal.timeout(2_000));
      } catch (err) {
        if (this.disposed || Date.now() > deadline) throw err;
        await new Promise((r) => setTimeout(r, 200));
      }
    }
  }

  /** Re-read the index key; adopt another peer's sidecar when its key won. One refresh runs at a time and re-checks the key when it ends. */
  refresh(): Promise<void> {
    if (!this.refreshing) {
      this.refreshing = this.refreshOnce().finally(() => {
        this.refreshing = null;
      });
    }
    return this.refreshing;
  }

  private async refreshOnce(): Promise<void> {
    for (;;) {
      const key = this.indexKey();
      if (this.disposed || !key || (this.handle && key === this.handle.documentId)) return;
      const previous = this.handle?.documentId ?? null;
      this.use(await this.findSidecar(key));
      this.log(`registry: adopted sidecar ${key} (was ${previous})`);
      if (previous !== null) this.onSidecarSwitched?.(key, previous);
    }
  }

  private use(h: DocHandle<Sidecar>): void {
    this.handle?.off('change', this.onSidecarChange);
    this.handle = h;
    h.on('change', this.onSidecarChange);
  }

  /** Re-read the index key whenever the index changes, so a lost sidecar race is noticed (G6). Returns the unsubscribe. */
  follow(): () => void {
    const onIndex = () => void this.refresh().catch((e) => this.log(`registry: could not adopt the index's sidecar: ${e instanceof Error ? e.message : String(e)}`));
    this.index.on('change', onIndex);
    onIndex(); // the key may already differ from the sidecar this client holds
    return () => this.index.off('change', onIndex);
  }

  /** Subscribe to sidecar changes (local and remote). Returns the unsubscribe. */
  onChange(cb: () => void): () => void {
    this.changeListeners.add(cb);
    return () => void this.changeListeners.delete(cb);
  }

  dispose(): void {
    this.disposed = true;
    this.handle?.off('change', this.onSidecarChange);
    this.changeListeners.clear();
  }

  private get h(): DocHandle<Sidecar> {
    if (!this.handle) throw new Error('registry sidecar not loaded; call ensure() first');
    return this.handle;
  }

  heads(): string[] {
    return this.h.heads() as string[];
  }

  view(): RegistryView {
    return this.handle?.doc() ? viewOf(this.handle.doc()!) : { entries: [], retired: [] };
  }

  canonical(path: string, blob: string): Entry | null {
    return canonical(this.view(), path, blob);
  }

  nextGeneration(path: string): number {
    return nextGeneration(this.view(), path);
  }

  register(e: Entry): void {
    this.h.update((d) => register(d, e));
  }

  moveForward(a: { path: string; oldBlob: string; newBlob: string; doc: string }): void {
    this.h.update((d) => moveForward(d, a));
  }

  retire(a: { path: string; blob: string; doc: string }): void {
    this.h.update((d) => retire(d, a));
  }

  repair(attached: { path: string; doc: string; gen: number }, headBlob: string): void {
    this.h.update((d) => repair(d, attached, headBlob));
  }
}
