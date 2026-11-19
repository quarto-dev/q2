/**
 * The vscode-free part of the two-way binding between one text buffer and one
 * Automerge text document (extracted from `Binding`, GE-8): the prefix and
 * suffix diff that turns a remote change into one replace, local edits
 * spliced into the doc, the echo guard, and the convergence pass.
 *
 * The editor side is a `BufferText`: VS Code implements it with a
 * `WorkspaceEdit`, tests with a string.
 */
import { splice, type DocHandle } from '@automerge/automerge-repo/slim';

export type TextDoc = { text: string };

export interface BufferText {
  getText(): string;
  /** Replace `[start, end)` (UTF-16 offsets) with `text`. May yield. */
  applyReplace(start: number, end: number, text: string): Promise<void>;
}

/** One content change of an editor event, against the pre-edit text. */
export interface TextChange {
  rangeOffset: number;
  rangeLength: number;
  text: string;
}

export interface Replace {
  start: number;
  end: number;
  text: string;
}

/** The one replace that turns `current` into `target`, or null if equal. */
export function computeReplace(current: string, target: string): Replace | null {
  if (current === target) return null;
  let prefix = 0;
  const max = Math.min(current.length, target.length);
  while (prefix < max && current[prefix] === target[prefix]) prefix++;
  let suffix = 0;
  while (suffix < max - prefix && current[current.length - 1 - suffix] === target[target.length - 1 - suffix]) suffix++;
  return { start: prefix, end: current.length - suffix, text: target.slice(prefix, target.length - suffix) };
}

export class TextBindingCore {
  /** Set while we write remote changes into the buffer, so they aren't echoed back. */
  private applyingRemote = false;
  private readonly onChange = () => void this.applyRemote();
  /** Called after each remote apply (the VS Code layer redraws cursors). */
  afterApply: (() => void) | null = null;

  constructor(
    readonly handle: DocHandle<TextDoc>,
    private readonly buffer: BufferText,
  ) {}

  get applying(): boolean {
    return this.applyingRemote;
  }

  start(): void {
    this.handle.on('change', this.onChange);
    void this.applyRemote(); // the buffer may lag the doc at bind time
  }

  dispose(): void {
    this.handle.off('change', this.onChange);
  }

  /** Local keystrokes -> Automerge splices. */
  onLocalEdit(changes: readonly TextChange[]): void {
    if (this.applyingRemote || changes.length === 0) return;
    // An editor reports a single event's changes against the pre-edit text in
    // descending offset order, so applying them sequentially needs no
    // adjustment (the same contract hub-client relies on with Monaco).
    this.handle.change((doc) => {
      for (const c of changes) splice(doc, ['text'], c.rangeOffset, c.rangeLength, c.text);
    });
  }

  /** Automerge changes -> buffer. One replace covering the changed span. */
  async applyRemote(): Promise<void> {
    const target = this.handle.doc()?.text;
    if (target === undefined) return;
    const replace = computeReplace(this.buffer.getText(), target);
    if (!replace) return; // our own edit echoing back, or nothing new

    this.applyingRemote = true;
    try {
      await this.buffer.applyReplace(replace.start, replace.end, replace.text);
    } finally {
      this.applyingRemote = false;
    }
    this.afterApply?.();
    // A keystroke that landed mid-apply was dropped by the guard above; if the
    // two sides now disagree, run again so the buffer converges on the doc.
    if (this.buffer.getText() !== (this.handle.doc()?.text ?? '')) void this.applyRemote();
  }
}
