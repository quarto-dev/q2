/**
 * The Preview button (plan D-C): `quarto/previewStart` asks `q2 lsp` for a
 * static preview of the active document; the answer's URL is shown in a
 * webview panel beside the editor (or in the browser, per
 * `q2.preview.target`). The server owns the preview: it re-renders on
 * save and the page reloads itself, so nothing here watches files.
 */
import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';

export const PREVIEW_START = 'quarto/previewStart';
export const PREVIEW_STOP = 'quarto/previewStop';
export const PREVIEW_STATUS = 'quarto/previewStatus';

export interface PreviewStarted {
  previewId: string;
  url: string;
  root: string;
}

export interface PreviewStatus {
  previewId: string;
  state: 'ready' | 'rendering' | 'rendered' | 'stopped';
  url?: string;
  ok?: boolean;
  errors?: number;
  warnings?: number;
  text?: string;
  error?: string;
}

/** Whether the server advertises the preview requests. */
export function serverSupportsPreview(client: LanguageClient): boolean {
  const experimental = client.initializeResult?.capabilities.experimental as
    | { quarto?: { preview?: number } }
    | undefined;
  return typeof experimental?.quarto?.preview === 'number';
}

/** The status bar text for a status notification. */
export function statusText(status: PreviewStatus): { text: string; tooltip: string } {
  switch (status.state) {
    case 'ready':
      return { text: '$(open-preview) Preview', tooltip: `Quarto 2 preview at ${status.url ?? ''}` };
    case 'rendering':
      return { text: '$(sync~spin) Rendering', tooltip: 'Quarto 2 is re-rendering the preview' };
    case 'rendered': {
      const errors = status.errors ?? 0;
      const warnings = status.warnings ?? 0;
      if (!status.ok || errors > 0) {
        return {
          text: `$(error) Preview: ${errors} error${errors === 1 ? '' : 's'}`,
          tooltip: 'The last render failed; see the Quarto 2 output',
        };
      }
      if (warnings > 0) {
        return {
          text: `$(warning) Preview: ${warnings} warning${warnings === 1 ? '' : 's'}`,
          tooltip: 'The last render had warnings; see the Quarto 2 output',
        };
      }
      return { text: '$(check) Preview', tooltip: 'The preview is up to date' };
    }
    case 'stopped':
      return {
        text: '$(circle-slash) Preview stopped',
        tooltip: status.error ?? 'The preview stopped',
      };
  }
}

export class PreviewManager implements vscode.Disposable {
  private client: LanguageClient | undefined;
  private readonly panels = new Map<string, vscode.WebviewPanel>();
  private readonly status: vscode.StatusBarItem;
  private clientDisposables: vscode.Disposable[] = [];

  constructor(private readonly output: vscode.OutputChannel) {
    this.status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
    this.status.command = 'q2.showOutput';
  }

  /** Use `client` (after each server start). */
  attach(client: LanguageClient): void {
    this.detach();
    this.client = client;
    void vscode.commands.executeCommand('setContext', 'q2.previewAvailable', serverSupportsPreview(client));
    this.clientDisposables.push(
      client.onNotification(PREVIEW_STATUS, (status: PreviewStatus) => this.onStatus(status)),
    );
  }

  /** The server went away: its previews did too. */
  detach(): void {
    this.clientDisposables.forEach((d) => d.dispose());
    this.clientDisposables = [];
    this.client = undefined;
    for (const panel of this.panels.values()) panel.dispose();
    this.panels.clear();
    this.status.hide();
  }

