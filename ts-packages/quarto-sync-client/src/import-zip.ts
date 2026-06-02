/**
 * Project ZIP import utility.
 *
 * Parses a ZIP archive into a list of files in the shape accepted by
 * {@link CreateProjectOptions.files}, so the result can be handed
 * straight to the existing "create new project" path. This is the
 * inverse of {@link exportProjectAsZip} (see `export-zip.ts`).
 *
 * The function is pure: it does not touch a SyncClient, Automerge, or
 * the network. Callers read the uploaded file's bytes and pass them in.
 */

import { unzipSync, strFromU8 } from 'fflate';
import { isBinaryExtension, isTextExtension, inferMimeType } from '@quarto/quarto-automerge-schema';
import type { CreateProjectOptions } from './types.js';

/** A single parsed file, in the shape `createNewProject` consumes. */
type ParsedFile = CreateProjectOptions['files'][number];

/** Validates UTF-8 strictly so unknown extensions can be sniffed. */
const STRICT_UTF8 = new TextDecoder('utf-8', { fatal: true });

/**
 * Parse a ZIP archive into a list of project files.
 *
 * Text files are returned as UTF-8 strings; binary files are returned
 * base64-encoded with an inferred MIME type — matching what
 * `createNewProject` expects (it `atob`-decodes binary content).
 *
 * Behavior:
 * - A single common leading directory (e.g. the `repo-main/` wrapper a
 *   GitHub "Download ZIP" adds) is stripped so paths are project-relative.
 * - Directory entries and editor/OS junk (`__MACOSX/`, `.DS_Store`,
 *   `.git/`) are dropped.
 * - Entries whose path escapes the project root (absolute paths or `..`
 *   traversal) cause the whole import to be rejected (zip-slip guard).
 *
 * @param zipBytes - Raw bytes of the uploaded `.zip`
 * @returns Files ready to pass as `CreateProjectOptions.files`
 * @throws If the archive is corrupt, contains an unsafe path, or yields
 *   no usable files.
 */
export function parseProjectZip(zipBytes: Uint8Array): ParsedFile[] {
  // unzipSync throws on corrupt input; let that propagate.
  const entries = unzipSync(zipBytes);

  // Keep only real file entries (skip directories and junk). We filter
  // before stripping the common prefix so junk like `__MACOSX/` can't
  // defeat the prefix detection.
  const usablePaths = Object.keys(entries).filter(
    path => !isDirectoryEntry(path) && !isJunkPath(path),
  );

  if (usablePaths.length === 0) {
    throw new Error('No files found in the archive.');
  }

  const prefix = commonLeadingDirectory(usablePaths);

  const files: ParsedFile[] = [];
  for (const rawPath of usablePaths) {
    const path = prefix ? rawPath.slice(prefix.length) : rawPath;

    // Stripping a prefix should never produce an empty path for a file
    // entry, but guard anyway.
    if (path === '') continue;

    if (!isSafePath(path)) {
      throw new Error(`Archive contains an unsafe path: ${rawPath}`);
    }

    const bytes = entries[rawPath];
    files.push(classifyEntry(path, bytes));
  }

  if (files.length === 0) {
    throw new Error('No files found in the archive.');
  }

  return files;
}

/** Directory entries are zero-length and end in a slash. */
function isDirectoryEntry(path: string): boolean {
  return path.endsWith('/');
}

/** OS/editor metadata and VCS internals that should never be imported. */
function isJunkPath(path: string): boolean {
  const segments = path.split('/');
  return segments.some(
    seg => seg === '__MACOSX' || seg === '.DS_Store' || seg === '.git',
  );
}

/**
 * Reject paths that would escape the project root once written. We work
 * on forward-slash ZIP paths; a backslash is treated as a literal
 * filename character by ZIP, but we reject it too since downstream
 * consumers may interpret it as a separator on Windows.
 */
function isSafePath(path: string): boolean {
  if (path.startsWith('/')) return false;
  if (path.includes('\\')) return false;
  const segments = path.split('/');
  return !segments.some(seg => seg === '..');
}

/**
 * If every entry shares the same leading directory segment, return that
 * segment including its trailing slash (e.g. `"repo-main/"`); otherwise
 * return `null`.
 *
 * A single-file archive is left untouched: one nested file is meaningful
 * structure, not a wrapper directory.
 */
function commonLeadingDirectory(paths: string[]): string | null {
  if (paths.length < 2) return null;

  const firstSlash = paths[0].indexOf('/');
  if (firstSlash === -1) return null;

  const prefix = paths[0].slice(0, firstSlash + 1); // includes trailing '/'
  return paths.every(p => p.startsWith(prefix)) ? prefix : null;
}

/**
 * Classify an entry as text or binary and produce its content payload.
 *
 * Extension is authoritative when known. For unknown extensions we sniff:
 * content that contains a NUL byte or is not valid UTF-8 is treated as
 * binary, everything else as text.
 */
function classifyEntry(path: string, bytes: Uint8Array): ParsedFile {
  let binary: boolean;
  if (isBinaryExtension(path)) {
    binary = true;
  } else if (isTextExtension(path)) {
    binary = false;
  } else {
    binary = looksBinary(bytes);
  }

  if (binary) {
    return {
      path,
      content: toBase64(bytes),
      contentType: 'binary',
      mimeType: inferMimeType(path),
    };
  }

  return {
    path,
    content: strFromU8(bytes),
    contentType: 'text',
  };
}

/** Heuristic: NUL byte or invalid UTF-8 => binary. */
function looksBinary(bytes: Uint8Array): boolean {
  if (bytes.includes(0)) return true;
  try {
    STRICT_UTF8.decode(bytes);
    return false;
  } catch {
    return true;
  }
}

/**
 * Base64-encode bytes for transport as a string. Matches the encoding
 * `createNewProject` decodes with `atob`. Chunked to avoid building a
 * single multi-megabyte intermediate string and to stay clear of
 * argument-count limits on `String.fromCharCode`.
 */
function toBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let binary = '';
  for (let i = 0; i < bytes.length; i += CHUNK) {
    const slice = bytes.subarray(i, i + CHUNK);
    binary += String.fromCharCode(...slice);
  }
  return btoa(binary);
}
