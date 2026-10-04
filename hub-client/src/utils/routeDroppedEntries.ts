/**
 * Split a drop into files to import and files to store as-is (document import I5).
 *
 * Only files dropped at the top level are imported: their `relativePath` has no `/`. A file inside
 * a dropped folder is stored as-is, so a folder upload keeps its structure. Matching is by
 * extension, case-insensitively, against the Rust format table; with no table (import
 * unavailable, or not loaded yet) everything is an upload, which is today's behaviour.
 *
 * Importable files are split off before the upload path sees them: `processAssetFiles` would
 * reject a 10 to 25 MB source as too large to store, and an import has its own, higher cap.
 */
import type { ImportFormats } from '../pandoc/importService';
import type { DroppedEntries } from './droppedEntries';
import { isImportableName } from './importFormats';

export interface RouteOptions {
  formats: ImportFormats | null;
  /** The folder the imports will be proposed in. */
  destination: string;
}

export interface RoutedDrop {
  imports: { file: File; folder: string }[];
  /** What is left for the upload path. Folders are kept as dropped, so a drop of only importable files creates none. */
  uploads: DroppedEntries;
}

export function routeDroppedEntries(entries: DroppedEntries, { formats, destination }: RouteOptions): RoutedDrop {
  if (!formats) return { imports: [], uploads: entries };
  const imports: RoutedDrop['imports'] = [];
  const files: DroppedEntries['files'] = [];
  for (const dropped of entries.files) {
    const topLevel = !dropped.relativePath.includes('/');
    if (topLevel && isImportableName(dropped.file.name, formats)) imports.push({ file: dropped.file, folder: destination });
    else files.push(dropped);
  }
  return { imports, uploads: { files, folders: entries.folders } };
}