  /** Start (or reuse) the preview of `uri` (default: the active editor). */
  async preview(uri?: vscode.Uri): Promise<PreviewStarted | undefined> {
    const client = this.client;
    if (!client) {
      void vscode.window.showErrorMessage('Quarto 2: the language server is not running.');
      return undefined;
    }
    if (!serverSupportsPreview(client)) {
      void vscode.window.showErrorMessage(
        'Quarto 2: this q2 does not support previews from the editor; update q2.',
      );
      return undefined;
    }
    const document = uri
      ? await vscode.workspace.openTextDocument(uri)
      : vscode.window.activeTextEditor?.document;
    if (!document) return undefined;
    if (document.uri.scheme !== 'file') {
      void vscode.window.showErrorMessage('Quarto 2: only files on disk can be previewed.');
      return undefined;
    }
    // The static preview renders what is on disk.
    if (document.isDirty) await document.save();

    let started: PreviewStarted;
    try {
      started = await vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Window,
          title: 'Quarto 2: rendering preview',
        },
        () =>
          client.sendRequest<PreviewStarted>(PREVIEW_START, {
            textDocument: { uri: client.code2ProtocolConverter.asUri(document.uri) },
          }),
      );
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      this.output.appendLine(`Preview failed: ${message}`);
      void vscode.window.showErrorMessage(`Quarto 2: preview failed. ${firstLine(message)}`, 'Show output').then(
        (choice) => {
          if (choice) this.output.show(true);
        },
      );
      return undefined;
    }
    this.output.appendLine(`Preview ${started.previewId} of ${started.root}: ${started.url}`);

    const target = vscode.workspace.getConfiguration('q2').get<string>('preview.target') ?? 'panel';
    const external = await vscode.env.asExternalUri(vscode.Uri.parse(started.url));
    if (target === 'browser') {
      await vscode.env.openExternal(external);
      return started;
    }
    await this.showPanel(started, external);
    return started;
  }

  private async showPanel(started: PreviewStarted, url: vscode.Uri): Promise<void> {
    let panel = this.panels.get(started.previewId);
    if (!panel) {
      panel = vscode.window.createWebviewPanel(
        'q2.preview',
        'Quarto 2 Preview',
        { viewColumn: vscode.ViewColumn.Beside, preserveFocus: true },
        { enableScripts: true, retainContextWhenHidden: true, localResourceRoots: [] },
      );
      const previewId = started.previewId;
      this.panels.set(previewId, panel);
      panel.onDidDispose(() => {
        // Closing the panel ends the preview, unless the server already did.
        if (this.panels.get(previewId) === panel) {
          this.panels.delete(previewId);
          void this.client?.sendRequest(PREVIEW_STOP, { previewId }).catch(() => undefined);
        }
      });
      panel.webview.onDidReceiveMessage((message: { type: string; url?: string }) => {
        if (message.type === 'open-external' && message.url) {
          void vscode.env.openExternal(vscode.Uri.parse(message.url));
        }
      });
    } else {
      panel.reveal(vscode.ViewColumn.Beside, true);
    }
    panel.webview.html = previewHtml(url.toString(true), nonce());
  }

  private onStatus(status: PreviewStatus): void {
    const { text, tooltip } = statusText(status);
    this.status.text = text;
    this.status.tooltip = tooltip;
    this.status.show();
    if (status.state === 'rendered' && status.text && (!status.ok || (status.errors ?? 0) > 0 || (status.warnings ?? 0) > 0)) {
      this.output.appendLine(status.text.trimEnd());
    }
    if (status.state === 'stopped') {
      if (status.error) this.output.appendLine(`Preview stopped: ${status.error}`);
      const panel = this.panels.get(status.previewId);
      if (panel) {
        this.panels.delete(status.previewId);
        panel.dispose();
      }
    }
  }

  dispose(): void {
    this.detach();
    this.status.dispose();
  }
}

function firstLine(s: string): string {
  return s.split('\n', 1)[0] ?? s;
}

function nonce(): string {
  let out = '';
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  for (let i = 0; i < 32; i++) out += chars.charAt(Math.floor(Math.random() * chars.length));
  return out;
}

function escapeAttr(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
}

/** The panel: a toolbar and an iframe of the preview server. */
export function previewHtml(url: string, scriptNonce: string): string {
  const origin = new URL(url).origin;
  const src = escapeAttr(url);
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; frame-src ${origin}; style-src 'unsafe-inline'; script-src 'nonce-${scriptNonce}';">
<style>
  html, body { margin: 0; padding: 0; height: 100%; overflow: hidden; background: var(--vscode-editor-background); }
  .bar { display: flex; gap: 4px; align-items: center; height: 28px; padding: 0 6px;
         border-bottom: 1px solid var(--vscode-panel-border); color: var(--vscode-foreground);
         font: 12px var(--vscode-font-family); }
  .bar button { background: none; border: none; color: inherit; cursor: pointer; padding: 2px 6px; }
  .bar button:hover { background: var(--vscode-toolbar-hoverBackground); }
  .bar .url { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; opacity: 0.7; }
  iframe { border: none; width: 100%; height: calc(100% - 29px); background: white; }
</style>
</head>
<body>
<div class="bar">
  <button id="reload" title="Reload">&#x21bb;</button>
  <span class="url">${src}</span>
  <button id="external" title="Open in browser">&#x2197;</button>
</div>
<iframe id="frame" src="${src}" sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-modals"></iframe>
<script nonce="${scriptNonce}">
  const vscode = acquireVsCodeApi();
  const frame = document.getElementById('frame');
  document.getElementById('reload').addEventListener('click', () => { frame.src = frame.src; });
  document.getElementById('external').addEventListener('click', () => {
    vscode.postMessage({ type: 'open-external', url: ${JSON.stringify(url)} });
  });
</script>
</body>
</html>`;
}
