/**
 * Project ZIP export utility.
 *
 * Walks all files in a connected SyncClient project and produces
 * a ZIP archive as a Uint8Array.
 */

import { zipSync, strToU8 } from 'fflate';
import { projectFolderName } from './project-folder-name.js';
import type { SyncClient } from './client.js';

/**
 * Export all project files as a ZIP archive.
 *
 * Reads every file from the connected SyncClient (text and binary)
 * and packs them into a ZIP. Text files are encoded as UTF-8.
 *
 * Project paths are stored absolute (leading slash). ZIP entries must be
 * *relative* — an absolute entry makes `unzip` emit a "stripped absolute
 * path spec" warning and drop the leading slash (GH #147). This function
 * therefore always strips leading slashes, and when a `rootDir` is given it
 * nests every entry under that single top-level folder (matching the download
 * filename), so the archive extracts into one tidy directory. The importer
 * (`parseProjectZip`) strips that common folder back off on the way in.
 *
 * @param client - A connected SyncClient instance
 * @param rootDir - Optional top-level folder name (typically the project's
 *   description). Sanitized to a safe single path segment. When omitted or
 *   blank, entries are packed at the archive root (still relative).
 * @returns Uint8Array containing the ZIP file bytes
 * @throws If the client is not connected
 */
export function exportProjectAsZip(
  client: SyncClient,
  rootDir?: string,
): Uint8Array {
  if (!client.isConnected()) {
    throw new Error('SyncClient is not connected');
  }

  // Sanitize the wrapper folder to a safe single segment. `projectFolderName`
  // trims stray leading/trailing separators, so we never emit `//` or an
  // absolute prefix. Blank/undefined => no wrapper folder.
  const prefix = rootDir && rootDir.trim() ? `${projectFolderName(rootDir)}/` : '';

  const paths = client.getFilePaths();
  const files: Record<string, Uint8Array> = {};

  for (const path of paths) {
    // Strip leading slashes so the entry is relative, then nest under prefix.
    const relative = path.replace(/^\/+/, '');
    if (relative === '') continue; // guard against a bare "/" path
    const key = `${prefix}${relative}`;

    if (client.isFileBinary(path)) {
      const binary = client.getBinaryFileContent(path);
      if (binary) {
        files[key] = binary.content;
      }
    } else {
      const text = client.getFileContent(path);
      if (text !== null) {
        files[key] = strToU8(text);
      }
    }
  }

  return zipSync(files, { level: 6 });
}
