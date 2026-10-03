import { describe, expect, it } from 'vitest';
import { matrixDocs } from './recordingFixtures';

describe('matrixDocs', () => {
  it('yields every case with its doc.qmd and companion files, paths relative to the case', () => {
    const docs = matrixDocs();
    expect(docs.length).toBeGreaterThanOrEqual(10);
    for (const d of docs) expect(d.files.some((f) => f.path === 'doc.qmd'), d.name).toBe(true);
    const theme = docs.find((d) => d.name === 'highlight-theme-file')!;
    expect(theme.files.map((f) => f.path).sort()).toEqual(['doc.qmd', 'mine.theme']);
  });
});
