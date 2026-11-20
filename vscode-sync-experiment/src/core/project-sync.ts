/**
 * Project mode: a real folder on disk mirrored from a Quarto Hub project.
 *
 * Ownership rule: the editor owns files that are open in a buffer (they get a
 * binding, and the editor's autosave writes them to disk); this module owns
 * every other file and writes remote changes straight to disk.
 *
 * `.quarto-hub/` holds device-local state and is never synced:
 *   project.json   index doc id + sync server
 *   automerge/     automerge-repo storage: the index and every file document,
 *                  with full history. This is what makes startup reconciliation
 *                  a proper merge: edits made on disk while offline are applied
 *                  as changes on top of the last-synced document state, so they
 *                  merge with whatever collaborators did meanwhile.
 *   events.jsonl   append-only log of file creation/deletion (hub or local)
 *
 * Nothing else is written into the project (no .gitignore etc.): any file we
 * add outside the ignore list would itself be synced to the hub.
 *
 * This is the vscode-free core (GE-8): the host supplies the file watcher, the
 * open buffers, a notifier, and a logger (see host.ts).
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { createHash } from 'node:crypto';
import { updateText, type DocHandle, type Repo } from '@automerge/automerge-repo/slim';
import {
  isBinaryDocument,
  isBinaryExtension,
  isTextDocument,
  inferMimeType,
  type BinaryDocumentContent,
  type IndexDocument,
} from '@quarto/quarto-automerge-schema';
import { writeFileDoc, type FileDoc } from './create-folder';
import { excludeMetaDir } from './git-exclude';
import { EditorGate, type GateBuffer, type GateClock } from './editor-gate';
import { Follower, type Clock } from './follower';
import type { BufferBinding, SyncHost } from './host';
import { META_DIR, isIgnoredPath } from './ignore';
import { META_FILE, type ProjectMeta, type ProjectMode } from './meta';
import type { Identity } from './presence';
import { asString, movedNotice, normalizeText, type ControllerProbe } from '@quarto/quarto-git-epochs';
import { CloneRefusedError, CloneSync, docKey, type CloneEvent, type CloneState, type DiskContent } from './clone/clone-sync';
import { NOOP_POINTER_WRITER, type PointerWriter } from './clone/pointer-writer';
import { OwnerPointerWriter } from './origination/owner-writer';
import { RegistryClient } from './clone/registry-client';
import type { RegistryView } from '@quarto/quarto-git-epochs';
import { findDoc, makeRepo, webSocketAdapter } from './repo';
import { UnsafePathError, safeJoin } from './safe-path';
import type { TextDoc } from './text-binding';

const MODE_LABEL = { mirror: 'Quarto Hub: mirror', 'editor-only': 'Quarto Hub: editor only', clone: 'Quarto Hub: clone' } as const;

/** Knobs the tests (and later plans) turn; every one has a product default. */
export interface SyncOptions {
  /** How long a follower swap may wait for the new doc before it reports `unavailable` (G32: 5000). */
  swapTimeoutMs?: number;
  /** Timers for the swap coordinator; default: the real ones. */
  clock?: Clock;
  /**
   * Editor-only mode: the git gate's probe and clock. Default: a real git probe on the project
   * directory (no gate when it is not inside a git repository) and the real clock.
   */
  gate?: { probe: ControllerProbe; clock: GateClock; flipWaitMs?: number };
  /** Editor-only: how long to wait for the hub's flip after the gate reopens when `gate` is not given (G32: 10 000). */
  flipWaitMs?: number;
  /** Clone mode: the controller's clock and probe (default: real ones) and the owner hook (default: none). */
  clone?: { clock?: GateClock; probe?: ControllerProbe; pointer?: PointerWriter };
}

export class ProjectSync {
  readonly mode: ProjectMode;
  private clone: CloneSync | null = null;
  /** The content key last written to each path by this engine (the clone gate ignores the file's own event). */
  private follower!: Follower;
  private gate: EditorGate | null = null;
  private repo!: Repo;
  private index!: DocHandle<IndexDocument>;
  private readonly handles = new Map<string, DocHandle<FileDoc>>(); // rel path -> handle
  private readonly fileListeners = new Map<string, () => void>();
  /** Open buffers bound to a doc: absolute path -> binding. */
  private readonly bound = new Map<string, BufferBinding>();
  /** Explicit (empty) folders last seen in the index, for hub-side removals. */
  private knownFolders = new Set<string>();
  /** Subfolders with their own `.quarto-hub/project.json`: not part of this project (G23). */
  private readonly boundSubfolders = new Set<string>();
  private readonly cleanups: (() => void)[] = [];
  /** Rejected keys already reported, so each index pass does not repeat the log lines. */
  private readonly reportedRejects = new Set<string>();

