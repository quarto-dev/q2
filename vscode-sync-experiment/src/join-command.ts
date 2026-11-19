/**
 * "Quarto Hub: Join session" and "Leave session" (GE-11, G22): the VS Code layer over `core/join.ts`. It owns the input
 * box, the QuickPick, the confirms, and the notifications; the decisions live in the core.
 */
import * as os from 'node:os';
import * as vscode from 'vscode';
import { checkAttach } from './core/clone/attach';
import { attachFolder, readShare } from './core/clone/attach-flow';
import { rebindFolder } from './core/clone/rebind';
import { createProjectFolder } from './core/create-folder';
import {
  bindFolder,
  classifyJoinInput,
  fetchHealth,
  healthUrlFor,
  inspectIndex,
  isLoopbackUrl,
  offeredModes,
  projectRootMatches,
  type JoinInput,
  type Offer,
} from './core/join';
import { startJoinProxy, type JoinProxy } from './core/join-proxy';
import type { Logger } from './core/host';
import { readProjectMeta } from './core/meta';
import { ensureWasm } from './core/wasm';

/** What the command needs from the extension's project registry. */
export interface JoinHost {
  log: Logger & vscode.OutputChannel;
  startProject(root: string): Promise<void>;
  stopProject(root: string): Promise<void>;
  /** Roots with a running sync. */
  runningRoots(): string[];
  /** Test seam: the server a share link without `server=` falls back to. */
  defaultServer(): string;
}

/** Proxies this window spawned, by the folder bound through them. */
const proxies = new Map<string, JoinProxy>();

export async function stopAllProxies(): Promise<void> {
  await Promise.all([...proxies.values()].map((p) => p.stop()));
  proxies.clear();
}

function fail(host: JoinHost, prefix: string, err: unknown): void {
  const msg = err instanceof Error ? err.message : String(err);
  host.log.appendLine(`${prefix}: ${msg}`);
  void vscode.window.showErrorMessage(`Quarto Hub: ${msg}`);
}

async function askDocId(): Promise<string | undefined> {
  const input = await vscode.window.showInputBox({
    prompt: 'This server did not report a project. Automerge document id of the project index',
    placeHolder: 'automerge:3RFyJzsLsZ7MsbWCQz2kuoeRRaTX',
    ignoreFocusOut: true,
  });
  return input?.trim().replace(/^.*automerge:/, '') || undefined;
}

export async function joinCommand(host: JoinHost): Promise<void> {
  const raw = await vscode.window.showInputBox({
    prompt: 'A share ticket (q2preview…), a hub URL, or a share link',
    placeHolder: 'q2preview… or http://127.0.0.1:3000 or https://quarto-hub.com/#/share/…',
    ignoreFocusOut: true,
  });
  if (raw === undefined || !raw.trim()) return;
  const input = classifyJoinInput(raw);
  if (input.kind === 'invalid') {
    void vscode.window.showErrorMessage(`Quarto Hub: ${input.message}`);
    return;
  }

  let proxy: JoinProxy | undefined;
  let bound = false;
  try {
    let server: string;
    if (input.kind === 'ticket') {
      const q2Path = vscode.workspace.getConfiguration('quartoHub').get<string>('q2Path', 'q2') || 'q2';
      proxy = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: 'Starting the join proxy…' }, () =>
        startJoinProxy({ q2Path, ticket: input.ticket, log: host.log }),
      );
      server = proxy.wsUrl;
    } else if (input.kind === 'share') {
      server = input.server ?? host.defaultServer();
    } else {
      server = input.url;
    }

    const health = await fetchHealth(healthUrlFor(server));
    const knownId = input.kind === 'share' ? input.indexDocId : health?.indexDocId;
    const indexDocId = knownId ?? (await askDocId());
    if (!indexDocId) return;

    await ensureWasm();
    const inspection = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: 'Reading the project…' }, () =>
      inspectIndex(server, indexDocId),
    );

    const folder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    let rootMatches = false;
    let passes = false;
    let isBound = false;
    if (folder) {
      rootMatches = await projectRootMatches(folder, health?.projectRoot);
      isBound = await readProjectMeta(folder).then((m) => m !== null, () => true);
      if (inspection.gitEnabled && !isBound) {
        const share = await readShare(server, indexDocId);
        passes = (await checkAttach({ dir: folder, share })).ok;
      }
    }
    const offers = offeredModes({
      loopback: input.kind !== 'ticket' && isLoopbackUrl(server),
      hasFolder: folder !== undefined,
      rootMatches,
      gitEnabled: inspection.gitEnabled,
      passes,
      bound: isBound,
    });
    const picked = await vscode.window.showQuickPick(
      offers.map((o) => ({ label: o.label, description: o === offers[0] ? 'default' : undefined, detail: o.detail, offer: o })),
      { title: 'How should this folder join the session?', ignoreFocusOut: true },
    );
    if (!picked) return;
    bound = await bind(host, picked.offer, { input, server, indexDocId, folder, proxy });
  } catch (err) {
    fail(host, 'join failed', err);
  } finally {
    // The proxy outlives the command only when a folder was bound through it.
    if (proxy && !bound) await proxy.stop();
  }
}

