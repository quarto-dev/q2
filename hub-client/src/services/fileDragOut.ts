/**
 * Drag a project file out of the browser onto the desktop (or into
 * another app) as a real file.
 *
 * Chromium implements this through the non-standard `DownloadURL`
 * dataTransfer type: `"<mime>:<filename>:<url>"`. On drop outside the
 * page the browser fetches the URL and saves it under the filename.
 * Per MDN (Drag data store → "Dragging files to an operating system file
 * explorer") no other browser offers an equivalent for arbitrary files:
 * Firefox and Safari only save natively dragged <img> elements. There,
 * the type is ignored and the drag does nothing outside the page.
 *
 * The URL is a `data:` URL over the file's current bytes, so nothing has
 * to stay alive (or be revoked) after the drag ends.
 */

import { inferMimeType } from '@quarto/quarto-automerge-schema';
import {
  exportFolderAsZip,
  getBinaryFileContent,
  getFileContent,
  isFileBinary,
} from '@quarto/preview-runtime';

function toBase64(bytes: Uint8Array): string {
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

interface FileBytes {
  name: string;
  mime: string;
  /** Text content, or null for binary. */
  text: string | null;
  /** Binary content, or null for text. */
  bytes: Uint8Array | null;
}

/** Current bytes of `path`, or null if not loaded / not connected. */
function readFileBytes(path: string): FileBytes | null {
  const name = path.split('/').pop() || path;
  try {
    if (isFileBinary(path)) {
      const binary = getBinaryFileContent(path);
      if (!binary) return null;
      return { name, mime: binary.mimeType || inferMimeType(path), text: null, bytes: binary.content };
    }
    const text = getFileContent(path);
    if (text === null) return null;
    // The MIME table has no entry for .qmd/.md/.yml; call text "text".
    const inferred = inferMimeType(path);
    const mime = inferred === 'application/octet-stream' ? 'text/plain' : inferred;
    return { name, mime, text, bytes: null };
  } catch {
    // Not connected (e.g. dev harness).
    return null;
  }
}

/** `DownloadURL` payload for `path`, or null if its bytes aren't loaded. */
export function prepareDragOut(path: string): string | null {
  const file = readFileBytes(path);
  if (!file) return null;
  const url =
    file.bytes !== null
      ? `data:${file.mime};base64,${toBase64(file.bytes)}`
      : `data:${file.mime};charset=utf-8,${encodeURIComponent(file.text ?? '')}`;
  return `${file.mime}:${file.name}:${url}`;
}

/** Zip a folder's contents, named after the folder. Null if it holds no files. */
function readFolderZip(folder: string): { name: string; bytes: Uint8Array } | null {
  try {
    const bytes = exportFolderAsZip(folder);
    if (!bytes) return null;
    return { name: `${folder.split('/').pop() || folder}.zip`, bytes };
  } catch {
    return null;
  }
}

/**
 * `DownloadURL` payload for a folder, as a zip. A drag can carry only one
 * URL, and no browser lets a page hand the OS a directory, so this is the
 * only shape a folder drag-out can take. Null if the folder holds no files.
 */
export function prepareFolderDragOut(folder: string): string | null {
  const zip = readFolderZip(folder);
  if (!zip) return null;
  return `application/zip:${zip.name}:data:application/zip;base64,${toBase64(zip.bytes)}`;
}

function triggerDownload(blob: Blob, name: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.rel = 'noopener';
  document.body.appendChild(a);
  a.click();
  a.remove();
  // Let the download start before dropping the URL.
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

/**
 * Save `path` through the browser's download flow (works in every
 * browser, unlike drag-out). Returns false if the bytes aren't loaded.
 */
export function downloadFile(path: string): boolean {
  const file = readFileBytes(path);
  if (!file) return false;
  const blob = new Blob([file.bytes !== null ? (file.bytes as BlobPart) : (file.text ?? '')], {
    type: file.mime,
  });
  triggerDownload(blob, file.name);
  return true;
}

/** Save a folder as `<folder>.zip`. Returns false if it holds no files. */
export function downloadFolderAsZip(folder: string): boolean {
  const zip = readFolderZip(folder);
  if (!zip) return false;
  triggerDownload(new Blob([zip.bytes as BlobPart], { type: 'application/zip' }), zip.name);
  return true;
}
