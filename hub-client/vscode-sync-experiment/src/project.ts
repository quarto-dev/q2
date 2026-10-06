/**
 * Project mode: a real folder on disk mirrored from a Quarto Hub project.
 *
 * Ownership rule: VS Code owns files that are open in an editor (they get a
 * `Binding`, and VS Code's autosave writes them to disk); this module owns
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
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { createHash } from 'node:crypto';
import * as vscode from 'vscode';
import { Repo, updateText, type DocHandle } from '@automerge/automerge-repo/slim';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import { NodeFSStorageAdapter } from '@automerge/automerge-repo-storage-nodefs';
import {
  isBinaryDocument,
  isBinaryExtension,
  isTextDocument,
  inferMimeType,
  type BinaryDocumentContent,
  type IndexDocument,
} from '@quarto/quarto-automerge-schema';
import { Binding, bindings, type TextDoc } from './binding';
import type { Identity } from './presence';

export const META_DIR = '.quarto-hub';
const META_FILE = 'project.json';
const SYNC_SERVER = 'wss://sync.automerge.org';
/** Never synced in either direction (device-local state, render output, tooling). */
const IGNORED_TOP_LEVEL = new Set([META_DIR, '.vscode', '.git', '.quarto', '_site', '_book', '_freeze', 'node_modules', '.DS_Store']);

interface ProjectMeta {
  indexDocId: string;
  syncServer: string;
  createdAt: string;
}

type FileDoc = TextDoc | BinaryDocumentContent;

export async function readProjectMeta(root: string): Promise<ProjectMeta | null> {
  try {
    return JSON.parse(await fs.readFile(path.join(root, META_DIR, META_FILE), 'utf8')) as ProjectMeta;
  } catch {
    return null;
  }
}

function makeRepo(root: string, online: boolean): Repo {
  return new Repo({
    storage: new NodeFSStorageAdapter(path.join(root, META_DIR, 'automerge')),
    network: online ? [new BrowserWebSocketClientAdapter(SYNC_SERVER)] : [],
  });
}

function isIgnored(rel: string): boolean {
  const top = rel.split('/')[0] ?? '';
  return rel === '' || IGNORED_TOP_LEVEL.has(top) || top.startsWith(META_DIR) || rel.split('/').some((seg) => seg === '.DS_Store');
}

async function findDoc<T>(repo: Repo, docId: string): Promise<DocHandle<T>> {
  const handle = await repo.find<T>(`automerge:${docId}` as never, { signal: AbortSignal.timeout(30_000) });
  await handle.whenReady();
  return handle;
}

async function writeFileDoc(root: string, rel: string, doc: FileDoc): Promise<void> {
  const abs = path.join(root, rel);
  await fs.mkdir(path.dirname(abs), { recursive: true });
  if (isTextDocument(doc)) await fs.writeFile(abs, doc.text, 'utf8');
  else if (isBinaryDocument(doc)) await fs.writeFile(abs, doc.content);
}

// ---------------------------------------------------------------------------
// Create a new folder from a project
// ---------------------------------------------------------------------------

/**
 * Folder name for a project. The hub's display name lives in each user's
 * project-set document, not in the index, so the best available source is
 * the `title:` in `_quarto.yml`; otherwise fall back to the id.
 */