  constructor(
    readonly root: string,
    private readonly meta: ProjectMeta,
    private readonly identity: Identity,
    private readonly host: SyncHost,
    private readonly options: SyncOptions = {},
  ) {
    this.mode = meta.mode ?? 'mirror';
  }

  private get writesDisk(): boolean {
    return this.mode === 'mirror' || this.mode === 'clone';
  }

  private get log() {
    return this.host.log;
  }

  async start(): Promise<void> {
    // Phase 1, offline: load the last-synced state from local storage and
    // fold in whatever changed on disk since. Doing this before going online
    // means offline edits become changes rooted at the state the disk
    // actually reflected, which is what lets Automerge merge them properly.
    // A freshly attached clone has no local store yet, so it starts online; its startup rule never merges disk into a
    // doc without the registry's answer anyway (G10).
    this.repo = makeRepo(this.root, this.mode === 'clone' ? this.meta.syncServer : null);
    this.index = await findDoc<IndexDocument>(this.repo, this.meta.indexDocId);
    if (this.mode !== 'clone') {
      for (const [rel, docId] of Object.entries(this.index.doc()?.files ?? {})) {
        if ((await this.safePath(rel)) === null) continue;
        this.track(rel, await findDoc<FileDoc>(this.repo, docId));
      }
      if (this.writesDisk) await this.reconcileDisk();
    }
    this.knownFolders = new Set(Object.keys(this.index.doc()?.folders ?? {}));

    // Phase 2: go online. From here on, changes flow both ways live.
    if (this.mode !== 'clone') this.repo.networkSubsystem.addNetworkAdapter(webSocketAdapter(this.meta.syncServer));
    const onIndex = () => this.background('index sync', () => this.syncIndex());
    this.index.on('change', onIndex);
    this.cleanups.push(() => this.index.off('change', onIndex));

    this.follower = new Follower(
      {
        currentHandle: (rel) => this.handles.get(rel),
        loadDoc: (docId, signal) => this.loadDoc(docId, signal),
        rebind: (rel, newHandle, oldHandle) => this.rebind(rel, newHandle, oldHandle),
        isOpen: (rel) => this.host.buffers.getText(this.absOf(rel)) !== null,
        onSwapped: (rel) => this.gate?.noteSwapped(rel),
        notifier: this.host.notifier,
        log: (line) => this.log.appendLine(line),
        clock: this.options.clock ?? { setTimeout, clearTimeout },
      },
      { swapTimeoutMs: this.options.swapTimeoutMs },
    );
    this.cleanups.push(() => this.follower.dispose());
    if (this.mode === 'editor-only') await this.startGate();
    const { watcher, buffers } = this.host;
    buffers.onOpen((p) => this.bind(p));
    buffers.onClose((p) => this.unbind(p));
    if (this.mode === 'clone') await this.startClone();
    if (this.writesDisk) {
      watcher.onChange((p) => this.background('disk change', () => this.onDiskChange(p)));
      watcher.onCreate((p) => this.background('disk create', () => this.onDiskCreate(p)));
      watcher.onDelete((p) => this.background('disk delete', () => this.onDiskDelete(p)));
    }
    this.cleanups.push(() => watcher.dispose(), () => buffers.dispose());
    buffers.openPaths().forEach((p) => this.bind(p));
    this.log.appendLine(`project sync running: ${this.root} (${this.handles.size} files)`);
    this.updateStatus(); // the persistent status bar item: the mode, plus a clone's detached count
    this.host.notifier.status(`$(sync) Quarto Hub: syncing ${path.basename(this.root)}`, 5000);
  }

  /** Work started by an event handler and not yet finished: `dispose` waits for it. */
  private readonly inFlight = new Set<Promise<void>>();

