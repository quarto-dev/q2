/**
 * Tests for URL routing utilities.
 */
import { describe, it, expect } from 'vitest';
import {
  parseHashRoute,
  buildHashRoute,
  routesEqual,
  sameFile,
  type Route,
} from './routing';

describe('parseHashRoute', () => {
  describe('project selector routes', () => {
    it('parses empty string as project selector', () => {
      expect(parseHashRoute('')).toEqual({ type: 'project-selector' });
    });

    it('parses bare # as project selector', () => {
      expect(parseHashRoute('#')).toEqual({ type: 'project-selector' });
    });

    it('parses #/ as project selector', () => {
      expect(parseHashRoute('#/')).toEqual({ type: 'project-selector' });
    });

    it('parses unknown routes as project selector', () => {
      expect(parseHashRoute('#/unknown/path')).toEqual({ type: 'project-selector' });
      expect(parseHashRoute('#/foo')).toEqual({ type: 'project-selector' });
    });
  });

  describe('project routes', () => {
    it('parses project route with UUID', () => {
      expect(parseHashRoute('#/project/abc-123-def')).toEqual({
        type: 'project',
        projectId: 'abc-123-def',
      });
    });

    it('parses project route with full UUID', () => {
      const uuid = '550e8400-e29b-41d4-a716-446655440000';
      expect(parseHashRoute(`#/project/${uuid}`)).toEqual({
        type: 'project',
        projectId: uuid,
      });
    });

    it('handles project route without leading #', () => {
      expect(parseHashRoute('/project/abc-123')).toEqual({
        type: 'project',
        projectId: 'abc-123',
      });
    });
  });

  describe('file routes', () => {
    it('parses simple file path', () => {
      expect(parseHashRoute('#/project/abc-123/file/index.qmd')).toEqual({
        type: 'file',
        projectId: 'abc-123',
        filePath: 'index.qmd',
      });
    });

    it('parses encoded nested file path', () => {
      // docs/intro.qmd encoded as docs%2Fintro.qmd
      expect(parseHashRoute('#/project/abc-123/file/docs%2Fintro.qmd')).toEqual({
        type: 'file',
        projectId: 'abc-123',
        filePath: 'docs/intro.qmd',
      });
    });

    it('parses file path with anchor', () => {
      expect(parseHashRoute('#/project/abc-123/file/index.qmd#section-1')).toEqual({
        type: 'file',
        projectId: 'abc-123',
        filePath: 'index.qmd',
        anchor: 'section-1',
      });
    });

    it('parses encoded path with anchor', () => {
      expect(parseHashRoute('#/project/abc-123/file/docs%2Fchapter1.qmd#intro')).toEqual({
        type: 'file',
        projectId: 'abc-123',
        filePath: 'docs/chapter1.qmd',
        anchor: 'intro',
      });
    });

    it('handles file path with special characters', () => {
      // Path with spaces: "my file.qmd" -> "my%20file.qmd"
      expect(parseHashRoute('#/project/abc-123/file/my%20file.qmd')).toEqual({
        type: 'file',
        projectId: 'abc-123',
        filePath: 'my file.qmd',
      });
    });

    it('handles deeply nested paths', () => {
      expect(parseHashRoute('#/project/abc/file/a%2Fb%2Fc%2Fd.qmd')).toEqual({
        type: 'file',
        projectId: 'abc',
        filePath: 'a/b/c/d.qmd',
      });
    });

    it('returns project route when file segment is empty', () => {
      expect(parseHashRoute('#/project/abc-123/file/')).toEqual({
        type: 'project',
        projectId: 'abc-123',
      });
    });
  });
});

