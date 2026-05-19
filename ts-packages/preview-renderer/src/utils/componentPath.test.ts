import { describe, it, expect } from 'vitest';
import { resolveComponentPath } from './componentPath';

describe('resolveComponentPath', () => {
  it('strips leading slash on absolute project-root paths', () => {
    expect(
      resolveComponentPath('/elliot/simple.tsx', 'elliot/index.qmd')
    ).toBe('elliot/simple.tsx');
  });

  it('resolves a bare filename against the document directory', () => {
    expect(
      resolveComponentPath('html.tsx', 'gordon/tldraw-shortcode/example.qmd')
    ).toBe('gordon/tldraw-shortcode/html.tsx');
  });

  it('resolves explicit ./ against the document directory', () => {
    expect(
      resolveComponentPath('./html.tsx', 'gordon/tldraw-shortcode/example.qmd')
    ).toBe('gordon/tldraw-shortcode/html.tsx');
  });

  it('resolves ../ against the parent of the document directory', () => {
    expect(
      resolveComponentPath('../shared/foo.tsx', 'elliot/sub/index.qmd')
    ).toBe('elliot/shared/foo.tsx');
  });

  it('handles a document at the project root', () => {
    expect(resolveComponentPath('foo.tsx', 'index.qmd')).toBe('foo.tsx');
  });

  it('does not let ../ traversal escape the project root', () => {
    expect(resolveComponentPath('../../foo.tsx', 'a/b.qmd')).toBe('foo.tsx');
  });
});