async function projectFolderName(repo: Repo, files: Record<string, string>, indexDocId: string): Promise<string> {
  let title: string | undefined;
  if (files['_quarto.yml']) {
    const yml = (await findDoc<FileDoc>(repo, files['_quarto.yml'])).doc();
    if (isTextDocument(yml)) title = /^\s*title:\s*["']?(.+?)["']?\s*$/m.exec(yml.text)?.[1];
  }
  // Same sanitisation as hub-client's export (quarto-sync-client/project-folder-name.ts).
  const cleaned = (title ?? '')
    .replace(/[\u0000-\u0020<>:"/\\|?*]/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-+/, '')
    .replace(/[-. ]+$/, '');
  return cleaned || `quarto-hub-${indexDocId.slice(0, 8)}`;
}

/** Create `<parentDir>/<project name>` from the project and return its path. */
export async function createProjectFolder(indexDocId: string, parentDir: string, log: vscode.OutputChannel): Promise<string> {
  // The storage dir is only known once we have a name; sync to a temp store
  // first, then move it into place.
  const tmpRoot = await fs.mkdtemp(path.join(parentDir, '.quarto-hub-tmp-'));
  await fs.mkdir(path.join(tmpRoot, META_DIR, 'automerge'), { recursive: true });
  const repo = makeRepo(tmpRoot, true);
  let root = tmpRoot;
  try {
    const index = await findDoc<IndexDocument>(repo, indexDocId);
    const files = index.doc()?.files ?? {};
    const base = await projectFolderName(repo, files, indexDocId);
    root = path.join(parentDir, base);
    for (let n = 2; await fs.stat(root).then(() => true, () => false); n++) root = path.join(parentDir, `${base}-${n}`);
    const meta: ProjectMeta = { indexDocId, syncServer: SYNC_SERVER, createdAt: new Date().toISOString() };
    await fs.writeFile(path.join(tmpRoot, META_DIR, META_FILE), JSON.stringify(meta, null, 2) + '\n');
    for (const [rel, docId] of Object.entries(files)) {
      const handle = await findDoc<FileDoc>(repo, docId);
      const doc = handle.doc();
      if (doc) await writeFileDoc(tmpRoot, rel, doc);
      log.appendLine(`wrote ${rel}`);
    }
    for (const folder of Object.keys(index.doc()?.folders ?? {})) {
      await fs.mkdir(path.join(tmpRoot, folder), { recursive: true });
    }
    // Open files are written by VS Code's autosave (see ownership rule above);
    // a formatter would broadcast its reflow to every collaborator.
    await fs.mkdir(path.join(tmpRoot, '.vscode'), { recursive: true });
    await fs.writeFile(
      path.join(tmpRoot, '.vscode', 'settings.json'),
      JSON.stringify({ 'files.autoSave': 'afterDelay', 'files.autoSaveDelay': 300, 'editor.formatOnSave': false }, null, 2) + '\n',
    );
    await repo.flush();
  } finally {
    await repo.shutdown().catch(() => {});
  }
  await fs.rename(tmpRoot, root);
  return root;
}

// ---------------------------------------------------------------------------
// Live sync of an existing project folder
// ---------------------------------------------------------------------------

export class ProjectSync {
  private repo!: Repo;
  private index!: DocHandle<IndexDocument>;
  private readonly handles = new Map<string, DocHandle<FileDoc>>(); // rel path -> handle
  private readonly fileListeners = new Map<string, () => void>();
  /** Explicit (empty) folders last seen in the index, for hub-side removals. */
  private knownFolders = new Set<string>();
  private readonly disposables: vscode.Disposable[] = [];

  constructor(
    readonly root: string,
    private readonly meta: ProjectMeta,
    private readonly identity: Identity,
    private readonly log: vscode.OutputChannel,
  ) {}

  async start(): Promise<void> {
    // Phase 1, offline: load the last-synced state from local storage and
    // fold in whatever changed on disk since. Doing this before going online
    // means offline edits become changes rooted at the state the disk
    // actually reflected, which is what lets Automerge merge them properly.
    this.repo = makeRepo(this.root, false);
    this.index = await findDoc<IndexDocument>(this.repo, this.meta.indexDocId);
    for (const [rel, docId] of Object.entries(this.index.doc()?.files ?? {})) {
      this.track(rel, await findDoc<FileDoc>(this.repo, docId));
    }
    await this.reconcileDisk();
    this.knownFolders = new Set(Object.keys(this.index.doc()?.folders ?? {}));

    // Phase 2: go online. From here on, changes flow both ways live.
    this.repo.networkSubsystem.addNetworkAdapter(new BrowserWebSocketClientAdapter(this.meta.syncServer));
    const onIndex = () => void this.syncIndex();
    this.index.on('change', onIndex);
    this.disposables.push({ dispose: () => this.index.off('change', onIndex) });

    const watcher = vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(this.root, '**/*'));
    this.disposables.push(
      watcher,
      watcher.onDidChange((uri) => void this.onDiskChange(uri)),
      watcher.onDidCreate((uri) => void this.onDiskCreate(uri)),
      watcher.onDidDelete((uri) => void this.onDiskDelete(uri)),
      vscode.workspace.onDidOpenTextDocument((doc) => this.bind(doc)),
      vscode.workspace.onDidCloseTextDocument((doc) => bindings.get(doc.uri.toString())?.dispose()),
    );
    vscode.workspace.textDocuments.forEach((doc) => this.bind(doc));
    this.log.appendLine(`project sync running: ${this.root} (${this.handles.size} files)`);
  }

  async dispose(): Promise<void> {
    this.disposables.forEach((d) => d.dispose());
    for (const rel of [...this.handles.keys()]) this.untrack(rel);
    for (const b of [...bindings.values()]) if (this.relOf(b.document.uri)) b.dispose();
    await this.repo.flush().catch(() => {});
    await this.repo.shutdown().catch(() => {});
  }

  // --- helpers -----------------------------------------------------------

  /** Relative slash path if `uri` is a non-ignored file inside this project, else null. */
  private relOf(uri: vscode.Uri): string | null {
    if (uri.scheme !== 'file') return null;
    const rel = path.relative(this.root, uri.fsPath).split(path.sep).join('/');
    if (rel.startsWith('..') || path.isAbsolute(rel) || isIgnored(rel)) return null;
    return rel;
  }

  private track(rel: string, handle: DocHandle<FileDoc>): void {
    this.handles.set(rel, handle);
    const listener = () => void this.onDocChange(rel);
    handle.on('change', listener);
    this.fileListeners.set(rel, listener);
    // The file may already be open in an editor. For a file created in VS
    // Code, onDidOpenTextDocument fires before the file watcher reports the
    // new file, so bind() ran when no document existed for this path yet and
    // did nothing; without a Binding there is no presence, so cursors never
    // showed up until the next activation. Bind now that the handle exists.
    const open = vscode.workspace.textDocuments.find((d) => d.uri.toString() === this.uriOf(rel));
    if (open) this.bind(open);
  }

  private untrack(rel: string): void {
    const handle = this.handles.get(rel);
    const listener = this.fileListeners.get(rel);
    if (handle && listener) handle.off('change', listener);
    this.handles.delete(rel);
    this.fileListeners.delete(rel);
    // A binding to this handle is stale once the path no longer maps to it.
    bindings.get(this.uriOf(rel))?.dispose();
  }

  private uriOf(rel: string): string {
    return vscode.Uri.file(path.join(this.root, rel)).toString();
  }

  private async logEvent(event: 'created' | 'deleted' | 'edited-offline', rel: string, source: 'hub' | 'local'): Promise<void> {
    const line = JSON.stringify({ at: new Date().toISOString(), event, path: rel, source }) + '\n';
    await fs.appendFile(path.join(this.root, META_DIR, 'events.jsonl'), line).catch(() => {});
    this.log.appendLine(`${source}: ${event} ${rel}`);
  }

  private bind(document: vscode.TextDocument): void {
    const rel = this.relOf(document.uri);
    if (!rel || bindings.has(document.uri.toString())) return;
    const handle = this.handles.get(rel);
    if (handle && isTextDocument(handle.doc())) new Binding(document, handle as DocHandle<TextDoc>, this.identity);
  }

  private isBound(rel: string): boolean {
    return bindings.has(this.uriOf(rel));
  }

  private async listDiskFiles(dir = this.root, out: string[] = []): Promise<string[]> {
    for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
      const abs = path.join(dir, entry.name);
      const rel = path.relative(this.root, abs).split(path.sep).join('/');
      if (isIgnored(rel)) continue;
      if (entry.isDirectory()) await this.listDiskFiles(abs, out);
      else if (entry.isFile()) out.push(rel);
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
        const diskText = await fs.readFile(path.join(this.root, rel), 'utf8');
        if (diskText !== doc.text) {
          (handle as DocHandle<TextDoc>).change((d) => updateText(d, ['text'], diskText));
          await this.logEvent('edited-offline', rel, 'local');
        }
      } else if (isBinaryDocument(doc)) {
        // Local wins for binaries too: a differing file on disk replaces the
        // hub copy wholesale (no character merge is meaningful for bytes).
        const content = new Uint8Array(await fs.readFile(path.join(this.root, rel)));
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

  private async syncIndex(): Promise<void> {
    const files = this.index.doc()?.files ?? {};
    for (const rel of [...this.handles.keys()]) {
      if (!(rel in files)) {
        this.untrack(rel);
        await fs.rm(path.join(this.root, rel), { force: true });
        await this.logEvent('deleted', rel, 'hub');
      }
    }
    for (const [rel, docId] of Object.entries(files)) {
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
      if (!this.knownFolders.has(folder)) await fs.mkdir(path.join(this.root, folder), { recursive: true }).catch(() => {});
    }
    for (const folder of this.knownFolders) {
      // Only remove if still empty: a folder with files is implied by them.
      if (!folders.has(folder)) await fs.rmdir(path.join(this.root, folder)).catch(() => {});
    }
    this.knownFolders = folders;
  }

  private async onDocChange(rel: string): Promise<void> {
    if (this.isBound(rel)) return; // the Binding + autosave own this file
    const doc = this.handles.get(rel)?.doc();
    if (!doc) return;
    if (isTextDocument(doc)) {
      const current = await fs.readFile(path.join(this.root, rel), 'utf8').catch(() => null);
      if (current === doc.text) return;
    }
    await writeFileDoc(this.root, rel, doc);
  }

  // --- disk -> hub ------------------------------------------------------------

  private async onDiskChange(uri: vscode.Uri): Promise<void> {
    const rel = this.relOf(uri);
    if (!rel || this.isBound(rel)) return;
    const handle = this.handles.get(rel);
    const doc = handle?.doc();
    if (!handle || !isTextDocument(doc)) return;
    const diskText = await fs.readFile(uri.fsPath, 'utf8').catch(() => null);
    if (diskText === null || diskText === doc.text) return; // our own write echoing back
    (handle as DocHandle<TextDoc>).change((d) => updateText(d, ['text'], diskText));
  }

  private async onDiskCreate(uri: vscode.Uri): Promise<void> {
    const rel = this.relOf(uri);
    if (!rel || this.handles.has(rel)) return;
    const stat = await fs.stat(uri.fsPath).catch(() => null);
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
    const abs = path.join(this.root, rel);
    let handle: DocHandle<FileDoc>;
    if (isBinaryExtension(rel)) {
      const content = new Uint8Array(await fs.readFile(abs));
      const hash = createHash('sha256').update(content).digest('hex');
      handle = this.repo.create<FileDoc>({ content, mimeType: inferMimeType(rel), hash } as FileDoc);
    } else {
      // If the file is already open, the buffer is the freshest copy: with
      // autosave there can be keystrokes not yet on disk, and the Binding
      // that track() attaches would otherwise roll them back to the doc.
      const open = vscode.workspace.textDocuments.find((d) => d.uri.toString() === this.uriOf(rel));
      handle = this.repo.create<FileDoc>({ text: open ? open.getText() : await fs.readFile(abs, 'utf8') } as FileDoc);
    }
    this.track(rel, handle);
    this.index.change((d) => void (d.files[rel] = handle.documentId));
    await this.logEvent('created', rel, 'local');
  }

  private async onDiskDelete(uri: vscode.Uri): Promise<void> {
    const rel = this.relOf(uri);
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
