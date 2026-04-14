/**
 * Tests for Node rendering with attribution in ReactAstDebugRenderer.
 *
 * Test spec 5 from the plan.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect } from 'vitest';
import React from 'react';
import { render, screen } from '@testing-library/react';
import { Ast } from './ReactAstDebugRenderer';
import type { PandocAST } from './ReactAstDebugRenderer';
import { NodeAttributionContext } from './ReactAstDebugRenderer';
import type { NodeAttribution } from '../../services/attribution';

/** Helper to build minimal AST JSON with a single Str node */
function makeAstJson(opts?: { sourceInfoId?: number }): string {
  const strNode: Record<string, unknown> = { t: 'Str', c: 'hello' };
  if (opts?.sourceInfoId !== undefined) {
    strNode.s = opts.sourceInfoId;
  }

  const ast: PandocAST = {
    'pandoc-api-version': [1, 23, 1],
    meta: {},
    blocks: [{ t: 'Para', c: [strNode as any] }],
  };
  return JSON.stringify(ast);
}

describe('Node rendering with attribution', () => {
  it('renders with attribution color and title tooltip when context is provided', () => {
    const mockGetNodeAttribution = (sourceInfoId: number): NodeAttribution | null => ({
      actor: 'actor1',
      time: 1700000000000,
      color: '#E91E63',
      name: 'Alice',
    });

    const astJson = makeAstJson({ sourceInfoId: 42 });

    const { container } = render(
      <NodeAttributionContext.Provider value={{ getNodeAttribution: mockGetNodeAttribution }}>
        <Ast
          astJson={astJson}
          setAst={() => {}}
        />
      </NodeAttributionContext.Provider>
    );

    // Find the Str node's span — it should have the attribution color
    const strSpan = container.querySelector('[title]');
    expect(strSpan).not.toBeNull();
    expect(strSpan!.getAttribute('title')).toContain('Alice');
    expect((strSpan as HTMLElement).style.color).toBe('rgb(233, 30, 99)'); // #E91E63
  });

  it('renders identically to current behavior when attribution context is null', () => {
    const astJson = makeAstJson({ sourceInfoId: 42 });

    const { container: withoutCtx } = render(
      <Ast astJson={astJson} setAst={() => {}} />
    );

    // Should render without any attribution styling — no title attribute on Str
    const strSpans = withoutCtx.querySelectorAll('span');
    const strSpan = Array.from(strSpans).find(s => s.textContent?.includes('hello'));
    expect(strSpan).toBeTruthy();
    // No title attribute when no attribution context
    expect(strSpan!.hasAttribute('title')).toBe(false);
  });

  it('renders without attribution styling when node has no s field', () => {
    const mockGetNodeAttribution = (_id: number): NodeAttribution | null => ({
      actor: 'actor1',
      time: 1700000000000,
      color: '#E91E63',
      name: 'Alice',
    });

    // No sourceInfoId set on the Str node
    const astJson = makeAstJson();

    const { container } = render(
      <NodeAttributionContext.Provider value={{ getNodeAttribution: mockGetNodeAttribution }}>
        <Ast astJson={astJson} setAst={() => {}} />
      </NodeAttributionContext.Provider>
    );

    // Str span should render but without attribution (no title)
    const strSpans = container.querySelectorAll('span');
    const strSpan = Array.from(strSpans).find(s => s.textContent?.includes('hello'));
    expect(strSpan).toBeTruthy();
    expect(strSpan!.hasAttribute('title')).toBe(false);
  });
});
