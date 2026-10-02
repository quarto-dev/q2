import { describe, expect, it } from 'vitest';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { EMBED_EXCLUDED_DIRS, pruneEmbedDist } from '../../../scripts/prune-embed-dist.mjs';

describe('pruneEmbedDist', () => {
  it('removes the excluded asset directories and keeps everything else', () => {
    const dist = mkdtempSync(path.join(tmpdir(), 'embed-dist-'));
    try {
      for (const d of EMBED_EXCLUDED_DIRS) {
        mkdirSync(path.join(dist, d), { recursive: true });
        writeFileSync(path.join(dist, d, 'asset.bin'), 'x');
      }
      mkdirSync(path.join(dist, 'assets'));
      writeFileSync(path.join(dist, 'assets', 'main.js'), 'x');
      writeFileSync(path.join(dist, 'index.html'), 'x');

      pruneEmbedDist(dist);

      for (const d of EMBED_EXCLUDED_DIRS) expect(existsSync(path.join(dist, d)), d).toBe(false);
      expect(existsSync(path.join(dist, 'assets', 'main.js'))).toBe(true);
      expect(existsSync(path.join(dist, 'index.html'))).toBe(true);
    } finally {
      rmSync(dist, { recursive: true, force: true });
    }
  });

  it('is a no-op when the directories are absent', () => {
    const dist = mkdtempSync(path.join(tmpdir(), 'embed-dist-'));
    try {
      expect(() => pruneEmbedDist(dist)).not.toThrow();
    } finally {
      rmSync(dist, { recursive: true, force: true });
    }
  });
});
