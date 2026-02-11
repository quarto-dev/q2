/**
 * Project ZIP export utility.
 *
 * Walks all files in a connected SyncClient project and produces
 * a ZIP archive as a Uint8Array.
 */

import { zipSync, strToU8 } from 'fflate';
import type { SyncClient } from './client.js';

/**
 * Export all project files as a ZIP archive.
 *
 * Reads every file from the connected SyncClient (text and binary)
 * and packs them into a ZIP. Text files are encoded as UTF-8.
 *
 * @param client - A connected SyncClient instance
 * @returns Uint8Array containing the ZIP file bytes
 * @throws If the client is not connected
 */
export function exportProjectAsZip(client: SyncClient): Uint8Array {
  if (!client.isConnected()) {
    throw new Error('SyncClient is not connected');
  }

  const paths = client.getFilePaths();
  const files: Record<string, Uint8Array> = {};

  for (const path of paths) {
    if (client.isFileBinary(path)) {
      const binary = client.getBinaryFileContent(path);
      if (binary) {
        files[path] = binary.content;
      }
    } else {
      const text = client.getFileContent(path);
      if (text !== null) {
        files[path] = strToU8(text);
      }
    }
  }

  return zipSync(files, { level: 6 });
}