describe('buildHashRoute', () => {
  describe('project selector routes', () => {
    it('builds project selector route', () => {
      expect(buildHashRoute({ type: 'project-selector' })).toBe('#/');
    });
  });

  describe('project routes', () => {
    it('builds project route', () => {
      expect(buildHashRoute({ type: 'project', projectId: 'abc-123' })).toBe(
        '#/project/abc-123'
      );
    });
  });

  describe('file routes', () => {
    it('builds simple file route', () => {
      expect(
        buildHashRoute({
          type: 'file',
          projectId: 'abc-123',
          filePath: 'index.qmd',
        })
      ).toBe('#/project/abc-123/file/index.qmd');
    });

    it('encodes nested file paths', () => {
      expect(
        buildHashRoute({
          type: 'file',
          projectId: 'abc-123',
          filePath: 'docs/intro.qmd',
        })
      ).toBe('#/project/abc-123/file/docs%2Fintro.qmd');
    });

    it('builds file route with anchor', () => {
      expect(
        buildHashRoute({
          type: 'file',
          projectId: 'abc-123',
          filePath: 'index.qmd',
          anchor: 'section-1',
        })
      ).toBe('#/project/abc-123/file/index.qmd#section-1');
    });

    it('encodes special characters in path', () => {
      expect(
        buildHashRoute({
          type: 'file',
          projectId: 'abc',
          filePath: 'my file.qmd',
        })
      ).toBe('#/project/abc/file/my%20file.qmd');
    });
  });
});

describe('round-trip parsing', () => {
  const testCases: Route[] = [
    { type: 'project-selector' },
    { type: 'project', projectId: 'abc-123' },
    { type: 'project', projectId: '550e8400-e29b-41d4-a716-446655440000' },
    { type: 'file', projectId: 'abc', filePath: 'index.qmd' },
    { type: 'file', projectId: 'abc', filePath: 'docs/chapter1.qmd' },
    { type: 'file', projectId: 'abc', filePath: 'a/b/c/d.qmd' },
    { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'intro' },
    { type: 'file', projectId: 'abc', filePath: 'docs/api.qmd', anchor: 'methods' },
    { type: 'file', projectId: 'abc', filePath: 'my file.qmd' },
  ];

  for (const route of testCases) {
    it(`round-trips ${JSON.stringify(route)}`, () => {
      const hash = buildHashRoute(route);
      const parsed = parseHashRoute(hash);
      expect(parsed).toEqual(route);
    });
  }
});

describe('routesEqual', () => {
  it('returns true for equal project selector routes', () => {
    expect(
      routesEqual({ type: 'project-selector' }, { type: 'project-selector' })
    ).toBe(true);
  });

  it('returns true for equal project routes', () => {
    expect(
      routesEqual(
        { type: 'project', projectId: 'abc' },
        { type: 'project', projectId: 'abc' }
      )
    ).toBe(true);
  });

  it('returns false for different project IDs', () => {
    expect(
      routesEqual(
        { type: 'project', projectId: 'abc' },
        { type: 'project', projectId: 'def' }
      )
    ).toBe(false);
  });

  it('returns true for equal file routes', () => {
    expect(
      routesEqual(
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'intro' },
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'intro' }
      )
    ).toBe(true);
  });

  it('returns false for different anchors', () => {
    expect(
      routesEqual(
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'intro' },
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'outro' }
      )
    ).toBe(false);
  });

  it('returns false for different types', () => {
    expect(
      routesEqual({ type: 'project-selector' }, { type: 'project', projectId: 'abc' })
    ).toBe(false);
  });
});

describe('sameFile', () => {
  it('returns true for same file with different anchors', () => {
    expect(
      sameFile(
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'intro' },
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'outro' }
      )
    ).toBe(true);
  });

  it('returns true for same file, one without anchor', () => {
    expect(
      sameFile(
        { type: 'file', projectId: 'abc', filePath: 'index.qmd' },
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'section' }
      )
    ).toBe(true);
  });

  it('returns false for different files', () => {
    expect(
      sameFile(
        { type: 'file', projectId: 'abc', filePath: 'index.qmd' },
        { type: 'file', projectId: 'abc', filePath: 'about.qmd' }
      )
    ).toBe(false);
  });

  it('returns false for non-file routes', () => {
    expect(
      sameFile({ type: 'project-selector' }, { type: 'project-selector' })
    ).toBe(false);
    expect(
      sameFile(
        { type: 'project', projectId: 'abc' },
        { type: 'file', projectId: 'abc', filePath: 'index.qmd' }
      )
    ).toBe(false);
  });
});
