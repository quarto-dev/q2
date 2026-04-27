/**
 * Tests for processAssetFiles
 */

import { describe, it, expect } from 'vitest';
import { processAssetFiles } from './processAssetFiles';
import { FILE_SIZE_LIMITS } from '../../services/resourceService';

function makeFile(name: string, size: number, type = 'application/octet-stream'): File {
  // File constructor in Node test env may not fill .size based on bits;
  // we stub it explicitly for deterministic tests.
  const blob = new Blob([new Uint8Array(Math.min(size, 16))], { type });
  const file = new File([blob], name, { type });
  Object.defineProperty(file, 'size', { value: size });
  return file;
}

describe('processAssetFiles', () => {
  it('returns a preview for each input file', () => {
    const files = [makeFile('a.png', 10), makeFile('b.wasm', 100)];
    const result = processAssetFiles(files);
    expect(result).toHaveLength(2);
    expect(result[0].file.name).toBe('a.png');
    expect(result[1].file.name).toBe('b.wasm');
  });

  it('marks empty files as errors', () => {
    const files = [makeFile('empty.png', 0)];
    const result = processAssetFiles(files);
    expect(result[0].error).toMatch(/empty/i);
  });

  it('marks oversized files as errors', () => {
    const oversize = FILE_SIZE_LIMITS.MAX_FILE_SIZE + 1;
    const files = [makeFile('big.png', oversize)];
    const result = processAssetFiles(files);
    expect(result[0].error).toMatch(/size|max/i);
  });

  it('leaves valid files with no error', () => {
    const files = [makeFile('ok.png', 1024)];
    const result = processAssetFiles(files);
    expect(result[0].error).toBeUndefined();
  });

  it('handles a mix of valid and invalid files', () => {
    const files = [
      makeFile('ok.png', 1024),
      makeFile('empty.txt', 0),
      makeFile('big.bin', FILE_SIZE_LIMITS.MAX_FILE_SIZE + 1),
    ];
    const result = processAssetFiles(files);
    expect(result[0].error).toBeUndefined();
    expect(result[1].error).toMatch(/empty/i);
    expect(result[2].error).toMatch(/size|max/i);
  });
});
