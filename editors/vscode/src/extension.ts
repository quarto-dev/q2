/**
 * Quarto 2 for VS Code (epic bd-xnpyb2vv; plan
 * claude-notes/plans/2026-11-17-q2-vscode-extension.md).
 *
 * Starts `q2 lsp` over stdio for `quarto2` documents. Highlighting,
 * diagnostics and the outline all come from the server; this file only
 * finds the binary and manages the client's lifetime.
 */
import * as vscode from 'vscode';
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
} from 'vscode-languageclient/node';

import { findQ2FromEnvironment } from './binary';

export const LANGUAGE_ID = 'quarto2';
const INSTALL_URL = 'https://github.com/quarto-dev/q2#installation';

let client: LanguageClient | undefined;
let output: vscode.OutputChannel;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  output = vscode.window.createOutputChannel('Quarto 2');
  context.subscriptions.push(output);

  context.subscriptions.push(
    vscode.commands.registerCommand('q2.restartServer', async () => {
      await stopClient();
      await startClient(context);
    }),
    vscode.commands.registerCommand('q2.showOutput', () => output.show()),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration('q2.path')) {
        await stopClient();
        await startClient(context);
      }
    }),
  );

  await startClient(context);
}

export async function deactivate(): Promise<void> {
  await stopClient();
}

/** The running client, for other modules (the preview, P5). */
export function currentClient(): LanguageClient | undefined {
  return client;
}

async function startClient(context: vscode.ExtensionContext): Promise<void> {
  const configured = vscode.workspace.getConfiguration('q2').get<string>('path') ?? '';
  const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const binary = findQ2FromEnvironment(configured, root);
  if (!binary.found) {
    output.appendLine(binary.reason);
    const choice = await vscode.window.showErrorMessage(
      `Quarto 2: ${binary.reason} Install q2 or set q2.path.`,
      'Install instructions',
      'Open settings',
    );
    if (choice === 'Install instructions') {
      void vscode.env.openExternal(vscode.Uri.parse(INSTALL_URL));
    } else if (choice === 'Open settings') {
      void vscode.commands.executeCommand('workbench.action.openSettings', 'q2.path');
    }
    return;
  }
  output.appendLine(`Using q2 at ${binary.path} (from ${binary.source}).`);

  const serverOptions: ServerOptions = { command: binary.path, args: ['lsp'] };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ language: LANGUAGE_ID }],
    outputChannel: output,
    initializationOptions: {},
  };
  client = new LanguageClient('q2', 'Quarto 2', serverOptions, clientOptions);
  try {
    await client.start();
  } catch (e) {
    output.appendLine(`q2 lsp failed to start: ${String(e)}`);
    void vscode.window.showErrorMessage('Quarto 2: q2 lsp failed to start. See the Quarto 2 output.');
    client = undefined;
    return;
  }
  const info = client.initializeResult?.serverInfo;
  output.appendLine(`Connected to ${info?.name ?? 'q2 lsp'} ${info?.version ?? ''}`.trimEnd());
  context.subscriptions.push({ dispose: () => void stopClient() });
}

async function stopClient(): Promise<void> {
  const c = client;
  client = undefined;
  if (c) {
    try {
      await c.stop();
    } catch (e) {
      output.appendLine(`Stopping q2 lsp: ${String(e)}`);
    }
  }
}
