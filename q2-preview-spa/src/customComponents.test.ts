/**
 * Unit tests for the SPA's render-components parent half (GH #402 /
 * bd-ue80chl0 Phase 2): `extractComponentPathsKey` (the cheap,
 * stable-string effect key) and `buildCustomComponentsCode` (path
 * resolution → content lookup → lazy transpile → warnings).
 *
 * The shared transpiler module is mocked so these tests don't pay for
 * `@babel/standalone`; a hoisted flag additionally proves the lazy
 * import is NOT taken for documents without `render-components` —
 * that's the "common path stays free" guarantee.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  extractComponentPathsKey,
  buildCustomComponentsCode,
  EMPTY_CUSTOM_COMPONENTS,
} from './customComponents';

const hoisted = vi.hoisted(() => ({ transpilerImported: false }));

vi.mock('@quarto/preview-renderer/utils/tsxTranspiler', () => {
  hoisted.transpilerImported = true;
  return {
    transpileTSX: (code: string) => {
      if (code.includes('SYNTAX ERROR')) {
        throw new Error('Failed to transpile TSX: unexpected token');
      }
      return `JS:${code}`;
    },
  };
});

function astJsonWith(paths: string[] | null): string {
  const meta =
    paths === null
      ? {}
      : {
          'render-components': {
            t: 'MetaList',
            c: paths.map((p) => ({
              t: 'MetaInlines',
              c: [{ t: 'Str', c: p }],
            })),
          },
        };
  return JSON.stringify({ 'pandoc-api-version': [1, 23, 0], meta, blocks: [] });
}

beforeEach(() => {
  hoisted.transpilerImported = false;
  vi.resetModules();
});

describe('extractComponentPathsKey', () => {
  it('returns a stable JSON key for the path list', () => {
    expect(extractComponentPathsKey(astJsonWith(['overrides.tsx', '/c/x.tsx']))).toBe(
      JSON.stringify(['overrides.tsx', '/c/x.tsx']),
    );
  });

  it('returns "" when the key is absent, astJson is null, or JSON is invalid', () => {
    expect(extractComponentPathsKey(astJsonWith(null))).toBe('');
    expect(extractComponentPathsKey(null)).toBe('');
    expect(extractComponentPathsKey('not json')).toBe('');
  });
});

describe('buildCustomComponentsCode', () => {
  it('returns the stable empty result — without importing the transpiler — for an empty key', async () => {
    const getContent = vi.fn();
    const result = await buildCustomComponentsCode('', 'index.qmd', getContent);
    expect(result).toBe(EMPTY_CUSTOM_COMPONENTS);
    expect(getContent).not.toHaveBeenCalled();
    expect(hoisted.transpilerImported).toBe(false);
  });

  it('transpiles each resolved component, keyed by the original path', async () => {
    const contents = new Map<string, string>([
      ['docs/overrides.tsx', 'export const Para = 1;'],
      ['components/x.tsx', 'export const Callout = 2;'],
    ]);
    const key = JSON.stringify(['overrides.tsx', '/components/x.tsx']);
    const result = await buildCustomComponentsCode(
      key,
      'docs/index.qmd',
      (p) => contents.get(p) ?? null,
    );
    // Relative entry resolves against the document's directory; leading
    // `/` resolves against the project root. Keys stay the ORIGINAL
    // meta strings (hub-client parity — the iframe logs them verbatim).
    expect(result.code).toEqual({
      'overrides.tsx': 'JS:export const Para = 1;',
      '/components/x.tsx': 'JS:export const Callout = 2;',
    });
    expect(result.warnings).toEqual([]);
  });

  it('warns and skips a component whose file is missing', async () => {
    const key = JSON.stringify(['missing.tsx', 'present.tsx']);
    const result = await buildCustomComponentsCode(
      key,
      'index.qmd',
      (p) => (p === 'present.tsx' ? 'export const A = 1;' : null),
    );
    expect(result.code).toEqual({ 'present.tsx': 'JS:export const A = 1;' });
    expect(result.warnings).toHaveLength(1);
    expect(result.warnings[0].kind).toBe('warning');
    expect(result.warnings[0].title).toContain('missing.tsx');
    expect(result.warnings[0].title.toLowerCase()).toContain('not found');
  });

  it('warns and skips a component that fails to transpile', async () => {
    const key = JSON.stringify(['bad.tsx']);
    const result = await buildCustomComponentsCode(
      key,
      'index.qmd',
      () => 'SYNTAX ERROR',
    );
    expect(result.code).toEqual({});
    expect(result.warnings).toHaveLength(1);
    expect(result.warnings[0].title).toContain('bad.tsx');
    expect(result.warnings[0].problem).toContain('Failed to transpile');
  });
});
