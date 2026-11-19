/**
 * The VS Code implementations of the core's host interfaces (core/host.ts):
 * a file watcher, the open text buffers, and notifications.
 */
import * as vscode from 'vscode';
import type { DocHandle } from '@automerge/automerge-repo/slim';
import { Binding } from './binding';
import type { BindHooks, BufferBinding, BufferHost, FileWatcher, Notifier } from './core/host';
import type { Identity } from './core/presence';
import type { TextDoc } from './core/text-binding';

export class VscodeFileWatcher implements FileWatcher {
  private readonly watcher: vscode.FileSystemWatcher;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(root: string) {
    this.watcher = vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(root, '**/*'));
    this.disposables.push(this.watcher);
  }
  private forward(cb: (absPath: string) => void) {
    return (uri: vscode.Uri) => {
      if (uri.scheme === 'file') cb(uri.fsPath);
    };
  }
  onChange(cb: (absPath: string) => void): void {
    this.disposables.push(this.watcher.onDidChange(this.forward(cb)));
  }
  onCreate(cb: (absPath: string) => void): void {
    this.disposables.push(this.watcher.onDidCreate(this.forward(cb)));
  }
  onDelete(cb: (absPath: string) => void): void {
    this.disposables.push(this.watcher.onDidDelete(this.forward(cb)));
  }
  dispose(): void {
    this.disposables.forEach((d) => d.dispose());
    this.disposables.length = 0;
  }
}

export class VscodeBufferHost implements BufferHost {
  private readonly disposables: vscode.Disposable[] = [];

  private docOf(absPath: string): vscode.TextDocument | undefined {
    const uri = vscode.Uri.file(absPath).toString();
    return vscode.workspace.textDocuments.find((d) => d.uri.toString() === uri);
  }
  getText(absPath: string): string | null {
    return this.docOf(absPath)?.getText() ?? null;
  }
  isDirty(absPath: string): boolean {
    return this.docOf(absPath)?.isDirty ?? false;
  }
  openPaths(): string[] {
    return vscode.workspace.textDocuments.filter((d) => d.uri.scheme === 'file').map((d) => d.uri.fsPath);
  }
  onOpen(cb: (absPath: string) => void): void {
    this.disposables.push(vscode.workspace.onDidOpenTextDocument((d) => d.uri.scheme === 'file' && cb(d.uri.fsPath)));
  }
  onClose(cb: (absPath: string) => void): void {
    this.disposables.push(vscode.workspace.onDidCloseTextDocument((d) => d.uri.scheme === 'file' && cb(d.uri.fsPath)));
  }
  bind(absPath: string, handle: DocHandle<TextDoc>, identity: Identity, hooks?: BindHooks): BufferBinding | null {
    const document = this.docOf(absPath);
    if (!document) return null;
    const binding = new Binding(document, handle, identity, hooks);
    return {
      dispose: () => binding.dispose(),
      sendMoved: () => binding.sendMoved(),
      reconcile: () => binding.reconcile(),
      setInert: (inert) => binding.setInert(inert),
    };
  }
  dispose(): void {
    this.disposables.forEach((d) => d.dispose());
    this.disposables.length = 0;
  }
}

export class VscodeNotifier implements Notifier {
  /** The persistent item (the mode label); created on first use. */
  private item: vscode.StatusBarItem | null = null;

  info(message: string, ...actions: string[]): Promise<string | undefined> {
    return Promise.resolve(vscode.window.showInformationMessage(`Quarto Hub: ${message}`, ...actions));
  }
  warn(message: string, ...actions: string[]): Promise<string | undefined> {
    return Promise.resolve(vscode.window.showWarningMessage(`Quarto Hub: ${message}`, ...actions));
  }
  error(message: string, ...actions: string[]): Promise<string | undefined> {
    return Promise.resolve(vscode.window.showErrorMessage(`Quarto Hub: ${message}`, ...actions));
  }
  status(text: string | null, timeoutMs?: number): void {
    if (text === null) {
      this.item?.dispose();
      this.item = null;
      return;
    }
    if (timeoutMs !== undefined) {
      vscode.window.setStatusBarMessage(text, timeoutMs);
      return;
    }
    if (!this.item) {
      this.item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 0);
      this.item.command = 'quartoHub.detachedFiles'; // lists the detached files in clone mode
    }
    this.item.text = text;
    this.item.show();
  }
}
