/**
 * Behavior-preservation contract for q2-debug's rendering surface.
 *
 * Plan 2pre's promise is "byte-equivalent for q2-debug across the
 * carve-up, with one deliberate exception (the literal `// TODO:`
 * text in the bordered Figure)." Before 2pre, this contract had no
 * test coverage — only manual eyes. These tests lock the contract so
 * the carve-up's Phase 2 (consumer migration + rename + shim deletion)
 * has a real regression gate, and so future format additions can rely
 * on q2-debug being a stable reference output.
 *
 * Coverage per Plan 2pre §"What stays exactly the same":
 *   - Bordered debug aesthetic — every q2-debug leaf renders inside a
 *     `border` styling.
 *   - Figure caption branch — when a Figure has a short caption, the
 *     bordered "Caption:" line is emitted (port-for-port from the
 *     pre-carve-up renderChildrenRegistry entry, minus Bug A).
 *   - Bug A: the literal `// TODO:` text from
 *     `ReactAstDebugRenderer.tsx`'s old Figure entry is gone from the
 *     rendered DOM.
 *   - 'Not registered' miss path renders a bordered message for an
 *     unknown node type — the dispatcher fallback removal didn't
 *     break this.
 *   - Override path resolves: registering a custom Para under the
 *     q2-debug registry wins over q2-debug's bordered Para. (Locks
 *     the framework→<Block>→registry[node.t] dispatch chain.)
 */

import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { Ast } from '../framework';
import type { PandocAST } from '../framework';
import { q2DebugRegistry } from './registry';

function astJson(blocks: any[], meta: Record<string, unknown> = {}): string {
  const ast: PandocAST = {
    'pandoc-api-version': [1, 23, 0],
    meta,
    blocks: blocks as any,
  };
  return JSON.stringify(ast);
}

const noopNav = () => {};
const noopSet = () => {};

function mount(blocks: any[]) {
  return render(
    <Ast
      astJson={astJson(blocks)}
      currentFilePath="/project/test.qmd"
      onNavigateToDocument={noopNav}
      setAst={noopSet}
      registry={q2DebugRegistry}
    />,
  );
}

// ---------------------------------------------------------------------------
// Block / inline fixtures
// ---------------------------------------------------------------------------

const STR = (c: string) => ({ t: 'Str', c });
const PARA = (text: string) => ({ t: 'Para', c: [STR(text)] });
const HEADER = (level: number, text: string) => ({
  t: 'Header',
  c: [level, ['', [], []], [STR(text)]],
});
const BULLET_LIST = (items: any[][]) => ({ t: 'BulletList', c: items });
const FIGURE_WITH_SHORT_CAPTION = (shortCaption: string, body: any[]) => ({
  t: 'Figure',
  c: [
    ['', [], []],
    [[STR(shortCaption)], []],
    body,
  ],
});

// ---------------------------------------------------------------------------
// Bordered debug aesthetic
// ---------------------------------------------------------------------------

describe('q2-debug bordered aesthetic', () => {
  it('renders Para inside a bordered block', () => {
    const { container } = mount([PARA('hello')]);
    const bordered = container.querySelector('div[style*="border"]');
    expect(bordered).not.toBeNull();
    expect(container.textContent).toContain('Para:');
    expect(container.textContent).toContain('hello');
  });

  it('renders Header inside a bordered block with the level annotation', () => {
    const { container } = mount([HEADER(2, 'Section')]);
    expect(container.textContent).toContain('Header(level=2):');
    expect(container.textContent).toContain('Section');
    const bordered = container.querySelector('div[style*="border"]');
    expect(bordered).not.toBeNull();
  });

  it('renders BulletList items recursively', () => {
    const { container } = mount([
      BULLET_LIST([[PARA('item one')], [PARA('item two')]]),
    ]);
    expect(container.textContent).toContain('BulletList:');
    expect(container.textContent).toContain('item one');
    expect(container.textContent).toContain('item two');
  });
});

// ---------------------------------------------------------------------------
// Figure: Bug A + caption-branch port
// ---------------------------------------------------------------------------

