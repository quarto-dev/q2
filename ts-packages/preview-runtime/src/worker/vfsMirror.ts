/**
 * The main thread's copy of the VFS when the engine runs in a worker
 * (preview VFS contract D3, D9).
 *
 * The worker owns the real VFS. Code on the main thread that reads it
 * synchronously (the theme CSS for the iframe, the asset manifest, the
 * iframe post-processor, the PDF pane) reads this mirror instead. It
 * holds every file the sync client wrote (each write also goes to the
 * worker) and the render artifacts under `/.quarto/` that each render
 * response carries back.
 *
 * Answers use the same `{ success, content | files | error }` envelope as
 * the WASM exports, so callers cannot tell the two apart.
 */

import type { VfsResponse } from '../wasmRenderer';

/** Render artifacts that changed in the worker's VFS during a render. */
export interface ArtifactDelta {
  changed: Array<{ path: string; text?: string; base64?: string }>;
  removed: string[];
}

type Entry = { text: string } | { bytes: Uint8Array; base64?: string };

const PROJECT_ROOT = '/project';

/**
 * Normalize a path the way `quarto_system_runtime::VirtualFileSystem` does:
 * a relative path lives under `/project`; `.` and `..` resolve lexically and
 * never climb above `/`.
 */
export function normalizeVfsPath(path: string): string {
  const absolute = path.startsWith('/') ? path : `${PROJECT_ROOT}/${path}`;
  const parts: string[] = [];
  for (const part of absolute.split('/')) {
    if (part === '' || part === '.') continue;
    if (part === '..') parts.pop();
    else parts.push(part);
  }
  return '/' + parts.join('/');
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

const utf8 = new TextDecoder('utf-8', { fatal: true });

/** Whether an entry holds exactly `bytes`. */
function sameContent(entry: Entry, bytes: Uint8Array): boolean {
  const held = 'text' in entry ? new TextEncoder().encode(entry.text) : entry.bytes;
  if (held.length !== bytes.length) return false;
  for (let i = 0; i < held.length; i++) if (held[i] !== bytes[i]) return false;
  return true;
}

export class VfsMirror {
  private readonly files = new Map<string, Entry>();

  /**
   * Write a text file. `changed` is false when identical content was
   * already there (preview VFS contract D2): the worker's VFS skips the
   * same write, and the sync client skips signalling a content change.
   */
  addText(path: string, content: string): VfsResponse {
    const key = normalizeVfsPath(path);
    const existing = this.files.get(key);
    if (existing && sameContent(existing, new TextEncoder().encode(content))) {
      return { success: true, changed: false };
    }
    this.files.set(key, { text: content });
    return { success: true, changed: true };
  }

  /** Write a binary file; `changed` as for {@link addText}. */
  addBinary(path: string, content: Uint8Array): VfsResponse {
    const key = normalizeVfsPath(path);
    const existing = this.files.get(key);
    if (existing && sameContent(existing, content)) {
      return { success: true, changed: false };
    }
    this.files.set(key, { bytes: content });
    return { success: true, changed: true };
  }

  remove(path: string): VfsResponse {
    return this.files.delete(normalizeVfsPath(path))
      ? { success: true }
      : { success: false, error: 'File not found' };
  }

  clear(): VfsResponse {
    this.files.clear();
    return { success: true };
  }

  listFiles(): VfsResponse {
    return { success: true, files: [...this.files.keys()] };
  }

  readFile(path: string): VfsResponse {
    const entry = this.files.get(normalizeVfsPath(path));
    if (!entry) return { success: false, error: `Failed to read file: ${path} not found` };
    if ('text' in entry) return { success: true, content: entry.text };
    try {
      return { success: true, content: utf8.decode(entry.bytes) };
    } catch {
      return { success: false, error: 'File is not valid UTF-8' };
    }
  }

  readBinaryFile(path: string): VfsResponse {
    const entry = this.files.get(normalizeVfsPath(path));
    if (!entry) return { success: false, error: `Failed to read file: ${path} not found` };
    if ('text' in entry) {
      return { success: true, content: bytesToBase64(new TextEncoder().encode(entry.text)) };
    }
    entry.base64 ??= bytesToBase64(entry.bytes);
    return { success: true, content: entry.base64 };
  }

  /** Apply the artifact changes a render response carried back. */
  applyArtifacts(delta: ArtifactDelta): void {
    for (const path of delta.removed) this.files.delete(normalizeVfsPath(path));
    for (const { path, text, base64 } of delta.changed) {
      if (text !== undefined) {
        this.files.set(normalizeVfsPath(path), { text });
      } else if (base64 !== undefined) {
        this.files.set(normalizeVfsPath(path), { bytes: base64ToBytes(base64), base64 });
      }
    }
  }
}
