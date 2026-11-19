/**
 * A headless peer: the sync core with a Node file watcher, no editor buffers,
 * and a recording notifier. It publishes a folder as a new project or joins an
 * existing one, and keeps the folder in sync. Used by the tests (and later
 * plans) as "the other collaborator" without a VS Code window. A library; no
 * command-line entry yet (detail chosen by the plan author; GE-9 may add one).
 *
 * Plain projects with `publish` and `join`; the git-enabled variants (`publishGit`, `joinAsClone`, `joinAsMirror`) are
 * GE-10's.
 */
import { watch, type FSWatcher } from 'node:fs';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { createIndexDocumentContent, type IndexDocument } from '@quarto/quarto-automerge-schema';
import { writeProjectFiles } from '../core/create-folder';
import type { BufferBinding, BufferHost, FileWatcher, SyncHost } from '../core/host';
import { readProjectMeta, writeProjectMeta, type ProjectMeta } from '../core/meta';
import type { Identity } from '../core/presence';
import { ProjectSync } from '../core/project-sync';
import { findDoc, makeRepo } from '../core/repo';
import { MemoryLog, RecordingNotifier } from '../core/recording';
import { ensureWasm } from '../core/wasm';
import { attachFolder } from '../core/clone/attach-flow';
import type { CloneState } from '../core/clone/clone-sync';
import { publishGit, type GitPublication } from './publish-git';
import type { HubLike } from './owner';

/** `fs.watch` on the folder, reporting absolute paths. */
export class NodeFileWatcher implements FileWatcher {
  private readonly watcher: FSWatcher;
  private readonly cbs = { change: [] as ((p: string) => void)[], create: [] as ((p: string) => void)[], delete: [] as ((p: string) => void)[] };

  constructor(root: string) {
    this.watcher = watch(root, { recursive: true }, (eventType, filename) => {
      if (!filename) return;
      const abs = path.join(root, filename.toString());
      if (eventType === 'change') {
        this.cbs.change.forEach((cb) => cb(abs));
        return;
      }
      // 'rename' covers create, delete, and (on macOS) many plain rewrites; the
      // disk tells which. A file that exists is reported as created and as
      // changed: the sync ignores a create for a file it already tracks, and a
      // change whose text equals the doc's.
      void fs.stat(abs).then(
        () => {
          this.cbs.create.forEach((cb) => cb(abs));
          this.cbs.change.forEach((cb) => cb(abs));
        },
        () => this.cbs.delete.forEach((cb) => cb(abs)),
      );
    });
  }
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
    this.watcher.close();
    this.cbs.change.length = this.cbs.create.length = this.cbs.delete.length = 0;
  }
}

/** A host with no editor: nothing is ever open. */
export class NoBuffers implements BufferHost {
  getText(): null {
    return null;
  }
  isDirty(): boolean {
    return false;
  }
  openPaths(): string[] {
    return [];
  }
  onOpen(): void {}
  onClose(): void {}
  bind(): BufferBinding | null {
    return null;
  }
  dispose(): void {}
}

const IDENTITY: Identity = { userId: 'headless-peer', userName: 'Headless peer', userColor: '#777777' };

export class HeadlessPeer {
  private constructor(
    readonly dir: string,
    readonly indexDocId: string,
    readonly host: SyncHost & { log: MemoryLog; notifier: RecordingNotifier },
    private readonly sync: ProjectSync,
  ) {}

  /** Turn `dir` into a new project on `serverUrl`: every non-ignored file becomes a doc. */
  static async publish(dir: string, serverUrl: string, identity: Identity = IDENTITY): Promise<HeadlessPeer> {
    await ensureWasm();
    await fs.mkdir(dir, { recursive: true });
    // An empty index in local storage; the sync's startup reconciliation then
    // turns each file on disk into a doc and an index entry.
    const repo = makeRepo(dir, null);
    let indexDocId: string;
    try {
      indexDocId = repo.create<IndexDocument>(createIndexDocumentContent()).documentId;
      await repo.flush();
    } finally {
      await repo.shutdown().catch(() => {});
    }
    await writeProjectMeta(dir, { indexDocId, syncServer: serverUrl, createdAt: new Date().toISOString() });
    return HeadlessPeer.start(dir, identity);
  }

  /** Join the project `indexDocId` on `serverUrl` into `dir`. */
  static async join(serverUrl: string, indexDocId: string, dir: string, identity: Identity = IDENTITY): Promise<HeadlessPeer> {
    await ensureWasm();
    await fs.mkdir(dir, { recursive: true });
    const log = new MemoryLog();
    const repo = makeRepo(dir, serverUrl);
    try {
      const index = await findDoc<IndexDocument>(repo, indexDocId);
      await writeProjectFiles(repo, dir, index.doc(), log);
      await repo.flush();
    } finally {
      await repo.shutdown().catch(() => {});
    }
    await writeProjectMeta(dir, { indexDocId, syncServer: serverUrl, createdAt: new Date().toISOString() });
    return HeadlessPeer.start(dir, identity);
  }

  /** Publish the tracked files of the git repository at `dir` as a version 4 project (test code; GE-12 builds the real owner). */
  static publishGit(dir: string, serverUrl: string, hub?: HubLike): Promise<GitPublication> {
    return publishGit(dir, serverUrl, hub);
  }

  /** Attach the git clone at `dir` to project `indexDocId` as a clone peer. Throws the refusal message when the check fails. */
  static async joinAsClone(dir: string, serverUrl: string, indexDocId: string, identity: Identity = IDENTITY): Promise<HeadlessPeer> {
    await ensureWasm();
    const r = await attachFolder({ dir, serverUrl, indexDocId });
    if (!r.ok) throw new Error(`attach refused (${r.reason}): ${r.message}`);
    return HeadlessPeer.start(r.projectRoot, identity);
  }

  /** Join the project into `dir` as a mirror, whatever the project's shape (the way out of a refused attach). */
  static async joinAsMirror(dir: string, serverUrl: string, indexDocId: string, identity: Identity = IDENTITY): Promise<HeadlessPeer> {
    await ensureWasm();
    await fs.mkdir(dir, { recursive: true });
    const log = new MemoryLog();
    const repo = makeRepo(dir, serverUrl);
    try {
      const index = await findDoc<IndexDocument>(repo, indexDocId);
      await writeProjectFiles(repo, dir, index.doc(), log);
      await repo.flush();
    } finally {
      await repo.shutdown().catch(() => {});
    }
    await writeProjectMeta(dir, { indexDocId, syncServer: serverUrl, createdAt: new Date().toISOString(), mode: 'mirror' });
    return HeadlessPeer.start(dir, identity);
  }

  private static async start(dir: string, identity: Identity): Promise<HeadlessPeer> {
    const meta = (await readProjectMeta(dir)) as ProjectMeta;
    const host = { log: new MemoryLog(), watcher: new NodeFileWatcher(dir), buffers: new NoBuffers(), notifier: new RecordingNotifier() };
    const sync = new ProjectSync(dir, meta, identity, host);
    await sync.start();
    return new HeadlessPeer(dir, meta.indexDocId, host, sync);
  }

  /** Clone mode: HEAD, the attached docs, and the detached files; null in other modes. */
  cloneState(): CloneState | null {
    return this.sync.cloneState();
  }

  /** The index and file docs this peer holds, with their heads. */
  inventory(): { docId: string; heads: string[] }[] {
    return this.sync.inventory();
  }

  async stop(): Promise<void> {
    await this.sync.dispose();
  }
}