describe('q2-debug Figure (Bug A + caption-branch port)', () => {
  it('renders the bordered "Caption:" line for a short-captioned figure', () => {
    const { container } = mount([
      FIGURE_WITH_SHORT_CAPTION('alt-text', [PARA('figure body')]),
    ]);

    // Figure leaf header
    expect(container.textContent).toContain('Figure:');
    // Body block recursed through framework's renderChildrenRegistry.Figure
    expect(container.textContent).toContain('figure body');
    // Caption branch ported port-for-port from the pre-carve-up registry entry
    expect(container.textContent).toContain('Caption:');
    expect(container.textContent).toContain('alt-text');
  });

  it('does NOT render the literal "// TODO:" text (Bug A)', () => {
    const { container } = mount([
      FIGURE_WITH_SHORT_CAPTION('alt-text', [PARA('figure body')]),
    ]);
    // The pre-carve-up `renderChildrenRegistry.Figure` had a `// TODO:`
    // text inside a JSX fragment that got rendered as DOM text. The
    // carve-up drops it; this test guards against a regression.
    expect(container.textContent).not.toContain('TODO:');
    expect(container.textContent).not.toContain('// TODO');
  });

  it('omits the Caption line when the figure has no short caption', () => {
    const figureNoCaption = {
      t: 'Figure',
      c: [
        ['', [], []],
        [null, []],
        [PARA('body only')],
      ],
    };
    const { container } = mount([figureNoCaption]);
    expect(container.textContent).toContain('Figure:');
    expect(container.textContent).toContain('body only');
    expect(container.textContent).not.toContain('Caption:');
  });
});

// ---------------------------------------------------------------------------
// Miss path: dispatcher fallback removal didn't break the "Not registered"
// rendering for unknown node types.
// ---------------------------------------------------------------------------

describe('q2-debug "Not registered" miss path', () => {
  it('renders a bordered "Not registered" message for an unknown block type', () => {
    const unknownBlock = { t: 'TotallyMadeUpBlock', c: [] };
    const { container } = mount([unknownBlock]);
    expect(container.textContent).toContain('Not registered: TotallyMadeUpBlock');
  });
});

// ---------------------------------------------------------------------------
// Override path: format-side registry layering (the contract Plan 2pre
// preserves so user-TSX overrides keep working).
// ---------------------------------------------------------------------------

describe('q2-debug override path', () => {
  it('user-registered Para overrides q2-debug bordered Para', () => {
    const overrideRegistry = {
      ...q2DebugRegistry,
      Para: ({ node }: any) => (
        <p data-testid="user-para">{node.c.map((n: any) => n.c).join('')}</p>
      ),
    };

    const { queryByTestId, container } = render(
      <Ast
        astJson={astJson([PARA('hello override')])}
        currentFilePath="/project/test.qmd"
        onNavigateToDocument={noopNav}
        setAst={noopSet}
        registry={overrideRegistry}
      />,
    );

    expect(queryByTestId('user-para')).not.toBeNull();
    expect(queryByTestId('user-para')!.textContent).toBe('hello override');
    // q2-debug bordered Para's "Para:" prefix should NOT appear because the
    // override took over completely.
    expect(container.textContent).not.toContain('Para:');
  });
});

// ---------------------------------------------------------------------------
// Atomic-aware gate parity (Plan 2B Phase 5.1).
//
// The atomic gate sits in framework's `Node` (in dispatch.tsx) so it
// benefits both formats automatically. Locking the q2-debug behavior
// here means a future regression in the gate (or a reorganization
// that accidentally moves it downstream of one format's dispatcher)
// fails on q2-debug's integration tests, not just q2-preview's.
// ---------------------------------------------------------------------------

import { vi } from 'vitest';

describe('q2-debug atomic-gate parity (framework gate fires regardless of format)', () => {
  it('atomic CustomInline (CrossrefResolvedRef) inside a Para receives a no-op setLocalAst', () => {
    // Capture the setLocalAst the Inline dispatcher receives. The
    // framework's atomic gate replaces it with NOOP for atomic
    // descendants; calling NOOP must not propagate to setAst.
    let captured: ((n: unknown) => void) | null = null;
    const CapturingInline = (args: any) => {
      if (args.node.t === 'CustomInline') {
        captured = args.setLocalAst;
      }
      return <span>captured</span>;
    };
    const setAstSpy = vi.fn();
    const ast = [
      {
        t: 'Para',
        c: [
          { t: 'Str', c: 'see ' },
          {
            t: 'CustomInline',
            type_name: 'CrossrefResolvedRef',
            slots: { suffix: { kind: 'inlines', value: [] } },
            plain_data: {
              identifier: 'fig-1', kind: 'Figure', ref_type: 'fig',
              resolved: true, kind_source: 'builtin',
              order: { section: [], order: 1 },
            },
            attr: ['', [], []],
          },
        ],
      },
    ];
    const merged = {
      ...q2DebugRegistry,
      Inline: CapturingInline,
    };

    render(
      <Ast
        astJson={astJson(ast)}
        currentFilePath="/project/test.qmd"
        onNavigateToDocument={noopNav}
        setAst={setAstSpy}
        registry={merged}
      />,
    );

    expect(captured).not.toBeNull();
    // Invoke the captured setLocalAst with an arbitrary edit. If the
    // gate fired, it's NOOP and setAstSpy stays uncalled. If the gate
    // failed open, the edit propagates to setAst.
    captured!({ t: 'Str', c: 'EDITED' });
    expect(setAstSpy).not.toHaveBeenCalled();
  });
});