  /**
   * Run event-driven work without letting a failure escape as an unhandled rejection. A failure is logged (a
   * `realpath` of a folder that is being deleted fails with EPERM on Windows, which says nothing about existence), and
   * `dispose` waits for the work so nothing touches the folder after the engine has stopped.
   */
  private background(what: string, task: () => Promise<void>): void {
    const p = task()
      .catch((err: unknown) => this.log.appendLine(`${what} failed: ${err instanceof Error ? err.message : String(err)}`))
      .finally(() => this.inFlight.delete(p));
    this.inFlight.add(p);
  }

  async dispose(): Promise<void> {
    await this.clone?.dispose().catch(() => {});
    this.cleanups.forEach((c) => c());
    this.cleanups.length = 0;
    await Promise.all([...this.inFlight]);
    this.host.notifier.status(null); // clears the mode item
    for (const rel of [...this.handles.keys()]) this.untrack(rel);
    for (const abs of [...this.bound.keys()]) this.unbind(abs);
    await this.repo.flush().catch(() => {});
    await this.repo.shutdown().catch(() => {});
  }

  /** The index and every tracked file doc with their current heads (URL-encoded). */
  inventory(): { docId: string; heads: string[] }[] {
    return [this.index, ...this.handles.values()].map((h) => ({ docId: h.documentId, heads: h.heads() as string[] }));
  }

  // --- clone mode (GE-10) -------------------------------------------------------

  private async startClone(): Promise<void> {
    const { createGitProbe } = await import('@quarto/quarto-git-epochs/probe');
    const probe = createGitProbe({ cwd: this.root });
    await excludeMetaDir(this.root).catch(() => false);
    const registry = new RegistryClient(this.repo, this.index);
    registry.log = (line) => this.log.appendLine(line);
    const clone = new CloneSync(this.cloneHost(), {
      identity: this.meta.repo,
      probe,
      controllerProbe: this.options.clone?.probe,
      clock: this.options.clone?.clock ?? { now: () => Date.now(), setTimeout, clearTimeout },
      registry,
      pointer: this.options.clone?.pointer ?? this.ownerPointerWriter(),
      isIgnored: (rel) => isIgnoredPath(rel, this.boundSubfolders),
    });
    try {
      await clone.start();
    } catch (err) {
      await clone.dispose().catch(() => {});
      if (err instanceof CloneRefusedError) this.host.notifier.error(err.message);
      throw err;
    }
    this.clone = clone;
    this.updateStatus();
  }

  /**
   * The owner hook (GE-12, G16): only the folder whose `ownerId` equals the index's `owner` key writes the pointer. A
   * hub-owned project has no `owner` key, and a second clone with the same user id has a different folder id.
   */
  private ownerPointerWriter(): PointerWriter {
    const owner = (this.index.doc() as { owner?: unknown } | undefined)?.owner;
    if (!this.meta.ownerId || owner == null || asString(owner) !== this.meta.ownerId) return NOOP_POINTER_WRITER;
    return new OwnerPointerWriter({ repo: this.repo, index: this.index, log: (l) => this.log.appendLine(l) });
  }

