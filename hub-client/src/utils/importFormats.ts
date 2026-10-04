/**
 * Helpers over the import format table (`getImportFormats()`, Rust-owned: TS keeps no
 * copy of the extension or MIME lists, document import I1 / interface 4).
 */
import type { ImportFormats } from '../pandoc/importService';

/** The picker's `accept` value: every extension and every MIME type, comma-separated. */
export function importAccept(formats: ImportFormats): string {
  return formats.formats.flatMap((f) => [...f.extensions, ...f.mimeTypes]).join(',');
}

/** Whether `fileName` ends in one of the table's extensions (case-insensitive, like Rust's `format_for_file_name`). */
export function isImportableName(fileName: string, formats: ImportFormats): boolean {
  const lower = fileName.toLowerCase();
  return formats.formats.some((f) => f.extensions.some((ext) => lower.endsWith(ext.toLowerCase())));
}
