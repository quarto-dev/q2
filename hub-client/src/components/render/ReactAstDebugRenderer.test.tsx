/**
 * Tests for Node rendering with attribution in ReactAstDebugRenderer.
 *
 * Test spec 5 from the plan.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect } from 'vitest';
import React from 'react';
import { render } from '@testing-library/react';
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
  it('renders colored badge with author name on attributed nodes', () => {
    const mockGetNodeAttribution = (_sourceInfoId: number): NodeAttribution | null => ({
      actor: 'actor1',
      time: 1700000000000,
      color: '#E91E63',
      name: 'Alice',
    });

    const astJson = makeAstJson({ sourceInfoId: 42 });

    const { container } = render(
      <NodeAttributionContext.Provider value={{ getNodeAttribution: mockGetNodeAttribution }}>
        <Ast astJson={astJson} setAst={() => {}} />
      </NodeAttributionContext.Provider>
    );

    // Badge element should exist with author name
    const badge = container.querySelector('.q2-attr-badge');
    expect(badge).not.toBeNull();
    expect(badge!.textContent).toContain('Alice');

    // Wrapper should have the attribution color
    const wrapper = container.querySelector('.q2-attr-wrap');
    expect(wrapper).not.toBeNull();
    expect((wrapper as HTMLElement).style.color).toBe('rgb(233, 30, 99)'); // #E91E63
  });

  it('renders without badge when attribution context is null', () => {
    const astJson = makeAstJson({ sourceInfoId: 42 });

    const { container } = render(
      <Ast astJson={astJson} setAst={() => {}} />
    );

    // No badge or attribution wrapper
    expect(container.querySelector('.q2-attr-badge')).toBeNull();
    expect(container.querySelector('.q2-attr-wrap')).toBeNull();

    // Str still renders
    const strSpan = Array.from(container.querySelectorAll('span')).find(
      s => s.textContent?.includes('hello'),
    );
    expect(strSpan).toBeTruthy();
  });

  it('renders without badge when node has no s field', () => {
    const mockGetNodeAttribution = (_id: number): NodeAttribution | null => ({
      actor: 'actor1',
      time: 1700000000000,
      color: '#E91E63',
      name: 'Alice',
    });

    // No sourceInfoId on the Str node
    const astJson = makeAstJson();

    const { container } = render(
      <NodeAttributionContext.Provider value={{ getNodeAttribution: mockGetNodeAttribution }}>
        <Ast astJson={astJson} setAst={() => {}} />
      </NodeAttributionContext.Provider>
    );

    // No badge since node has no source info
    expect(container.querySelector('.q2-attr-badge')).toBeNull();
  });
});
