/**
 * Two-way binding between one open VS Code text buffer and one Automerge
 * text document, plus remote-cursor decorations. Used by both modes: the
 * single-file `quartohub:` buffers and files inside a synced project folder.
 */
import * as vscode from 'vscode';
import { splice, type DocHandle } from '@automerge/automerge-repo/slim';
import { Presence, type Identity, type RemoteCursor } from './presence';

export type TextDoc = { text: string };

interface PeerDecorations {
  cursor: vscode.TextEditorDecorationType;
  selection: vscode.TextEditorDecorationType;
}

/** uri.toString() -> binding */
export const bindings = new Map<string, Binding>();

export class Binding {
  private readonly presence: Presence;
  private cursors: RemoteCursor[] = [];
  private readonly decorations = new Map<string, PeerDecorations>();
  /** Set while we write remote changes into the buffer, so they aren't echoed back. */
  private applyingRemote = false;
  private readonly onChange = () => void this.applyRemote();

  constructor(
    readonly document: vscode.TextDocument,
    readonly handle: DocHandle<TextDoc>,
    identity: Identity,
  ) {
    this.presence = new Presence(handle, identity, (cursors) => {
      this.cursors = cursors;
      this.render();
    });
    handle.on('change', this.onChange);
    bindings.set(document.uri.toString(), this);
    void this.applyRemote(); // buffer may lag the doc at bind time
  }

  dispose(): void {
    bindings.delete(this.document.uri.toString());
    this.handle.off('change', this.onChange);
    this.presence.dispose();
    for (const d of this.decorations.values()) {
      d.cursor.dispose();
      d.selection.dispose();
    }
    this.decorations.clear();
  }

  /** Local keystrokes -> Automerge splices. */
  onLocalEdit(e: vscode.TextDocumentChangeEvent): void {
    if (this.applyingRemote || e.contentChanges.length === 0) return;
    // VS Code reports a single event's changes against the pre-edit text in
    // descending offset order, so applying them sequentially needs no
    // adjustment (the same contract hub-client relies on with Monaco).
    this.handle.change((doc) => {
      for (const c of e.contentChanges) splice(doc, ['text'], c.rangeOffset, c.rangeLength, c.text);
    });
  }

  onSelection(e: vscode.TextEditorSelectionChangeEvent): void {
    const sel = e.selections[0];
    if (!sel) return;
    const d = this.document;
    const start = d.offsetAt(sel.start);
    const end = d.offsetAt(sel.end);
    this.presence.update(d.offsetAt(sel.active), start === end ? null : { start, end });
  }

  /** Automerge changes -> buffer. One replace covering the changed span. */
  private async applyRemote(): Promise<void> {
    const target = this.handle.doc()?.text;
    if (target === undefined) return;
    const buffer = this.document;
    const current = buffer.getText();
    if (current === target) return; // our own edit echoing back, or nothing new

    let prefix = 0;
    const max = Math.min(current.length, target.length);
    while (prefix < max && current[prefix] === target[prefix]) prefix++;
    let suffix = 0;
    while (suffix < max - prefix && current[current.length - 1 - suffix] === target[target.length - 1 - suffix]) suffix++;

    const range = new vscode.Range(buffer.positionAt(prefix), buffer.positionAt(current.length - suffix));
    const edit = new vscode.WorkspaceEdit();
    edit.replace(buffer.uri, range, target.slice(prefix, target.length - suffix));

    this.applyingRemote = true;
    try {
      await vscode.workspace.applyEdit(edit);
    } finally {
      this.applyingRemote = false;
    }
    // Cursor offsets were re-resolved against the new doc while the buffer
    // was still the old text (render skipped them); draw them now.
    this.render();
    // A keystroke that landed mid-apply was dropped by the guard above; if the
    // two sides now disagree, run again so the buffer converges on the doc.
    if (buffer.getText() !== (this.handle.doc()?.text ?? '')) void this.applyRemote();
  }

  render(): void {
    const target = this.handle.doc()?.text;
    const editors = vscode.window.visibleTextEditors.filter(
      (ed) =>
        ed.document === this.document &&
        // Offsets are relative to the Automerge text; while the buffer still
        // lags a remote edit (applyRemote is async) they would map to the
        // wrong spot, so leave the previous decorations in place for now.
        ed.document.getText() === target,
    );
    const live = new Set(this.cursors.map((c) => c.peerId));
    for (const [peerId, deco] of this.decorations) {
      if (!live.has(peerId)) {
        deco.cursor.dispose();
        deco.selection.dispose();
        this.decorations.delete(peerId);
      }
    }
    for (const c of this.cursors) {
      let deco = this.decorations.get(c.peerId);
      if (!deco) {
        deco = createPeerDecorations(c.userColor, c.userName);
        this.decorations.set(c.peerId, deco);
      }
      for (const ed of editors) {
        const at = (o: number) => ed.document.positionAt(o);
        const cursorPos = c.cursor !== null ? at(c.cursor) : c.selection ? at(c.selection.end) : null;
        ed.setDecorations(deco.cursor, cursorPos ? [new vscode.Range(cursorPos, cursorPos)] : []);
        ed.setDecorations(deco.selection, c.selection ? [new vscode.Range(at(c.selection.start), at(c.selection.end))] : []);
      }
    }
  }
}

/**
 * Same look as hub-client's remote cursors (hooks/usePresence.ts): a 2px bar
 * in the user's exact colour with a small name flag above it, plus a faint
 * tint for selections. For an empty range VS Code inserts the before/after
 * spans straight into the line with no wrapping span to position against, so
 * both attachments are inline-blocks that take no horizontal space; the extra
 * declarations ride in through the `textDecoration` escape hatch.
 */
function createPeerDecorations(color: string, name: string): PeerDecorations {
  const cursor = vscode.window.createTextEditorDecorationType({
    before: {
      contentText: '',
      backgroundColor: color,
      textDecoration: 'none; display: inline-block; width: 2px; height: 1.1em; margin: 0 -1px; vertical-align: text-bottom;',
    },
    after: {
      contentText: name,
      color,
      textDecoration:
        'none; display: inline-block; width: 0; overflow: visible; position: relative; top: -1.05em; left: 2px; ' +
        'font-size: 10px; font-weight: 600; line-height: 1; white-space: nowrap; pointer-events: none; z-index: 10; ' +
        'text-shadow: 0 0 3px var(--vscode-editor-background), 0 0 3px var(--vscode-editor-background);',
    },
  });
  const selection = vscode.window.createTextEditorDecorationType({
    // Translucent on purpose: this is an overlay on top of text.
    backgroundColor: `${color}33`,
  });
  return { cursor, selection };
}

/** Workspace-wide listeners that dispatch to whichever binding owns the buffer. */
export function registerBindingEvents(): vscode.Disposable[] {
  return [
    vscode.workspace.onDidChangeTextDocument((e) => bindings.get(e.document.uri.toString())?.onLocalEdit(e)),
    vscode.window.onDidChangeTextEditorSelection((e) => bindings.get(e.textEditor.document.uri.toString())?.onSelection(e)),
    vscode.window.onDidChangeVisibleTextEditors(() => bindings.forEach((b) => b.render())),
  ];
}
