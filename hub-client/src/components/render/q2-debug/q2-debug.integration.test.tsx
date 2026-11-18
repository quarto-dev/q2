/**
 * Contract for q2-debug's rendering surface.
 *
 * q2-debug renders the AST as a collapsible tree (`.q2d`): each node is
 * a tag (`.q2d-tag`, the node type) followed by its properties and
 * children. These tests pin what a reader of that tree relies on, so
 * other formats can keep treating q2-debug as a reference output.
 *
 * Coverage:
 *   - Tree view — every node shows its type as a tag, with its
 *     properties (Header level, list item count) and its text.
 *   - Figure caption — a short caption is shown in a `caption:` slot,
 *     and the slot is absent when there is no short caption.
 *   - Bug A: the literal `// TODO:` text from the old Figure entry is
 *     gone from the rendered DOM.
 *   - Unknown node types still render, tagged as unknown
 *     (`.q2d-tag-unknown`), rather than breaking the tree.
 *   - Override path resolves: registering a custom Para under the
 *     q2-debug registry wins over q2-debug's own Para. (Locks the
 *     framework→<Block>→registry[node.t] dispatch chain.)
 */

import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { Ast } from '@quarto/preview-renderer/framework';
import type { PandocAST } from '@quarto/preview-renderer/framework';
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

/** Node-type tags in the rendered tree, in document order. */
function tags(container: HTMLElement): string[] {
  return [...container.querySelectorAll('.q2d-tag')].map((el) => el.textContent ?? '');
}

/** Slot labels (`caption:` etc.) in the rendered tree. */
function slots(container: HTMLElement): string[] {
  return [...container.querySelectorAll('.q2d-slot')].map((el) => el.textContent ?? '');
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
// Tree view
// ---------------------------------------------------------------------------

describe('q2-debug tree view', () => {
  it('renders Para as a tagged node in the tree', () => {
    const { container } = mount([PARA('hello')]);
    expect(container.querySelector('.q2d')).not.toBeNull();
    expect(tags(container)).toContain('Para');
    expect(container.textContent).toContain('hello');
  });

  it('renders Header as a tagged node with the level annotation', () => {
    const { container } = mount([HEADER(2, 'Section')]);
    expect(tags(container)).toContain('Header');
    expect(container.textContent).toContain('level=2');
    expect(container.textContent).toContain('Section');
  });

  it('renders BulletList items recursively', () => {
    const { container } = mount([
      BULLET_LIST([[PARA('item one')], [PARA('item two')]]),
    ]);
    expect(tags(container)).toContain('BulletList');
    expect(container.textContent).toContain('items=2');
    expect(tags(container).filter((t) => t === 'Para')).toHaveLength(2);
    expect(container.textContent).toContain('item one');
    expect(container.textContent).toContain('item two');
  });
});

// ---------------------------------------------------------------------------
// Figure: Bug A + caption-branch port
// ---------------------------------------------------------------------------

describe('q2-debug Figure (Bug A + caption slot)', () => {
  it('renders the "caption:" slot for a short-captioned figure', () => {
    const { container } = mount([
      FIGURE_WITH_SHORT_CAPTION('alt-text', [PARA('figure body')]),
    ]);

    // Figure node tag
    expect(tags(container)).toContain('Figure');
    // Body blocks are rendered as the figure's children
    expect(container.textContent).toContain('figure body');
    // The short caption gets its own labeled slot
    expect(slots(container)).toContain('caption:');
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

  it('omits the caption slot when the figure has no short caption', () => {
    const figureNoCaption = {
      t: 'Figure',
      c: [
        ['', [], []],
        [null, []],
        [PARA('body only')],
      ],
    };
    const { container } = mount([figureNoCaption]);
    expect(tags(container)).toContain('Figure');
    expect(container.textContent).toContain('body only');
    expect(slots(container)).not.toContain('caption:');
  });
});

// ---------------------------------------------------------------------------
// Miss path: an unknown node type still renders, marked as unknown.
// ---------------------------------------------------------------------------

describe('q2-debug unknown-node miss path', () => {
  it('renders an unknown block type as a node tagged unknown', () => {
    const unknownBlock = { t: 'TotallyMadeUpBlock', c: [] };
    const { container } = mount([unknownBlock]);
    const unknown = container.querySelector('.q2d-tag-unknown');
    expect(unknown).not.toBeNull();
    expect(unknown!.textContent).toBe('TotallyMadeUpBlock');
  });
});

// ---------------------------------------------------------------------------
// Override path: format-side registry layering (the contract Plan 2pre
// preserves so user-TSX overrides keep working).
// ---------------------------------------------------------------------------

describe('q2-debug override path', () => {
  it('user-registered Para overrides q2-debug Para', () => {
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
    // q2-debug's own "Para" tag should NOT appear because the override
    // took over completely.
    expect(tags(container)).not.toContain('Para');
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
