import { describe, expect, it } from 'vitest';
import { VfsMirror, normalizeVfsPath } from './vfsMirror';

describe('normalizeVfsPath', () => {
  it('matches the Rust VFS: relative paths live under /project, dots resolve', () => {
    expect(normalizeVfsPath('index.qmd')).toBe('/project/index.qmd');
    expect(normalizeVfsPath('./a/../b.qmd')).toBe('/project/b.qmd');
    expect(normalizeVfsPath('/project/x/y.png')).toBe('/project/x/y.png');
    expect(normalizeVfsPath('/.quarto/project-artifacts/styles.css')).toBe(
      '/.quarto/project-artifacts/styles.css',
    );
    expect(normalizeVfsPath('/../../etc')).toBe('/etc');
  });
});

describe('VfsMirror', () => {
  it('answers the synchronous read API for text and binary entries', () => {
    const m = new VfsMirror();
    m.addText('index.qmd', '# Hi é');
    m.addBinary('img/a.png', new Uint8Array([0x89, 0x50, 0x4e, 0x47]));

    expect(m.readFile('/project/index.qmd')).toEqual({ success: true, content: '# Hi é' });
    // Text read back as binary is its UTF-8 bytes, base64-encoded.
    expect(m.readBinaryFile('index.qmd')).toEqual({
      success: true,
      content: btoa(String.fromCharCode(...new TextEncoder().encode('# Hi é'))),
    });
    expect(m.readBinaryFile('img/a.png')).toEqual({ success: true, content: 'iVBORw==' });
    // Invalid UTF-8 is an error for a text read, as in the Rust VFS.
    expect(m.readFile('img/a.png').success).toBe(false);
    expect(m.readFile('missing.qmd').success).toBe(false);
    expect(m.listFiles().files?.sort()).toEqual(['/project/img/a.png', '/project/index.qmd']);
  });

  it('removes, clears, and applies artifact updates from a render', () => {
    const m = new VfsMirror();
    m.addText('a.qmd', 'a');
    expect(m.remove('a.qmd').success).toBe(true);
    expect(m.remove('a.qmd').success).toBe(false);

    m.addText('b.qmd', 'b');
    m.applyArtifacts({
      changed: [
        { path: '/.quarto/project-artifacts/styles.css', text: 'body{}' },
        { path: '/.quarto/x.png', base64: 'iVBORw==' },
      ],
      removed: [],
    });
    expect(m.readFile('/.quarto/project-artifacts/styles.css').content).toBe('body{}');
    expect(m.readBinaryFile('/.quarto/x.png').content).toBe('iVBORw==');
    m.applyArtifacts({ changed: [], removed: ['/.quarto/x.png'] });
    expect(m.readBinaryFile('/.quarto/x.png').success).toBe(false);

    m.clear();
    expect(m.listFiles().files).toEqual([]);
  });

  it('reports an identical write as unchanged, for text and binary', () => {
    const m = new VfsMirror();
    expect(m.addText('a.qmd', 'x').changed).toBe(true);
    expect(m.addText('a.qmd', 'x').changed).toBe(false);
    expect(m.addText('a.qmd', 'y').changed).toBe(true);
    expect(m.addBinary('b.png', new Uint8Array([1, 2])).changed).toBe(true);
    expect(m.addBinary('b.png', new Uint8Array([1, 2])).changed).toBe(false);
    // A pulled file mirrored as bytes, echoed back by sync as text.
    m.addBinary('refs.bib', new TextEncoder().encode('@a'));
    expect(m.addText('refs.bib', '@a').changed).toBe(false);
  });
});
