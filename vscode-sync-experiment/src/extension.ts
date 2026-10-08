/**
 * Quarto Hub for VS Code — prototype.
 *
 * Two modes, both against wss://sync.automerge.org with no auth:
 *  - "Connect to Document": one file document opened as a `quartohub:/` buffer.
 *  - "Create Folder from Project": a real folder mirrored from a project index
 *    document and kept in sync while it is open as a workspace folder.
 * Both share the buffer binding and remote cursors in binding.ts.
 */
import * as os from 'node:os';
import * as path from 'node:path';
import * as vscode from 'vscode';
// Slim entrypoints: no wasm is compiled at module load. The wasm is
// initialised asynchronously on first use (see ensureWasm), which keeps
// activation instant and avoids synchronous WebAssembly compilation, which
// Electron forbids in some process types.
import { Repo, type AnyDocumentId, type DocHandle } from '@automerge/automerge-repo/slim';
import { initializeBase64Wasm, isWasmInitialized } from '@automerge/automerge/slim';
import { automergeWasmBase64 } from '@automerge/automerge/automerge.wasm.base64';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import { Binding, bindings, registerBindingEvents, type TextDoc } from './binding';
import type { Identity } from './presence';
import { ProjectSync, createProjectFolder, readProjectMeta } from './project';

const SCHEME = 'quartohub';
const SYNC_SERVER = 'wss://sync.automerge.org';

/** Single-file mode: docId -> repo + handle. */
const singleFiles = new Map<string, { repo: Repo; handle: DocHandle<TextDoc> }>();
/** Project mode: workspace folder path -> sync. */
const projects = new Map<string, ProjectSync>();
let log: vscode.OutputChannel;

async function ensureWasm(): Promise<void> {
  if (isWasmInitialized()) return;
  await initializeBase64Wasm(automergeWasmBase64);
}

// ---------------------------------------------------------------------------
// Activation
// ---------------------------------------------------------------------------

export function activate(context: vscode.ExtensionContext): void {
  log = vscode.window.createOutputChannel('Quarto Hub');
  context.subscriptions.push(
    log,
    ...registerBindingEvents(),
    vscode.workspace.registerFileSystemProvider(SCHEME, new HubFs(), { isCaseSensitive: true }),
    vscode.commands.registerCommand('quartoHub.connect', () => connect(context)),
    vscode.commands.registerCommand('quartoHub.disconnect', disconnect),
    vscode.commands.registerCommand('quartoHub.createFolder', createFolder),
    vscode.workspace.onDidCloseTextDocument((doc) => {
      if (doc.uri.scheme === SCHEME) void closeSingleFile(docIdOf(doc.uri));
    }),
    vscode.workspace.onDidChangeWorkspaceFolders((e) => {
      e.removed.forEach((f) => void stopProject(f.uri.fsPath));
      e.added.forEach((f) => void startProject(f.uri.fsPath, context));
    }),
  );
  for (const f of vscode.workspace.workspaceFolders ?? []) void startProject(f.uri.fsPath, context);
}

export async function deactivate(): Promise<void> {
  await Promise.all([...[...singleFiles.keys()].map(closeSingleFile), ...[...projects.keys()].map(stopProject)]);
}

function showError(prefix: string, err: unknown): void {
  const msg = err instanceof Error ? err.message : String(err);
  log.appendLine(`${prefix}: ${err instanceof Error ? (err.stack ?? msg) : msg}`);
  void vscode.window.showErrorMessage(`Quarto Hub: ${msg}`);
}

function getIdentity(context: vscode.ExtensionContext): Identity {
  let userId = context.globalState.get<string>('quartoHub.userId');
  if (!userId) {
    userId = crypto.randomUUID();
    void context.globalState.update('quartoHub.userId', userId);
  }
  const configured = vscode.workspace.getConfiguration('quartoHub').get<string>('userName', '').trim();
  const palette = ['#CC6677', '#332288', '#DDCC77', '#117733', '#88CCEE', '#882255', '#44AA99', '#999933', '#AA4499'];
  const userColor = palette[parseInt(userId.slice(0, 6), 16) % palette.length]!;
  return { userId, userName: configured || `${os.userInfo().username} (VS Code)`, userColor };
}

async function askDocId(prompt: string): Promise<string | undefined> {
  const input = await vscode.window.showInputBox({ prompt, placeHolder: 'automerge:3RFyJzsLsZ7MsbWCQz2kuoeRRaTX', ignoreFocusOut: true });
  const id = input?.trim().replace(/^.*automerge:/, '');
  return id || undefined;
}

// ---------------------------------------------------------------------------
// Project mode
// ---------------------------------------------------------------------------

async function createFolder(): Promise<void> {
  const indexDocId = await askDocId('Automerge document id of the Quarto Hub project');
  if (!indexDocId) return;
  // Place the folder inside the current workspace folder (home dir if none).
  const parentDir = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? os.homedir();
  let root: string;
  try {
    await ensureWasm();
    root = await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `Creating folder from project ${indexDocId}…` },
      () => createProjectFolder(indexDocId, parentDir, log),
    );
  } catch (err) {
    showError('create folder failed', err);
    return;
  }
  // Opening the folder reloads the window; startProject picks it up on activation.
  await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(root), { forceNewWindow: false });
}

