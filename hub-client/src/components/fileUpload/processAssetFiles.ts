/**
 * Pre-validate a batch of dropped/selected files for asset upload.
 *
 * Pure, synchronous. Returns one entry per input file, annotated with an
 * error string if the file can't be uploaded. Callers display the result
 * directly and let the user remove bad entries before confirming.
 *
 * Checks:
 * - Size cap (`FILE_SIZE_LIMITS.MAX_FILE_SIZE`)
 * - Empty files (zero bytes) are rejected outright.
 */

import { validateFileSize } from '../../services/resourceService';

export interface AssetFilePreview {
  file: File;
  /** Error message if the file is not uploadable; undefined means OK. */
  error?: string;
}

export function processAssetFiles(files: File[]): AssetFilePreview[] {
  const out: AssetFilePreview[] = [];
  for (const file of files) {
    const preview: AssetFilePreview = { file };

    if (file.size === 0) {
      preview.error = 'File is empty';
      out.push(preview);
      continue;
    }

    const sizeValidation = validateFileSize(file.size);
    if (!sizeValidation.valid) {
      preview.error = sizeValidation.error;
      out.push(preview);
      continue;
    }

    out.push(preview);
  }
  return out;
}
