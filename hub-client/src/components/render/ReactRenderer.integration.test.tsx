/**
 * Integration tests for `ReactRenderer`.
 *
 * Two concerns share this file because both depend on the same set of
 * module mocks (Q2DebugIframe / Q2PreviewIframe + tsxTranspiler + slide
 * renderers):
 *
 *  1. Render-components lookup (the chain that broke during the
 *     bugfix-react-components-not-loading investigation): AST meta
 *     extraction → resolveComponentPath → fileContents lookup →
 *     transpileTSX → customComponentsCode prop captured on the iframe
 *     wrapper. Plan 2A item 13 extended this gate to cover q2-preview
 *     alongside q2-debug.
 *
 *  2. Format routing (Plan 2A item 12): the format dispatch is split.
 *     `q2-debug` routes through `Q2DebugIframe`; `q2-preview` routes
 *     through `Q2PreviewIframe`; `q2-slides` routes through neither.
 *     These tests guard against a regression that would silently route
 *     q2-preview through `SlideAst` (which expects slide-shaped AST
 *     and would crash) or through `Q2DebugIframe` (which lacks the
 *     theme-CSS effect).
 *
 * Both iframe wrappers are mocked so we can capture the prop without
 * spinning up real iframes; the routing tests check which mock was
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
const capturedPreviewIframeProps: any[] = [];

vi.mock('./q2-debug/Q2DebugIframe', () => ({
  Q2DebugIframe: (props: any) => {
    capturedAstIframeProps.push(props);
    return null;
  },
}));

// Q2PreviewIframe moved to @quarto/preview-renderer/iframe/ in
// bd-hfjj Phase 4. Mock the new location.
vi.mock('@quarto/preview-renderer/iframe/Q2PreviewIframe', () => ({
  Q2PreviewIframe: (props: any) => {
    capturedPreviewIframeProps.push(props);
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

function lastCapturedPreviewCode(): Record<string, string> | undefined {
  return capturedPreviewIframeProps.at(-1)?.customComponentsCode;
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
    capturedPreviewIframeProps.length = 0;
  });

  it('routes q2-preview through Q2PreviewIframe (Plan 2A item 12)', () => {
    mountForRouting('q2-preview');
    expect(capturedPreviewIframeProps.length).toBeGreaterThan(0);
    // q2-preview must NOT also hit the q2-debug iframe.
    expect(capturedAstIframeProps.length).toBe(0);
  });

  it('routes q2-debug through Q2DebugIframe (regression baseline)', () => {
    mountForRouting('q2-debug');
    expect(capturedAstIframeProps.length).toBeGreaterThan(0);
    expect(capturedPreviewIframeProps.length).toBe(0);
  });

  it('does not route q2-slides through any AST iframe', () => {
    const { queryByTestId } = mountForRouting('q2-slides');
    expect(capturedAstIframeProps.length).toBe(0);
    expect(capturedPreviewIframeProps.length).toBe(0);
    // Positive sentinel: the slide path was actually taken.
    expect(queryByTestId('slide-sentinel')).not.toBeNull();
  });
});

describe('ReactRenderer render-components gate (Plan 2A item 13)', () => {
  beforeEach(() => {
    capturedAstIframeProps.length = 0;
    capturedPreviewIframeProps.length = 0;
  });

  it('extracts customComponentsCode for q2-preview format', () => {
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
        format="q2-preview"
      />,
    );

    // Same transpilation behavior as q2-debug — gate covers both.
    expect(lastCapturedPreviewCode()).toEqual({
      '/elliot/simple.tsx': 'JS:export const Para = () => null;',
    });
  });

  it('q2-debug behavior unchanged (regression baseline)', () => {
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

  it('skips empty `render-components` list entries without crashing', () => {
    // Reproduces the bug observed while typing in the YAML frontmatter:
    //
    //     render-components:
    //       -
    //
    // The parser produces a MetaList with a single empty / null entry
    // (no MetaInlines, no Str). Before the fix, the path extraction
    // emitted `[undefined]`, which then crashed `resolveComponentPath`
    // mid-type with `Cannot read properties of undefined (reading
    // 'startsWith')`. No upstream ErrorBoundary, so the iframe-host
    // page went blank and the user couldn't finish typing.
    //
    // The renderer should tolerate empty entries by skipping them,
    // leaving the customComponentsCode map empty.
    const malformedAst = JSON.stringify({
      'pandoc-api-version': [1, 23, 0],
      meta: {
        'render-components': {
          t: 'MetaList',
          // single empty bullet: the parser commonly produces `null`,
          // sometimes `{t: 'MetaString', c: ''}`, occasionally an
          // empty MetaInlines. All three shapes must not crash.
          c: [null],
        },
      },
      blocks: [],
    });

    expect(() => {
      render(
        <ReactRenderer
          astJson={malformedAst}
          currentFilePath="elliot/index.qmd"
          files={[]}
          fileContents={new Map()}
          onNavigateToDocument={() => {}}
          setAst={() => {}}
          format="q2-preview"
        />,
      );
    }).not.toThrow();
    expect(lastCapturedPreviewCode()).toEqual({});
  });

  it('skips `render-components` entries that resolve to empty strings', () => {
    // Same defensive coverage for the case where the YAML entry parsed
    // as a MetaInlines with no children (or a Str of empty text). The
    // path extraction yields '' which would also crash downstream
    // (well, technically empty string would skip resolveComponentPath
    // but still attempt a `fileContents.get('')` lookup and a noisy
    // console.warn). Treating empty paths as absent keeps the warn
    // surface clean while the user is mid-typing.
    const partiallyTypedAst = JSON.stringify({
      'pandoc-api-version': [1, 23, 0],
      meta: {
        'render-components': {
          t: 'MetaList',
          c: [{ t: 'MetaInlines', c: [] }],
        },
      },
      blocks: [],
    });

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    expect(() => {
      render(
        <ReactRenderer
          astJson={partiallyTypedAst}
          currentFilePath="elliot/index.qmd"
          files={[]}
          fileContents={new Map()}
          onNavigateToDocument={() => {}}
          setAst={() => {}}
          format="q2-preview"
        />,
      );
    }).not.toThrow();
    expect(lastCapturedPreviewCode()).toEqual({});
    // Empty-string paths should be skipped without warning — they
    // arise from mid-typing the YAML and aren't a user error worth
    // surfacing.
    expect(warnSpy).not.toHaveBeenCalled();
    warnSpy.mockRestore();
  });
});
