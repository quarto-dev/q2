/**
 * What the sync core needs from its host (GE-8). The VS Code extension and
 * the headless peer each supply one; the core imports nothing from `vscode`.
 *
 * Deviation from the plan's first sketch: the buffer host *binds* a buffer
 * to a doc (`bind`) instead of exposing `applyReplace`/`onEdit`, because the
 * VS Code binding also owns presence and cursor decorations, which are not
 * the core's business. The text logic itself (diff, splice, echo guard,
 * convergence pass) is shared: `TextBindingCore`.
 */
import type { DocHandle } from '@automerge/automerge-repo/slim';
import type { Identity } from './presence';
import type { TextDoc } from './text-binding';

/** An `OutputChannel` satisfies this. */
export interface Logger {
  appendLine(line: string): void;
}

/** Disk change events; every path is absolute. */
export interface FileWatcher {
  onChange(cb: (absPath: string) => void): void;
  onCreate(cb: (absPath: string) => void): void;
  onDelete(cb: (absPath: string) => void): void;
  dispose(): void;
}

export interface BufferBinding {
  dispose(): void;
  /** Tell the others on this doc that this peer is leaving it because of a version switch (G25). */
  sendMoved?(): void;
  /** Bring the buffer to the doc's text now (used when a suspended binding resumes). */
  reconcile?(): void;
  /** A detached file (G27): drop local edits and skip remote changes, keeping the handle. */
  setInert?(inert: boolean): void;
}

/** What the sync core asks of a binding it asks the host to create. */
export interface BindHooks {
  /** An accepted local edit batch (the follower counts those that land in a retired doc). */
  onLocalEdit?(): void;
  /** True while the binding must neither send local edits nor write the doc into the buffer (the editor-only gate). */
  suspended?(): boolean;
  /** A peer left this doc for a new version; `name` is that peer's presence name when known (G25). */
  onMoved?(name: string | undefined): void;
}

/** The editor buffers the host has open. A headless host has none. */
export interface BufferHost {
  /** Text of the open buffer for `absPath`, or null when it is not open. */
  getText(absPath: string): string | null;
  /** True when the open buffer has unsaved changes. */
  isDirty(absPath: string): boolean;
  /** Absolute paths of every open buffer. */
  openPaths(): string[];
  onOpen(cb: (absPath: string) => void): void;
  onClose(cb: (absPath: string) => void): void;
  /**
   * Bind the open buffer to `handle` (two-way text sync, plus whatever the
   * host adds, such as cursors). Null when the host cannot bind it.
   */
  bind(absPath: string, handle: DocHandle<TextDoc>, identity: Identity, hooks?: BindHooks): BufferBinding | null;
  dispose(): void;
}

export interface Notifier {
  info(message: string, ...actions: string[]): Promise<string | undefined>;
  warn(message: string, ...actions: string[]): Promise<string | undefined>;
  error(message: string, ...actions: string[]): Promise<string | undefined>;
  /** A transient status line; `timeoutMs` omitted keeps it until replaced. */
  status(text: string | null, timeoutMs?: number): void;
}

export interface SyncHost {
  log: Logger;
  watcher: FileWatcher;
  buffers: BufferHost;
  notifier: Notifier;
}