  private cloneHost() {
    return {
      root: this.root,
      repo: this.repo,
      notifier: this.host.notifier,
      log: (line: string) => this.log.appendLine(line),
      handleOf: (rel: string) => this.handles.get(rel),
      attachedPaths: () => [...this.handles.keys()],
      switchTo: async (rel: string, handle: DocHandle<FileDoc>, o: { writeDisk: boolean }) => {
        const old = this.handles.get(rel);
        if (old?.documentId !== handle.documentId) {
          this.bound.get(this.absOf(rel))?.sendMoved?.(); // the moved message goes out on the old doc first (G25)
          this.untrack(rel);
          this.track(rel, handle);
        }
        const doc = handle.doc();
        if (o.writeDisk && doc) await this.writeDoc(rel, doc);
      },
      setInert: (rel: string, inert: boolean) => this.bound.get(this.absOf(rel))?.setInert?.(inert),
      createDoc: (rel: string, disk: DiskContent) =>
        disk.kind === 'binary'
          ? this.repo.create<FileDoc>({ content: disk.bytes, mimeType: inferMimeType(rel), hash: disk.key } as FileDoc)
          : this.repo.create<FileDoc>({ text: this.host.buffers.getText(this.absOf(rel)) ?? disk.text } as FileDoc),
      readDisk: async (rel: string): Promise<DiskContent | null> => {
        const abs = this.absOf(rel);
        try {
          if (isBinaryExtension(rel)) {
            const bytes = new Uint8Array(await fs.readFile(abs));
            return { kind: 'binary', bytes, key: createHash('sha256').update(bytes).digest('hex') };
          }
          const text = await fs.readFile(abs, 'utf8');
          return { kind: 'text', text, key: normalizeText(text) };
        } catch {
          return null;
        }
      },
      writeBytes: async (rel: string, bytes: Uint8Array) => {
        const abs = await this.safePath(rel);
        if (abs === null) return;
        this.clone?.noteWritten(rel, isBinaryExtension(rel) ? createHash('sha256').update(bytes).digest('hex') : normalizeText(Buffer.from(bytes).toString('utf8')));
        await fs.writeFile(abs, bytes);
      },
      mergeDiskIntoDoc: (rel: string) => this.mergeDiskIntoDoc(rel),
      followPointer: async (untracked: (rel: string) => boolean) => {
        for (const [rel, docId] of Object.entries(this.index.doc()?.files ?? {})) {
          if (!untracked(rel) || this.handles.has(rel) || (await this.safePath(rel)) === null) continue;
          if (!(await fs.stat(this.absOf(rel)).then((s) => s.isFile(), () => false))) continue; // never create (G14)
          this.track(rel, await findDoc<FileDoc>(this.repo, docId));
          await this.mergeDiskIntoDoc(rel);
        }
      },
      loadDoc: (docId: string, signal: AbortSignal) => this.loadDoc(docId, signal),
      stateChanged: () => this.updateStatus(),
      logEvent: (event: CloneEvent, rel: string, extra: { from?: string; to?: string; reason?: string }) =>
        this.logEvent(event, rel, 'local', extra),
    };
  }

  /** The persistent status bar item: the mode, plus the clone's detached and superseded counts. */
  private updateStatus(): void {
    const st = this.clone?.state();
    const parts: string[] = [MODE_LABEL[this.mode]];
    if (st && st.detached.length > 0) parts.push(`${st.detached.length} detached`);
    if (st && st.superseded.length > 0) parts.push(`${st.superseded.length} superseded`);
    this.host.notifier.status(parts.join(' · '));
  }

  /** Clone mode: HEAD, the gate, the attached docs, and the detached and superseded files. Null in other modes. */
  cloneState(): CloneState | null {
    return this.clone?.state() ?? null;
  }

  /** Clone mode: the registry as this peer sees it. */
  registryView(): RegistryView {
    return this.clone?.registryView() ?? { entries: [], retired: [] };
  }

  /** The text of the doc attached to `rel`, or undefined. */
  docText(rel: string): string | undefined {
    const doc = this.handles.get(rel)?.doc();
    return isTextDocument(doc) ? doc.text : undefined;
  }

  /** Clone mode: take the shared version of a detached file ("Adopt", asking first when the file has local changes). */
  adopt(rel: string): Promise<boolean> {
    return this.clone?.adopt(rel) ?? Promise.resolve(false);
  }

  stayDetached(rel: string): Promise<void> {
    return this.clone?.stayDetached(rel) ?? Promise.resolve();
  }

  // --- helpers -----------------------------------------------------------

  /** Relative slash path if `abs` is a non-ignored file inside this project, else null. */
  private relOf(abs: string): string | null {
    const rel = path.relative(this.root, abs).split(path.sep).join('/');
    if (rel.startsWith('..') || path.isAbsolute(rel) || isIgnoredPath(rel, this.boundSubfolders)) return null;
    return rel;
  }

  private absOf(rel: string): string {
    return path.join(this.root, rel);
  }

  private track(rel: string, handle: DocHandle<FileDoc>): void {
    this.handles.set(rel, handle);
    const listener = () => this.background('doc change', () => this.onDocChange(rel));
    handle.on('change', listener);
    this.fileListeners.set(rel, listener);
    // The file may already be open in an editor. For a file created in the
    // editor, the open event fires before the file watcher reports the new
    // file, so bind() ran when no handle existed for this path yet and did
    // nothing; without a binding there is no presence, so cursors never
    // showed up until the next activation. Bind now that the handle exists.
    if (this.host.buffers.getText(this.absOf(rel)) !== null) this.bind(this.absOf(rel));
  }