interface BindContext {
  input: JoinInput;
  server: string;
  indexDocId: string;
  folder: string | undefined;
  proxy: JoinProxy | undefined;
}

/** Carry out the picked offer; true when a folder was bound (so the proxy must stay). */
async function bind(host: JoinHost, offer: Offer, c: BindContext): Promise<boolean> {
  const keep = (root: string) => c.proxy && proxies.set(root, c.proxy);
  if (offer.mode === 'mirror') {
    const parentDir = c.folder ?? os.homedir();
    await ensureWasm();
    const root = await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `Creating folder from project ${c.indexDocId}…` },
      () => createProjectFolder(c.indexDocId, parentDir, host.log, { syncServer: c.server }),
    );
    keep(root);
    // With a window already open, add the mirror beside it: the extension host (and the proxy) survive. Opening it
    // instead reloads the window, which ends the proxy.
    const count = vscode.workspace.workspaceFolders?.length ?? 0;
    if (count > 0 && vscode.workspace.updateWorkspaceFolders(count, 0, { uri: vscode.Uri.file(root) })) return true;
    await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(root), { forceNewWindow: false });
    return true;
  }
  const folder = c.folder;
  if (!folder) return false;
  if (offer.mode === 'rebind') {
    const result = await rebindFolder({
      root: folder,
      indexDocId: c.indexDocId,
      serverUrl: c.server,
      confirm: async () =>
        (await vscode.window.showWarningMessage("Rebinding deletes this folder's local sync history. Your files are not touched.", { modal: true }, 'Rebind')) === 'Rebind',
      shutdown: () => host.stopProject(folder),
    });
    if (!result.ok) {
      if (result.reason !== 'declined') void vscode.window.showErrorMessage(`Quarto Hub: ${result.message}`);
      return false;
    }
    keep(folder);
    await host.startProject(folder);
    return true;
  }
  if (offer.mode === 'clone') {
    const result = await attachFolder({ dir: folder, serverUrl: c.server, indexDocId: c.indexDocId });
    if (!result.ok) {
      void vscode.window.showErrorMessage(`Quarto Hub: ${result.message}`);
      return false;
    }
    keep(result.projectRoot);
    await host.startProject(result.projectRoot);
    return true;
  }
  // editor-only
  if (offer.confirm) {
    const yes = await vscode.window.showWarningMessage('This looks like the hub\'s own project folder. Use it as editor only?', { modal: true }, 'Use as editor only');
    if (yes !== 'Use as editor only') return false;
  }
  await bindFolder({ root: folder, indexDocId: c.indexDocId, syncServer: c.server, mode: 'editor-only' });
  keep(folder);
  await host.startProject(folder);
  return true;
}

/** Stop the sync of every folder joined through a proxy (or, with none, the running folder) and the proxies. The folders stay bound. */
export async function leaveCommand(host: JoinHost): Promise<void> {
  const roots = proxies.size > 0 ? [...proxies.keys()] : host.runningRoots();
  if (roots.length === 0) {
    void vscode.window.showInformationMessage('Quarto Hub: no session to leave.');
    return;
  }
  for (const root of roots) {
    await host.stopProject(root);
    await proxies.get(root)?.stop();
    proxies.delete(root);
  }
  void vscode.window.showInformationMessage('Quarto Hub: left the session. The folder stays bound.');
}