async function startProject(root: string, context: vscode.ExtensionContext): Promise<void> {
  if (projects.has(root)) return;
  const meta = await readProjectMeta(root);
  if (!meta) return;
  try {
    await ensureWasm();
    const sync = new ProjectSync(root, meta, getIdentity(context), log);
    projects.set(root, sync);
    await sync.start();
    vscode.window.setStatusBarMessage(`$(sync) Quarto Hub: syncing ${path.basename(root)}`, 5000);
  } catch (err) {
    projects.delete(root);
    showError(`project sync failed for ${root}`, err);
  }
}

async function stopProject(root: string): Promise<void> {
  const sync = projects.get(root);
  projects.delete(root);
  await sync?.dispose();
}

// ---------------------------------------------------------------------------
// Single-file mode
// ---------------------------------------------------------------------------

async function connect(context: vscode.ExtensionContext): Promise<void> {
  const docId = await askDocId('Automerge document id of the Quarto Hub file');
  if (!docId) return;
  if (!singleFiles.has(docId)) {
    try {
      await ensureWasm();
      await vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: `Connecting to ${docId}…` },
        () => openSingleFile(docId),
      );
    } catch (err) {
      showError('connect failed', err);
      return;
    }
  }
  // The id lives in the path, not the authority: VS Code lowercases
  // authorities and Automerge ids are case-sensitive.
  const uri = vscode.Uri.from({ scheme: SCHEME, path: `/${docId}.qmd` });
  try {
    const document = await vscode.workspace.openTextDocument(uri);
    const entry = singleFiles.get(docId)!;
    if (!bindings.has(uri.toString())) new Binding(document, entry.handle, getIdentity(context));
    await vscode.window.showTextDocument(document, { preview: false });
  } catch (err) {
    showError('open failed', err);
  }
}

async function disconnect(): Promise<void> {
  const ids = [...singleFiles.keys()];
  if (ids.length === 0) return;
  const active = vscode.window.activeTextEditor?.document.uri;
  const id = active?.scheme === SCHEME ? docIdOf(active) : await vscode.window.showQuickPick(ids, { title: 'Disconnect which document?' });
  if (id) await closeSingleFile(id);
}

async function openSingleFile(docId: string): Promise<void> {
  // In Node the browser adapter's isomorphic-ws import resolves to `ws`.
  const repo = new Repo({ network: [new BrowserWebSocketClientAdapter(SYNC_SERVER)] });
  try {
    const handle = await repo.find<TextDoc>(`automerge:${docId}` as AnyDocumentId, { signal: AbortSignal.timeout(30_000) });
    await handle.whenReady();
    const doc = handle.doc();
    if (!doc || typeof doc.text !== 'string') {
      throw new Error('That document has no `text` field. For a project index, use "Create Folder from Project".');
    }
    singleFiles.set(docId, { repo, handle });
    log.appendLine(`connected: ${docId} (${doc.text.length} chars)`);
  } catch (err) {
    await repo.shutdown().catch(() => {});
    throw err;
  }
}

async function closeSingleFile(docId: string): Promise<void> {
  const entry = singleFiles.get(docId);
  if (!entry) return;
  singleFiles.delete(docId);
  for (const b of [...bindings.values()]) if (b.handle === entry.handle) b.dispose();
  await entry.repo.shutdown().catch(() => {});
}

function docIdOf(uri: vscode.Uri): string {
  return uri.path.replace(/^\//, '').replace(/\.qmd$/, '');
}

/**
 * FileSystemProvider for single-file mode: makes quartohub:/ buffers writable.
 * Content comes from the live Automerge doc, so "save" has nothing to persist.
 */
class HubFs implements vscode.FileSystemProvider {
  private readonly emitter = new vscode.EventEmitter<vscode.FileChangeEvent[]>();
  readonly onDidChangeFile = this.emitter.event;

  watch(): vscode.Disposable {
    return new vscode.Disposable(() => {});
  }
  stat(uri: vscode.Uri): vscode.FileStat {
    const text = singleFiles.get(docIdOf(uri))?.handle.doc()?.text;
    if (text === undefined) throw vscode.FileSystemError.FileNotFound(uri);
    return { type: vscode.FileType.File, ctime: 0, mtime: Date.now(), size: Buffer.byteLength(text) };
  }
  readFile(uri: vscode.Uri): Uint8Array {
    const text = singleFiles.get(docIdOf(uri))?.handle.doc()?.text;
    if (text === undefined) throw vscode.FileSystemError.FileNotFound(uri);
    return Buffer.from(text, 'utf8');
  }
  writeFile(): void {
    // Edits were already streamed to Automerge keystroke by keystroke.
  }
  readDirectory(): [string, vscode.FileType][] {
    return [];
  }
  createDirectory(): void {
    throw vscode.FileSystemError.NoPermissions();
  }
  delete(): void {
    throw vscode.FileSystemError.NoPermissions();
  }
  rename(): void {
    throw vscode.FileSystemError.NoPermissions();
  }
}