  private untrack(rel: string): void {
    const handle = this.handles.get(rel);
    const listener = this.fileListeners.get(rel);
    if (handle && listener) handle.off('change', listener);
    this.handles.delete(rel);
    this.fileListeners.delete(rel);
    // A binding to this handle is stale once the path no longer maps to it.
    this.unbind(this.absOf(rel));
  }

  /**
   * Validate a doc-derived path (G36). A rejected key is skipped and logged
   * (an event line and the output channel), never thrown.
   */
  private async safePath(rel: string): Promise<string | null> {
    try {
      return await safeJoin(this.root, rel);
    } catch (err) {
      if (!(err instanceof UnsafePathError)) throw err;
      if (!this.reportedRejects.has(rel)) {
        this.reportedRejects.add(rel);
        this.log.appendLine(`skipped index key: ${err.message}`);
        await this.logEvent('rejected', rel, 'hub');
      }
      return null;
    }
  }

  private async logEvent(
    event: 'created' | 'deleted' | 'edited-offline' | 'rejected' | 'flipped' | CloneEvent,
    rel: string,
    source: 'hub' | 'local',
    extra: { from?: string; to?: string; reason?: string } = {},
  ): Promise<void> {
    const line = JSON.stringify({ at: new Date().toISOString(), event, path: rel, source, ...extra }) + '\n';
    // A failed append is reported, never thrown (an event is a record, not a step the sync depends on) and
    // never silent: a missing event with no trace cost a Windows run its diagnosis (GE-16a B4).
    await fs.appendFile(path.join(this.root, META_DIR, 'events.jsonl'), line).catch((err: unknown) => {
      this.log.appendLine(`event log append failed (${event} ${rel}): ${(err as Error).message}`);
    });
    this.log.appendLine(`${source}: ${event} ${rel}`);
  }

  /**
   * Env-gated diagnostics (`QUARTO_HUB_DEBUG=1`): the path form each watcher event arrives in, the relative
   * key it maps to, and the clone gate's state. Written to the log and to stderr, so a test run that fails
   * on a platform we cannot observe (Windows) leaves the cause in its output.
   */
  private debugDisk(kind: 'change' | 'create', abs: string, rel: string | null): void {
    if (!process.env.QUARTO_HUB_DEBUG) return;
    const c = this.clone;
    const gate = c
      ? ` closed=${c.isClosed()} inert=${rel ? c.isInert(rel) : '-'} removed=${rel ? c.isRemoved(rel) : '-'} gate=${c.state().gate}`
      : '';
    const line = `debug: disk ${kind} ${abs} rel=${rel ?? 'null'} bound=${rel ? this.isBound(rel) : '-'} tracked=${rel ? this.handles.has(rel) : '-'}${gate}`;
    this.log.appendLine(line);
    process.stderr.write(line + '\n');
  }

  private bind(abs: string): void {
    const rel = this.relOf(abs);
    if (!rel || this.bound.has(abs)) return;
    const handle = this.handles.get(rel);
    if (!handle || !isTextDocument(handle.doc())) return;
    const binding = this.host.buffers.bind(abs, handle as DocHandle<TextDoc>, this.identity, {
      // The Binding keeps the handle it was created with; the base doc id tells the follower
      // whether an edit landed in a doc that is being, or has been, replaced.
      onLocalEdit: () => this.follower?.noteLocalEdit(rel, handle.documentId),
      suspended: () => (this.gate?.isClosed() ?? false) || (this.clone?.isClosed() ?? false),
      onMoved: (name) => this.host.notifier.status(movedNotice(name), 3000),
    });
    if (binding) {
      this.bound.set(abs, binding);
      if (this.clone?.isInert(rel)) binding.setInert?.(true);
    }
  }

  private unbind(abs: string): void {
    this.bound.get(abs)?.dispose();
    this.bound.delete(abs);
  }

  private isBound(rel: string): boolean {
    return this.bound.has(this.absOf(rel));
  }

