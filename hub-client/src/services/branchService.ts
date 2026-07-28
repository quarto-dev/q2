/**
 * Branch Service — local-only per-document branches ("forks").
 *
 * Each text file in a project is one synced automerge doc (its implicit
 * "main" branch). A branch is `A.clone` of that doc at fork time: a plain
 * `@automerge/automerge` document that is deliberately NEVER registered with
 * an automerge-repo `Repo`. No repo, no network adapter — a branch cannot
 * reach the sync server. Branch docs are persisted to localStorage
 * (`A.save` → base64) so they survive reloads on this browser only.
 *
 * "Merge to main" is a true CRDT merge: `handle.update(d => A.merge(d,
 * branchDoc))` on the file's synced DocHandle. Because the branch shares
 * history with main (clone, not copy), concurrent main edits made while the
 * branch diverged interleave cleanly, and the merge propagates to peers
 * through the ordinary sync-client change path (VFS, Monaco, preview).
 *
 * Module-level singleton with a subscribe/notify surface, following the
 * established service pattern (cf. presenceService, projectSetService).
 * State is keyed by the file's automerge documentId (stable across renames);
 * the API is keyed by path, which is the UI's currency.
 */

import { next as A } from '@automerge/automerge';
import { getFileHandle, type EditorContentChange } from '@quarto/preview-runtime';

export interface BranchMeta {
  id: string;
  name: string;
  createdAt: number;
}

/** Structural view of a text file doc; matches TextDocumentContent. */
interface FileDocShape {
  text: string;
}

/** The subset of DocHandle the service needs (structural, for testability). */
interface FileHandleLike {
  documentId: string;
  doc(): unknown;
  update(cb: (d: A.Doc<FileDocShape>) => A.Doc<FileDocShape>): void;
}

type HandleGetter = (path: string) => FileHandleLike | null | undefined;

const defaultHandleGetter: HandleGetter = (path) =>
  getFileHandle(path) as unknown as FileHandleLike | null | undefined;

let handleGetter: HandleGetter = defaultHandleGetter;

const INDEX_KEY_PREFIX = 'qh-doc-branches:';
const DOC_KEY_PREFIX = 'qh-branch-doc:';

// ── Module state ────────────────────────────────────────────────────────

/** In-memory branch docs, keyed `${docId}:${branchId}`. */
let branchDocs = new Map<string, A.Doc<FileDocShape>>();
/** Active branch id per file path; absent/null means main. */
let activeByPath = new Map<string, string | null>();
let listeners = new Set<() => void>();

// ── Helpers ─────────────────────────────────────────────────────────────

function indexKey(docId: string): string {
  return `${INDEX_KEY_PREFIX}${docId}`;
}

function docKey(docId: string, branchId: string): string {
  return `${DOC_KEY_PREFIX}${docId}:${branchId}`;
}

function memKey(docId: string, branchId: string): string {
  return `${docId}:${branchId}`;
}

