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
import * as vscode from 'vscode';
// Slim entrypoints: no wasm is compiled at module load; see core/wasm.ts.
import { Repo, type AnyDocumentId, type DocHandle } from '@automerge/automerge-repo/slim';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import { Binding, bindings, registerBindingEvents, type TextDoc } from './binding';
import { attachFolder } from './core/clone/attach-flow';
import { rebindFolder } from './core/clone/rebind';
import { createProjectFolder } from './core/create-folder';
import { DEFAULT_SYNC_SERVER } from './core/meta';
import { parseProjectRef } from '@quarto/share-url';
import { readProjectMeta } from './core/meta';
import type { Identity } from './core/presence';
import { ProjectSync } from './core/project-sync';
import { ensureWasm } from './core/wasm';
import { VscodeBufferHost, VscodeFileWatcher, VscodeNotifier } from './vscode-host';

const SCHEME = 'quartohub';
const SYNC_SERVER = 'wss://sync.automerge.org';

/** Single-file mode: docId -> repo + handle. */
const singleFiles = new Map<string, { repo: Repo; handle: DocHandle<TextDoc> }>();
/** Project mode: workspace folder path -> sync. */
const projects = new Map<string, ProjectSync>();
let log: vscode.OutputChannel;

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
    vscode.commands.registerCommand('quartoHub.attachFolder', () => attachCommand(context)),
    vscode.commands.registerCommand('quartoHub.rebindFolder', () => rebindCommand(context)),
    vscode.commands.registerCommand('quartoHub.detachedFiles', detachedFilesCommand),
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
  await createFolderFor(indexDocId, process.env.QUARTO_HUB_TEST_SYNC_SERVER || undefined);
}

async function createFolderFor(indexDocId: string, syncServer: string | undefined): Promise<void> {
  // Place the folder inside the current workspace folder (home dir if none).
  const parentDir = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? os.homedir();
  let root: string;
  try {
    await ensureWasm();
    root = await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `Creating folder from project ${indexDocId}…` },
      // QUARTO_HUB_TEST_SYNC_SERVER is a test-only seam for the Playwright
      // smoke tests (GE-8); GE-11 replaces it with the real per-folder binding.
      () => createProjectFolder(indexDocId, parentDir, log, { syncServer }),
    );
  } catch (err) {
    showError('create folder failed', err);
    return;
  }
  // Opening the folder reloads the window; startProject picks it up on activation.
  await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(root), { forceNewWindow: false });
}

/** The id and server a user names: a bare id, or a share URL (its `server=` wins; the test seam comes next). */
async function askProject(prompt: string): Promise<{ indexDocId: string; syncServer: string } | undefined> {
  const input = await vscode.window.showInputBox({ prompt, placeHolder: 'automerge:… or https://quarto-hub.com/#/share/…', ignoreFocusOut: true });
  if (!input?.trim()) return undefined;
  const ref = parseProjectRef(input.trim().replace(/^automerge:/, ''));
  const syncServer = ref.server ?? (process.env.QUARTO_HUB_TEST_SYNC_SERVER || DEFAULT_SYNC_SERVER);
  return { indexDocId: ref.project.replace(/^.*automerge:/, ''), syncServer };
}

/** "Attach Folder to Project": bind the open git clone to a project (GE-10; GE-11's Join command calls the same core). */
async function attachCommand(context: vscode.ExtensionContext): Promise<void> {
  const dir = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (!dir) {
    void vscode.window.showErrorMessage('Quarto Hub: open the git clone as a folder first.');
    return;
  }
  const project = await askProject('Project id or share link to attach this folder to');
  if (!project) return;
  try {
    await ensureWasm();
    const result = await attachFolder({ dir, serverUrl: project.syncServer, indexDocId: project.indexDocId });
    if (!result.ok) {
      log.appendLine(`attach refused (${result.reason}): ${result.message}`);
      const choice = await vscode.window.showErrorMessage(`Quarto Hub: ${result.message}`, ...(result.offerMirror ? ['Create a mirror folder instead'] : []));
      if (choice) await createFolderFor(project.indexDocId, project.syncServer);
      return;
    }
    await startProject(result.projectRoot, context);
  } catch (err) {
    showError('attach failed', err);
  }
}

/** "Rebind Folder to Project": point a bound folder at a new project session, after a confirm. */
async function rebindCommand(context: vscode.ExtensionContext): Promise<void> {
  const root = [...projects.keys()][0] ?? vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (!root) return;
  const project = await askProject('Project id or share link to rebind this folder to');
  if (!project) return;
  try {
    await ensureWasm();
    const result = await rebindFolder({
      root,
      indexDocId: project.indexDocId,
      serverUrl: project.syncServer,
      confirm: async () =>
        (await vscode.window.showWarningMessage(
          'Rebinding deletes this folder\'s local sync history. Your files are not touched.',
          { modal: true },
          'Rebind',
        )) === 'Rebind',
      shutdown: () => stopProject(root),
    });
    if (!result.ok) {
      if (result.reason !== 'declined') void vscode.window.showErrorMessage(`Quarto Hub: ${result.message}`);
      return;
    }
    await startProject(root, context);
  } catch (err) {
    showError('rebind failed', err);
  }
}

/** The status bar item's click: list the detached files and offer "Adopt" or "Stay detached" for each. */
async function detachedFilesCommand(): Promise<void> {
  const items: (vscode.QuickPickItem & { sync: ProjectSync; rel: string })[] = [];
  for (const sync of projects.values()) {
    for (const rel of sync.cloneState()?.detached ?? []) items.push({ label: rel, description: 'detached', sync, rel });
    for (const rel of sync.cloneState()?.superseded ?? []) items.push({ label: rel, description: 'your doc was superseded', sync, rel });
  }
  if (items.length === 0) {
    void vscode.window.showInformationMessage('Quarto Hub: no detached files.');
    return;
  }
  const picked = await vscode.window.showQuickPick(items, { title: 'Detached files' });
  if (!picked) return;
  const action = await vscode.window.showQuickPick(['Adopt shared version', 'Stay detached'], { title: picked.rel });
  if (action === 'Adopt shared version') await picked.sync.adopt(picked.rel);
  else if (action === 'Stay detached') await picked.sync.stayDetached(picked.rel);
}

async function startProject(root: string, context: vscode.ExtensionContext): Promise<void> {
  if (projects.has(root)) return;
  try {
    const meta = await readProjectMeta(root); // throws for a mode this version does not know
    if (!meta) return;
    await ensureWasm();
    const host = { log, watcher: new VscodeFileWatcher(root), buffers: new VscodeBufferHost(), notifier: new VscodeNotifier() };
    // QUARTO_HUB_TEST_* are test-only seams for the Playwright tests (GE-9): shorter waits.
    const flipWaitMs = Number(process.env.QUARTO_HUB_TEST_FLIP_WAIT_MS) || undefined;
    const swapTimeoutMs = Number(process.env.QUARTO_HUB_TEST_SWAP_TIMEOUT_MS) || undefined;
    const sync = new ProjectSync(root, meta, getIdentity(context), host, { flipWaitMs, swapTimeoutMs });
    projects.set(root, sync);
    await sync.start();
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