  private async listDiskFiles(dir = this.root, out: string[] = []): Promise<string[]> {
    for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
      const abs = path.join(dir, entry.name);
      const rel = path.relative(this.root, abs).split(path.sep).join('/');
      if (isIgnoredPath(rel, this.boundSubfolders)) continue;
      if (entry.isDirectory()) {
        // A subfolder bound to a project of its own is not part of this one.
        if (await fs.stat(path.join(abs, META_DIR, META_FILE)).then(() => true, () => false)) {
          this.boundSubfolders.add(rel);
          continue;
        }
        await this.listDiskFiles(abs, out);
      } else if (entry.isFile()) out.push(rel);
    }
    return out;
  }

  // --- startup reconciliation ----------------------------------------------

  private async reconcileDisk(): Promise<void> {
    const onDisk = new Set(await this.listDiskFiles());
    for (const [rel, handle] of this.handles) {
      if (!onDisk.has(rel)) {
        this.index.change((d) => void delete d.files[rel]);
        this.untrack(rel);
        await this.logEvent('deleted', rel, 'local');
        continue;
      }
      await this.mergeDiskIntoDoc(rel);
    }
    for (const rel of onDisk) if (!this.handles.has(rel)) await this.createFromDisk(rel);
  }

  /** Fold what changed on disk while offline into the attached doc, as changes on top of its last-synced state. */
  private async mergeDiskIntoDoc(rel: string): Promise<void> {
    const handle = this.handles.get(rel);
    const doc = handle?.doc();
    if (!handle) return;
    if (isTextDocument(doc)) {
      const diskText = await fs.readFile(this.absOf(rel), 'utf8');
      if (diskText !== doc.text) {
        (handle as DocHandle<TextDoc>).change((d) => updateText(d, ['text'], diskText));
        await this.logEvent('edited-offline', rel, 'local');
      }
    } else if (isBinaryDocument(doc)) {
      // Local wins for binaries too: a differing file on disk replaces the
      // hub copy wholesale (no character merge is meaningful for bytes).
      const content = new Uint8Array(await fs.readFile(this.absOf(rel)));
      const hash = createHash('sha256').update(content).digest('hex');
      if (hash !== doc.hash) {
        (handle as DocHandle<BinaryDocumentContent>).change((d) => {
          d.content = content;
          d.hash = hash;
          d.mimeType = inferMimeType(rel);
        });
        await this.logEvent('edited-offline', rel, 'local');
      }
    }
  }

  // --- hub -> disk -------------------------------------------------------------

  /**
   * The owner's pointer moved. A path with no tracked handle is a new entry
   * (written to disk in a mirror, never in editor-only). A path whose doc id
   * differs is a version switch and goes through the swap coordinator (G26):
   * the old doc stays bound until the new one has loaded, edits typed meanwhile
   * stay with the old doc and are counted, and one aggregate notice follows.
   * A path that left the index is removed (the file too, in a mirror).
   */
  private async syncIndex(): Promise<void> {
    const files = this.index.doc()?.files ?? {};
    const clone = this.clone;
    if (this.mode === 'clone' && !clone) return; // the clone's startup has not finished: it reads the index itself
    for (const rel of [...this.handles.keys()]) {
      if (clone?.isTracked(rel)) continue; // a tracked file ignores the pointer and its removals (G14, G13)
      if (!(rel in files)) {
        this.untrack(rel);
        const abs = await this.safePath(rel);
        // A clone never deletes a file from the user's working tree because the pointer dropped it.
        if (abs !== null && this.writesDisk && !clone) await fs.rm(abs, { force: true });
        await this.logEvent('deleted', rel, 'hub');
      }
    }
    for (const [rel, docId] of Object.entries(files)) {
      if ((await this.safePath(rel)) === null) continue;
      if (clone?.isTracked(rel)) continue;
      if (clone && !this.handles.has(rel) && !(await fs.stat(this.absOf(rel)).then((s) => s.isFile(), () => false))) continue; // the pointer never creates an untracked file (G14)
      const existing = this.handles.get(rel);
      if (existing) {
        this.follower.indexChanged(rel, docId); // a no-op when the doc is the one already bound
        continue;
      }
      const handle = await findDoc<FileDoc>(this.repo, docId);
      if (this.handles.has(rel)) continue; // another pass got here first
      this.track(rel, handle);
      const doc = handle.doc();
      if (doc && this.writesDisk) await this.writeDoc(rel, doc);
      await this.logEvent('created', rel, 'hub');
    }
    if (clone) return; // git owns the folders of a clone
    const folders = new Set(Object.keys(this.index.doc()?.folders ?? {}));
    for (const folder of folders) {
      if (this.knownFolders.has(folder)) continue;
      const abs = await this.safePath(folder);
      if (abs !== null) await fs.mkdir(abs, { recursive: true }).catch(() => {});
    }
    for (const folder of this.knownFolders) {
      // Only remove if still empty: a folder with files is implied by them.
      if (folders.has(folder)) continue;
      const abs = await this.safePath(folder);
      if (abs !== null) await fs.rmdir(abs).catch(() => {});
    }
    this.knownFolders = folders;
  }

  /** Editor-only: the git gate. No gate (always open) when the folder is not inside a git repository. */
  private async startGate(): Promise<void> {
    let gate = this.options.gate;
    if (!gate) {
      const { createGitProbe } = await import('@quarto/quarto-git-epochs/probe');
      await excludeMetaDir(this.root).catch(() => false); // keep .quarto-hub/ out of `git status`
      const probe = createGitProbe({ cwd: this.root });
      await probe.ready().catch(() => {}); // not a repository: the controller goes off at init
      gate = { probe, clock: { now: () => Date.now(), setTimeout, clearTimeout } };
    }
    this.gate = new EditorGate({
      probe: gate.probe,
      clock: gate.clock,
      notifier: this.host.notifier,
      log: (line) => this.log.appendLine(line),
      flipWaitMs: gate.flipWaitMs ?? this.options.flipWaitMs,
      inspect: () => this.inspectBuffers(),
      reconcile: () => this.bound.forEach((b) => b.reconcile?.()),
    });
    await this.gate.start();
    this.cleanups.push(() => this.gate?.dispose());
  }

  private inspectBuffers(): GateBuffer[] {
    const out: GateBuffer[] = [];
    for (const abs of this.bound.keys()) {
      const rel = this.relOf(abs);
      if (!rel) continue;
      const doc = this.handles.get(rel)?.doc();
      out.push({
        rel,
        text: this.host.buffers.getText(abs),
        docText: isTextDocument(doc) ? doc.text : undefined,
        dirty: this.host.buffers.isDirty(abs),
      });
    }
    return out;
  }

  /**
   * Load the doc a version switch points at. The server answers "unavailable"
   * at once for a doc it has not received yet (the owner may still be
   * uploading it), so keep asking until the swap's signal aborts: the swap
   * timeout (5 s, G32) is what decides the doc is unavailable.
   */
  private async loadDoc(docId: string, signal: AbortSignal): Promise<DocHandle<FileDoc>> {
    for (;;) {
      try {
        return await findDoc<FileDoc>(this.repo, docId, signal);
      } catch (err) {
        if (signal.aborted) throw err;
        await new Promise<void>((resolve) => {
          const t = setTimeout(resolve, 150);
          signal.addEventListener('abort', () => (clearTimeout(t), resolve()), { once: true });
        });
        if (signal.aborted) throw err;
      }
    }
  }

  /**
   * The swap's host step: the path now maps to `newHandle`. The moved message
   * goes out on the old doc before the old Binding is disposed (G25).
   */
  private async rebind(rel: string, newHandle: DocHandle<FileDoc>, oldHandle: DocHandle<FileDoc> | undefined): Promise<void> {
    const abs = this.absOf(rel);
    this.bound.get(abs)?.sendMoved?.();
    this.untrack(rel); // disposes the old Binding
    this.track(rel, newHandle); // binds an open buffer to the new doc
    const doc = newHandle.doc();
    if (doc && this.writesDisk) await this.writeDoc(rel, doc);
    await this.logEvent('flipped', rel, 'hub', { from: oldHandle?.documentId, to: newHandle.documentId });
  }

  // PINNED by project-sync.test.ts, gated by GE-10: a doc change rewrites a
  // file that is missing on disk.
  private async onDocChange(rel: string): Promise<void> {
    if (!this.writesDisk) return; // editor-only: the hub owns the disk
    if (this.clone) {
      // A clone writes a doc to disk only while the file is attached, not held, and present at HEAD (G13, G27).
      if (!this.clone.mayWriteDisk(rel)) {
        if (this.clone.isInert(rel)) void this.clone.maybeReattach(rel);
        return;
      }
    }
    if (this.isBound(rel)) return; // the binding + autosave own this file
    const doc = this.handles.get(rel)?.doc();
    if (!doc) return;
    if (isTextDocument(doc)) {
      const current = await fs.readFile(this.absOf(rel), 'utf8').catch(() => null);
      if (current === doc.text) return;
    }
    await this.writeDoc(rel, doc);
  }

  /** Write a doc to disk, remembering the content so the clone gate recognizes the file's own event. */
  private async writeDoc(rel: string, doc: FileDoc): Promise<void> {
    const key = docKey(doc);
    if (key !== undefined) this.clone?.noteWritten(rel, key);
    await writeFileDoc(this.root, rel, doc);
  }

  // --- disk -> hub ------------------------------------------------------------

  private async onDiskChange(abs: string): Promise<void> {
    const rel = this.relOf(abs);
    this.debugDisk('change', abs, rel);
    if (rel) void this.clone?.noteFsEvent(rel);
    if (!rel) return;
    if (this.clone) {
      if (this.clone.isInert(rel)) return void this.clone.maybeReattach(rel);
      if (this.clone.isClosed() || this.clone.isRemoved(rel)) return; // git is working: this is its rewrite, not an edit
    }
    if (this.isBound(rel)) return;
    const handle = this.handles.get(rel);
    const doc = handle?.doc();
    if (!handle || !isTextDocument(doc)) return;
    const diskText = await fs.readFile(abs, 'utf8').catch(() => null);
    if (diskText === null || diskText === doc.text) return; // our own write echoing back
    (handle as DocHandle<TextDoc>).change((d) => updateText(d, ['text'], diskText));
  }

  private async onDiskCreate(abs: string): Promise<void> {
    const rel = this.relOf(abs);
    this.debugDisk('create', abs, rel);
    if (rel) void this.clone?.noteFsEvent(rel);
    // A clone shares a new file when git adds it (arrival step 1), not when it appears on disk.
    if (!rel || this.handles.has(rel) || this.clone) return;
    const stat = await fs.stat(abs).catch(() => null);
    if (stat?.isDirectory()) {
      // Empty folders are only representable via the index's explicit
      // `folders` set (folders implied by file paths are never listed).
      if (this.index.doc()?.folders?.[rel]) return;
      this.index.change((d) => void ((d.folders ??= {})[rel] = true));
      await this.logEvent('created', `${rel}/`, 'local');
      return;
    }
    if (!stat?.isFile()) return;
    await this.createFromDisk(rel);
  }

  private async createFromDisk(rel: string): Promise<void> {
    const abs = this.absOf(rel);
    let handle: DocHandle<FileDoc>;
    if (isBinaryExtension(rel)) {
      const content = new Uint8Array(await fs.readFile(abs));
      const hash = createHash('sha256').update(content).digest('hex');
      handle = this.repo.create<FileDoc>({ content, mimeType: inferMimeType(rel), hash } as FileDoc);
    } else {
      // If the file is already open, the buffer is the freshest copy: with
      // autosave there can be keystrokes not yet on disk, and the binding
      // that track() attaches would otherwise roll them back to the doc.
      const open = this.host.buffers.getText(abs);
      handle = this.repo.create<FileDoc>({ text: open ?? (await fs.readFile(abs, 'utf8')) } as FileDoc);
    }
    this.track(rel, handle);
    this.index.change((d) => void (d.files[rel] = handle.documentId));
    await this.logEvent('created', rel, 'local');
  }

  private async onDiskDelete(abs: string): Promise<void> {
    const rel = this.relOf(abs);
    if (rel) void this.clone?.noteFsEvent(rel);
    if (!rel) return;
    if (this.clone && (this.clone.isClosed() || this.clone.isRemoved(rel) || this.clone.isInert(rel))) return; // git removed it (G13)
    // A deleted directory arrives as one event for the directory itself.
    const gone = [...this.handles.keys()].filter((p) => p === rel || p.startsWith(`${rel}/`));
    const goneFolders = Object.keys(this.index.doc()?.folders ?? {}).filter((p) => p === rel || p.startsWith(`${rel}/`));
    if (gone.length === 0 && goneFolders.length === 0) return;
    this.index.change((d) => {
      for (const p of gone) delete d.files[p];
      for (const p of goneFolders) delete d.folders?.[p];
    });
    for (const p of goneFolders) await this.logEvent('deleted', `${p}/`, 'local');
    for (const p of gone) {
      this.untrack(p);
      await this.logEvent('deleted', p, 'local');
    }
  }
}
