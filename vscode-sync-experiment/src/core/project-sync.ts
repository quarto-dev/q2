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
import type { BufferBinding, SyncHost } from './host';
import { META_DIR, isIgnoredPath } from './ignore';
import { META_FILE, type ProjectMeta } from './meta';
import type { Identity } from './presence';
import { findDoc, makeRepo, webSocketAdapter } from './repo';
import { UnsafePathError, safeJoin } from './safe-path';
import type { TextDoc } from './text-binding';

export class ProjectSync {
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
  ) {}

  private get log() {
    return this.host.log;
  }

  async start(): Promise<void> {
    // Phase 1, offline: load the last-synced state from local storage and
    // fold in whatever changed on disk since. Doing this before going online
    // means offline edits become changes rooted at the state the disk
    // actually reflected, which is what lets Automerge merge them properly.
    this.repo = makeRepo(this.root, null);
    this.index = await findDoc<IndexDocument>(this.repo, this.meta.indexDocId);
    for (const [rel, docId] of Object.entries(this.index.doc()?.files ?? {})) {
      if ((await this.safePath(rel)) === null) continue;
      this.track(rel, await findDoc<FileDoc>(this.repo, docId));
    }
    await this.reconcileDisk();
    this.knownFolders = new Set(Object.keys(this.index.doc()?.folders ?? {}));

    // Phase 2: go online. From here on, changes flow both ways live.
    this.repo.networkSubsystem.addNetworkAdapter(webSocketAdapter(this.meta.syncServer));
    const onIndex = () => void this.syncIndex();
    this.index.on('change', onIndex);
    this.cleanups.push(() => this.index.off('change', onIndex));

    const { watcher, buffers } = this.host;
    watcher.onChange((p) => void this.onDiskChange(p));
    watcher.onCreate((p) => void this.onDiskCreate(p));
    watcher.onDelete((p) => void this.onDiskDelete(p));
    buffers.onOpen((p) => this.bind(p));
    buffers.onClose((p) => this.unbind(p));
    this.cleanups.push(() => watcher.dispose(), () => buffers.dispose());
    buffers.openPaths().forEach((p) => this.bind(p));
    this.log.appendLine(`project sync running: ${this.root} (${this.handles.size} files)`);
    this.host.notifier.status(`$(sync) Quarto Hub: syncing ${path.basename(this.root)}`, 5000);
  }

  async dispose(): Promise<void> {
    this.cleanups.forEach((c) => c());
    this.cleanups.length = 0;
    for (const rel of [...this.handles.keys()]) this.untrack(rel);
    for (const abs of [...this.bound.keys()]) this.unbind(abs);
    await this.repo.flush().catch(() => {});
    await this.repo.shutdown().catch(() => {});
  }

  /** The index and every tracked file doc with their current heads (URL-encoded). */
  inventory(): { docId: string; heads: string[] }[] {
    return [this.index, ...this.handles.values()].map((h) => ({ docId: h.documentId, heads: h.heads() as string[] }));
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
    const listener = () => void this.onDocChange(rel);
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

  private async logEvent(event: 'created' | 'deleted' | 'edited-offline' | 'rejected', rel: string, source: 'hub' | 'local'): Promise<void> {
    const line = JSON.stringify({ at: new Date().toISOString(), event, path: rel, source }) + '\n';
    await fs.appendFile(path.join(this.root, META_DIR, 'events.jsonl'), line).catch(() => {});
    this.log.appendLine(`${source}: ${event} ${rel}`);
  }

  private bind(abs: string): void {
    const rel = this.relOf(abs);
    if (!rel || this.bound.has(abs)) return;
    const handle = this.handles.get(rel);
    if (!handle || !isTextDocument(handle.doc())) return;
    const binding = this.host.buffers.bind(abs, handle as DocHandle<TextDoc>, this.identity);
    if (binding) this.bound.set(abs, binding);
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
      const doc = handle.doc();
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
    for (const rel of onDisk) if (!this.handles.has(rel)) await this.createFromDisk(rel);
  }

  // --- hub -> disk -------------------------------------------------------------

  // PINNED by project-sync.test.ts, replaced by GE-9: every index entry whose
  // doc id differs is untracked, retracked, and written to disk with no look
  // at what the disk holds.
  private async syncIndex(): Promise<void> {
    const files = this.index.doc()?.files ?? {};
    for (const rel of [...this.handles.keys()]) {
      if (!(rel in files)) {
        this.untrack(rel);
        const abs = await this.safePath(rel);
        if (abs !== null) await fs.rm(abs, { force: true });
        await this.logEvent('deleted', rel, 'hub');
      }
    }
    for (const [rel, docId] of Object.entries(files)) {
      if ((await this.safePath(rel)) === null) continue;
      const existing = this.handles.get(rel);
      if (existing && existing.documentId === docId) continue;
      if (existing) this.untrack(rel); // path now points at a different document
      const handle = await findDoc<FileDoc>(this.repo, docId);
      this.track(rel, handle);
      const doc = handle.doc();
      if (doc) await writeFileDoc(this.root, rel, doc);
      await this.logEvent('created', rel, 'hub');
    }
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

  // PINNED by project-sync.test.ts, gated by GE-10: a doc change rewrites a
  // file that is missing on disk.
  private async onDocChange(rel: string): Promise<void> {
    if (this.isBound(rel)) return; // the binding + autosave own this file
    const doc = this.handles.get(rel)?.doc();
    if (!doc) return;
    if (isTextDocument(doc)) {
      const current = await fs.readFile(this.absOf(rel), 'utf8').catch(() => null);
      if (current === doc.text) return;
    }
    await writeFileDoc(this.root, rel, doc);
  }

  // --- disk -> hub ------------------------------------------------------------

  private async onDiskChange(abs: string): Promise<void> {
    const rel = this.relOf(abs);
    if (!rel || this.isBound(rel)) return;
    const handle = this.handles.get(rel);
    const doc = handle?.doc();
    if (!handle || !isTextDocument(doc)) return;
    const diskText = await fs.readFile(abs, 'utf8').catch(() => null);
    if (diskText === null || diskText === doc.text) return; // our own write echoing back
    (handle as DocHandle<TextDoc>).change((d) => updateText(d, ['text'], diskText));
  }

  private async onDiskCreate(abs: string): Promise<void> {
    const rel = this.relOf(abs);
    if (!rel || this.handles.has(rel)) return;
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
    if (!rel) return;
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
