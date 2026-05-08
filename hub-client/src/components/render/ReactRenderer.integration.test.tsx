/**
 * Integration test for ReactRenderer's q2-debug render-components lookup.
 *
 * Exercises the full chain that broke during the bugfix-react-components-not-loading
 * investigation: AST meta extraction -> resolveComponentPath -> fileContents lookup
 * -> transpileTSX -> customComponentsCode prop passed to AstIframe.
 *
 * AstIframe is mocked so we can capture the prop without spinning up a real
 * iframe. transpileTSX is mocked so the assertion doesn't depend on
 * babel-standalone behavior; the integration we care about here is the wiring,
 * not the transpile output.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render } from '@testing-library/react';

const capturedAstIframeProps: any[] = [];

vi.mock('./AstIframe', () => ({
  AstIframe: (props: any) => {
    capturedAstIframeProps.push(props);
    return null;
  },
}));

vi.mock('../../services/tsxTranspiler', () => ({
  transpileTSX: (code: string) => `JS:${code}`,
}));

// Imported after vi.mock so the mocks are in place.
import ReactRenderer from './ReactRenderer';

function astWithRenderComponents(paths: string[]): string {
  return JSON.stringify({
    'pandoc-api-version': [1, 23, 0],
    meta: {
      'render-components': {
        t: 'MetaList',
        c: paths.map((p) => ({
          t: 'MetaInlines',
          c: [{ t: 'Str', c: p }],
        })),
      },
    },
    blocks: [],
  });
}

function lastCapturedCode(): Record<string, string> | undefined {
  return capturedAstIframeProps.at(-1)?.customComponentsCode;
}

describe('ReactRenderer (q2-debug render-components lookup)', () => {
  beforeEach(() => {
    capturedAstIframeProps.length = 0;
  });

  it('passes a transpiled component for an absolute project-root path', () => {
    const fileContents = new Map([
      ['elliot/simple.tsx', 'export const Para = () => null;'],
    ]);

    render(
      <ReactRenderer
        astJson={astWithRenderComponents(['/elliot/simple.tsx'])}
        currentFilePath="elliot/index.qmd"
        files={[]}
        fileContents={fileContents}
        onNavigateToDocument={() => {}}
        setAst={() => {}}
        format="q2-debug"
      />,
    );

    expect(lastCapturedCode()).toEqual({
      '/elliot/simple.tsx': 'JS:export const Para = () => null;',
    });
  });

  it('resolves a bare filename against the current document directory', () => {
    // Regression for Bug 4: render-components: html.tsx in a nested document
    // should map to gordon/tldraw-shortcode/html.tsx.
    const fileContents = new Map([
      ['gordon/tldraw-shortcode/html.tsx', 'export const Header = () => null;'],
    ]);

    render(
      <ReactRenderer
        astJson={astWithRenderComponents(['html.tsx'])}
        currentFilePath="gordon/tldraw-shortcode/example.qmd"
        files={[]}
        fileContents={fileContents}
        onNavigateToDocument={() => {}}
        setAst={() => {}}
        format="q2-debug"
      />,
    );

    expect(lastCapturedCode()).toEqual({
      'html.tsx': 'JS:export const Header = () => null;',
    });
  });

  it('omits unresolvable paths and warns', () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    render(
      <ReactRenderer
        astJson={astWithRenderComponents(['/elliot/missing.tsx'])}
        currentFilePath="elliot/index.qmd"
        files={[]}
        fileContents={new Map()}
        onNavigateToDocument={() => {}}
        setAst={() => {}}
        format="q2-debug"
      />,
    );

    expect(lastCapturedCode()).toEqual({});
    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('Component file not found: /elliot/missing.tsx'),
    );
    warnSpy.mockRestore();
  });

});