function notify(): void {
  for (const listener of listeners) listener();
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function readIndex(docId: string): BranchMeta[] {
  try {
    const raw = localStorage.getItem(indexKey(docId));
    if (!raw) return [];
    const parsed = JSON.parse(raw) as { branches?: BranchMeta[] };
    return Array.isArray(parsed.branches) ? parsed.branches : [];
  } catch {
    return [];
  }
}

function writeIndex(docId: string, branches: BranchMeta[]): void {
  if (branches.length === 0) {
    localStorage.removeItem(indexKey(docId));
  } else {
    localStorage.setItem(indexKey(docId), JSON.stringify({ branches }));
  }
}

function persistDoc(docId: string, branchId: string): void {
  const doc = branchDocs.get(memKey(docId, branchId));
  if (!doc) return;
  try {
    localStorage.setItem(docKey(docId, branchId), bytesToBase64(A.save(doc)));
  } catch (err) {
    // Quota exceeded or storage unavailable — branch keeps working
    // in-memory; it just won't survive a reload.
    console.warn('branchService: failed to persist branch doc', err);
  }
}

function loadDoc(docId: string, branchId: string): A.Doc<FileDocShape> | null {
  const cached = branchDocs.get(memKey(docId, branchId));
  if (cached) return cached;
  const raw = localStorage.getItem(docKey(docId, branchId));
  if (!raw) return null;
  try {
    const doc = A.load<FileDocShape>(base64ToBytes(raw));
    branchDocs.set(memKey(docId, branchId), doc);
    return doc;
  } catch (err) {
    console.warn('branchService: failed to load branch doc', err);
    return null;
  }
}

function resolveDocId(path: string): string | null {
  const handle = handleGetter(path);
  return handle ? handle.documentId : null;
}

function generateBranchId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

// ── Public API ──────────────────────────────────────────────────────────

export function getBranches(path: string): BranchMeta[] {
  const docId = resolveDocId(path);
  return docId ? readIndex(docId) : [];
}

export function getActiveBranchId(path: string): string | null {
  return activeByPath.get(path) ?? null;
}

export function setActiveBranch(path: string, branchId: string | null): void {
  if (branchId !== null) {
    const docId = resolveDocId(path);
    if (!docId || loadDoc(docId, branchId) === null) return; // unknown branch
  }
  activeByPath.set(path, branchId);
  notify();
}

/**
 * Fork the currently viewed state of `path` (main, or the active branch)
 * into a new local-only branch, and make it active.
 * Returns null for unknown files and non-text documents.
 */
export function createBranch(path: string, name?: string): BranchMeta | null {
  const handle = handleGetter(path);
  if (!handle) return null;
  const docId = handle.documentId;

  const activeId = getActiveBranchId(path);
  const sourceDoc = activeId !== null
    ? loadDoc(docId, activeId)
    : (handle.doc() as A.Doc<FileDocShape> | undefined);
  if (!sourceDoc || typeof (sourceDoc as FileDocShape).text !== 'string') {
    return null; // not loaded yet, or a binary document
  }

  const branches = readIndex(docId);
  let finalName = name?.trim() ?? '';
  if (!finalName) {
    let n = branches.length + 1;
    while (branches.some((b) => b.name === `fork-${n}`)) n += 1;
    finalName = `fork-${n}`;
  }

  const meta: BranchMeta = {
    id: generateBranchId(),
    name: finalName,
    createdAt: Date.now(),
  };

  // A.clone shares full history with the source but gets a fresh actor id,
  // which is exactly what makes the eventual A.merge back into main clean.
  const branchDoc = A.clone(sourceDoc);
  branchDocs.set(memKey(docId, meta.id), branchDoc);
  writeIndex(docId, [...branches, meta]);
  persistDoc(docId, meta.id);
  activeByPath.set(path, meta.id);
  notify();
  return meta;
}

export function getBranchText(path: string, branchId: string): string | null {
  const docId = resolveDocId(path);
  if (!docId) return null;
  const doc = loadDoc(docId, branchId);
  if (!doc) return null;
  const text = (doc as FileDocShape).text;
  return typeof text === 'string' ? text : null;
}

/**
 * Apply Monaco editor operations to a branch doc as automerge splices.
 * Mirrors the sync client's `applyEditorOperations` for main.
 */
export function applyBranchEdits(path: string, branchId: string, changes: EditorContentChange[]): void {
  if (changes.length === 0) return;
  const docId = resolveDocId(path);
  if (!docId) return;
  const doc = loadDoc(docId, branchId);
  if (!doc) return;

  const updated = A.change(doc, (d) => {
    for (const change of changes) {
      A.splice(d, ['text'], change.rangeOffset, change.rangeLength, change.text);
    }
  });
  branchDocs.set(memKey(docId, branchId), updated);
  persistDoc(docId, branchId);
}

/**
 * CRDT-merge a branch back into the file's synced main doc, then delete the
 * branch and return to main. The `handle.update` fires the sync client's
 * normal change path, so peers, VFS, and the editor all pick up the result.
 */
export function mergeBranchToMain(path: string, branchId: string): boolean {
  const handle = handleGetter(path);
  if (!handle) return false;
  const branchDoc = loadDoc(handle.documentId, branchId);
  if (!branchDoc) return false;

  try {
    handle.update((mainDoc) => A.merge(mainDoc, branchDoc));
  } catch (err) {
    console.error('branchService: merge failed', err);
    return false;
  }
  deleteBranch(path, branchId);
  return true;
}

export function deleteBranch(path: string, branchId: string): void {
  const docId = resolveDocId(path);
  if (!docId) return;
  writeIndex(docId, readIndex(docId).filter((b) => b.id !== branchId));
  localStorage.removeItem(docKey(docId, branchId));
  branchDocs.delete(memKey(docId, branchId));
  if (getActiveBranchId(path) === branchId) {
    activeByPath.set(path, null);
  }
  notify();
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

// ── Testing utilities ───────────────────────────────────────────────────

/** @internal For testing only. */
export function _resetForTesting(opts?: { keepStorage?: boolean }): void {
  branchDocs = new Map();
  activeByPath = new Map();
  listeners = new Set();
  handleGetter = defaultHandleGetter;
  if (!opts?.keepStorage && typeof localStorage !== 'undefined') {
    const doomed: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key && (key.startsWith(INDEX_KEY_PREFIX) || key.startsWith(DOC_KEY_PREFIX))) {
        doomed.push(key);
      }
    }
    doomed.forEach((k) => localStorage.removeItem(k));
  }
}

/** @internal For testing only. Pass null to restore the default getter. */
export function _setHandleGetterForTesting(getter: HandleGetter | null): void {
  handleGetter = getter ?? defaultHandleGetter;
}
