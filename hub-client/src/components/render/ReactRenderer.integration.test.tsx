/**
 * Integration tests for `ReactRenderer`.
 *
 * Two concerns share this file because both depend on the same set of
 * module mocks (Q2DebugIframe + tsxTranspiler + slide renderers):
 *
 *  1. Render-components lookup (the chain that broke during the
 *     bugfix-react-components-not-loading investigation): AST meta
 *     extraction → resolveComponentPath → fileContents lookup →
 *     transpileTSX → customComponentsCode prop captured on Q2DebugIframe.
 *
 *  2. Format routing (Plan 1 §"Test plan" item 5): mounting with
 *     `format="q2-preview"` routes through `Q2DebugIframe` alongside
 *     `q2-debug`, while `q2-slides` does not. The dispatch lives at
 *     `ReactRenderer.tsx`'s `format === 'q2-debug' || format === 'q2-preview'`
 *     branch; these tests guard against a regression that would silently
 *     route q2-preview through `SlideAst` (which expects slide-shaped
 *     AST and would crash).
 *
 * `Q2DebugIframe` is mocked so we can capture the prop without spinning
 * up a real iframe; the routing tests check whether the mock was
 * invoked to decide which branch was taken.
 *
 * `transpileTSX` is mocked because it would otherwise pull the full
 * TypeScript transpiler graph at module-init time.
 *
 * The slide renderers are mocked so the q2-slides arm is exercised
 * without pulling in `AspectRatioScaler`'s `ResizeObserver` dependency,
 * and so the negative assertion ("not Q2DebugIframe") has a positive
 * sentinel to prove the path was actually taken.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render } from '@testing-library/react';

const capturedAstIframeProps: any[] = [];

vi.mock('./q2-debug/Q2DebugIframe', () => ({
  Q2DebugIframe: (props: any) => {
    capturedAstIframeProps.push(props);
    return null;
  },
}));

vi.mock('../../services/tsxTranspiler', () => ({
  transpileTSX: (code: string) => `JS:${code}`,
}));

vi.mock('./ReactAstSlideRenderer', () => ({
  SlideAst: () => <div data-testid="slide-sentinel" />,
}));

vi.mock('./RevealjsReactAstSlideRenderer', () => ({
  RevealjsSlideAst: () => <div data-testid="revealjs-sentinel" />,
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

const EMPTY_AST = JSON.stringify({
  'pandoc-api-version': [1, 23, 1],
  meta: {},
  blocks: [],
});

function mountForRouting(format: string) {
  return render(
    <ReactRenderer
      astJson={EMPTY_AST}
      currentFilePath="/project/index.qmd"
      files={[]}
      fileContents={new Map()}
      onNavigateToDocument={() => {}}
      setAst={() => {}}
      format={format}
    />,
  );
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

describe('ReactRenderer format routing', () => {
  beforeEach(() => {
    capturedAstIframeProps.length = 0;
  });

  it('routes q2-preview through Q2DebugIframe', () => {
    mountForRouting('q2-preview');
    expect(capturedAstIframeProps.length).toBeGreaterThan(0);
  });

  it('routes q2-debug through Q2DebugIframe (regression baseline)', () => {
    mountForRouting('q2-debug');
    expect(capturedAstIframeProps.length).toBeGreaterThan(0);
  });

  it('does not route q2-slides through Q2DebugIframe', () => {
    const { queryByTestId } = mountForRouting('q2-slides');
    expect(capturedAstIframeProps.length).toBe(0);
    // Positive sentinel: the slide path was actually taken.
    expect(queryByTestId('slide-sentinel')).not.toBeNull();
  });
});
