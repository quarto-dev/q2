/**
 * Fakes and fixtures for the core's tests: a fake watcher, buffer host, and
 * notifier, and helpers that seed a project on an in-process test hub.
 * Not part of the extension bundle.
 */
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import { createIndexDocumentContent, type IndexDocument } from '@quarto/quarto-automerge-schema';
import { startTestHub, type TestHub } from '@quarto/quarto-sync-client/testing';
import type { BufferBinding, BufferHost, FileWatcher, Logger, Notifier, SyncHost } from './host';
import type { Identity } from './presence';
import { MemoryLog, RecordingNotifier } from './recording';

export { MemoryLog, RecordingNotifier };
import { TextBindingCore, computeReplace, type TextDoc } from './text-binding';

export const identity: Identity = { userId: 'u-test', userName: 'Test', userColor: '#112233' };

export class FakeWatcher implements FileWatcher {
  private readonly cbs = { change: [] as ((p: string) => void)[], create: [] as ((p: string) => void)[], delete: [] as ((p: string) => void)[] };
  onChange(cb: (p: string) => void): void {
    this.cbs.change.push(cb);
  }
  onCreate(cb: (p: string) => void): void {
    this.cbs.create.push(cb);
  }
  onDelete(cb: (p: string) => void): void {
    this.cbs.delete.push(cb);
  }
  dispose(): void {
    this.cbs.change.length = this.cbs.create.length = this.cbs.delete.length = 0;
  }
  fire(kind: 'change' | 'create' | 'delete', absPath: string): void {
    for (const cb of this.cbs[kind]) cb(absPath);
  }
}

/** Open buffers held in memory; "typing" goes through the bound TextBindingCore. */
export class FakeBufferHost implements BufferHost {
  readonly texts = new Map<string, string>();
  readonly bindings = new Map<string, TextBindingCore>();
  private readonly opens: ((p: string) => void)[] = [];
  private readonly closes: ((p: string) => void)[] = [];
  getText(absPath: string): string | null {
    return this.texts.get(absPath) ?? null;
  }
  openPaths(): string[] {
    return [...this.texts.keys()];
  }
  onOpen(cb: (p: string) => void): void {
    this.opens.push(cb);
  }
  onClose(cb: (p: string) => void): void {
    this.closes.push(cb);
  }
  bind(absPath: string, handle: DocHandle<TextDoc>): BufferBinding {
    const buffer = {
      getText: () => this.texts.get(absPath) ?? '',
      applyReplace: async (start: number, end: number, text: string) => {
        const cur = this.texts.get(absPath) ?? '';
        this.texts.set(absPath, cur.slice(0, start) + text + cur.slice(end));
      },
    };
    const core = new TextBindingCore(handle, buffer);
    this.bindings.set(absPath, core);
    core.start();
    return {
      dispose: () => {
        core.dispose();
        this.bindings.delete(absPath);
      },
    };
  }
  dispose(): void {}
  /** The user opens a file in an editor. */
  open(absPath: string, text: string): void {
    this.texts.set(absPath, text);
    for (const cb of this.opens) cb(absPath);
  }
  close(absPath: string): void {
    this.texts.delete(absPath);
    for (const cb of this.closes) cb(absPath);
  }
  /** The user types: replaces the whole buffer text, reported as one splice. */
  type(absPath: string, newText: string): void {
    const cur = this.texts.get(absPath) ?? '';
    const r = computeReplace(cur, newText);
    this.texts.set(absPath, newText);
    if (r) this.bindings.get(absPath)?.onLocalEdit([{ rangeOffset: r.start, rangeLength: r.end - r.start, text: r.text }]);
  }
}

export interface FakeHost extends SyncHost {
  watcher: FakeWatcher;
  buffers: FakeBufferHost;
  notifier: RecordingNotifier;
  log: MemoryLog;
}

export function makeFakeHost(): FakeHost {
  return { log: new MemoryLog(), watcher: new FakeWatcher(), buffers: new FakeBufferHost(), notifier: new RecordingNotifier() };
}

export async function tempDir(prefix = 'ge8-'): Promise<string> {
  return fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), prefix)));
}

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Poll until `check` returns a truthy value, or fail after `timeoutMs`. */
export async function waitFor<T>(check: () => T | Promise<T>, what: string, timeoutMs = 15_000): Promise<NonNullable<T>> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const v = await check();
    if (v) return v as NonNullable<T>;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await sleep(50);
  }
}

export async function readText(p: string): Promise<string | null> {
  return fs.readFile(p, 'utf8').catch(() => null);
}

export type SeedFile = string | Uint8Array;

/**
 * A second client on the hub: seeds a project and edits it the way another
 * collaborator would. Waits for the hub to hold its heads (presence is not
 * delivery; see .claude/rules/hub-client-tests.md) before returning.
 */
export class Seeder {
  readonly repo: Repo;
  index!: DocHandle<IndexDocument>;
  constructor(private readonly hub: TestHub) {
    this.repo = new Repo({ network: [new BrowserWebSocketClientAdapter(hub.url)] });
  }
  async create(files: Record<string, SeedFile>, folders: string[] = []): Promise<string> {
    this.index = this.repo.create<IndexDocument>(createIndexDocumentContent());
    for (const [rel, content] of Object.entries(files)) await this.putFile(rel, content, false);
    if (folders.length) this.index.change((d) => void folders.forEach((f) => ((d.folders ??= {})[f] = true)));
    await this.flush();
    return this.index.documentId;
  }
  async putFile(rel: string, content: SeedFile, flush = true): Promise<DocHandle<unknown>> {
    const handle =
      typeof content === 'string'
        ? this.repo.create<TextDoc>({ text: content })
        : this.repo.create<unknown>({ content, mimeType: 'image/png', hash: 'seed' });
    this.index.change((d) => void (d.files[rel] = handle.documentId));
    if (flush) await this.flush();
    return handle as DocHandle<unknown>;
  }
  async removeFile(rel: string): Promise<void> {
    this.index.change((d) => void delete d.files[rel]);
    await this.flush();
  }
  /** Wait until the hub holds every doc this client holds, at its current heads. */
  async flush(): Promise<void> {
    const ids = [this.index.documentId, ...Object.values(this.index.doc()!.files)];
    for (const id of ids) {
      const h = await this.repo.find(`automerge:${id}` as never);
      if (!(await this.hub.hubHasHeads(id, h.heads(), 15_000))) throw new Error(`hub never received ${id}`);
    }
  }
  async stop(): Promise<void> {
    await this.repo.shutdown().catch(() => {});
  }
}

export async function startHub(): Promise<TestHub> {
  return startTestHub();
}
