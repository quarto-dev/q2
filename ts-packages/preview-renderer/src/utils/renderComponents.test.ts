import { describe, it, expect } from 'vitest';
import { extractRenderComponentPaths } from './renderComponents';

/**
 * Unit tests for the shared `render-components` meta walk (GH #402 /
 * bd-ue80chl0 Phase 1). The behavior is a verbatim extraction of
 * hub-client's inline walk in `ReactRenderer.tsx`, including its
 * mid-typing guards; these tests pin that contract for both parents
 * (hub-client and the q2-preview SPA).
 */

function astWithMeta(renderComponents: unknown): unknown {
  return {
    'pandoc-api-version': [1, 23, 0],
    meta:
      renderComponents === undefined
        ? {}
        : { 'render-components': renderComponents },
    blocks: [],
  };
}

function metaList(entries: unknown[]): unknown {
  return { t: 'MetaList', c: entries };
}

function metaInlinesStr(s: string): unknown {
  return { t: 'MetaInlines', c: [{ t: 'Str', c: s }] };
}

describe('extractRenderComponentPaths', () => {
  it('extracts every path from a well-formed MetaList', () => {
    const ast = astWithMeta(
      metaList([metaInlinesStr('overrides.tsx'), metaInlinesStr('/components/extra.tsx')]),
    );
    expect(extractRenderComponentPaths(ast)).toEqual([
      'overrides.tsx',
      '/components/extra.tsx',
    ]);
  });

  it('returns [] when the key is absent', () => {
    expect(extractRenderComponentPaths(astWithMeta(undefined))).toEqual([]);
  });

  it('drops a mid-typing null entry (bare `-` bullet parses to null)', () => {
    const ast = astWithMeta(metaList([null, metaInlinesStr('overrides.tsx')]));
    expect(extractRenderComponentPaths(ast)).toEqual(['overrides.tsx']);
  });

  it('drops an empty MetaInlines entry (delimiter typed, no content yet)', () => {
    const ast = astWithMeta(
      metaList([{ t: 'MetaInlines', c: [] }, metaInlinesStr('overrides.tsx')]),
    );
    expect(extractRenderComponentPaths(ast)).toEqual(['overrides.tsx']);
  });

  it('returns [] for a non-list meta value', () => {
    // `render-components: overrides.tsx` (scalar, not a list) parses to
    // MetaInlines directly; the walk must not misread inline nodes as
    // list entries.
    const ast = astWithMeta(metaInlinesStr('overrides.tsx'));
    expect(extractRenderComponentPaths(ast)).toEqual([]);
  });

  it('returns [] for null / non-object ASTs', () => {
    expect(extractRenderComponentPaths(null)).toEqual([]);
    expect(extractRenderComponentPaths(undefined)).toEqual([]);
    expect(extractRenderComponentPaths('not an ast')).toEqual([]);
    expect(extractRenderComponentPaths({ blocks: [] })).toEqual([]);
  });

  it('drops entries whose first inline is not a Str', () => {
    const ast = astWithMeta(
      metaList([{ t: 'MetaInlines', c: [{ t: 'Space' }] }, metaInlinesStr('a.tsx')]),
    );
    expect(extractRenderComponentPaths(ast)).toEqual(['a.tsx']);
  });
});
