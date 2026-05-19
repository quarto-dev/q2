/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/react';
import { Ast } from '../framework';
import { q2DebugRegistry } from './registry';

afterEach(() => {
  cleanup();
});

const noopSetAst = () => {};

/**
 * Phase 5c — when `astContext.attribution` / `astContext.attributionActors`
 * are absent, the q2-debug renderer wraps no nodes and paints no colour.
 * When they're present, each annotated node gets a `q2-attr-wrap`
 * wrapper carrying `data-sid=<s>` (used for badge lookup) and
 * `data-attr-actor=<actor>` (matched by the per-actor CSS rule the
 * framework injects); the colour reaches the wrap via the cascade,
 * not via an inline `style`. Hovering surfaces a single floating
 * badge with the author's name and a relative-time string.
 *
 * Mounted via the framework `Ast` with the `q2DebugRegistry` so we
 * exercise the same wiring the iframe uses at runtime.
 */
describe('q2-debug attribution wiring', () => {
  it('off path: no q2-attr-wrap and no inline colour', () => {
    const ast = {
      'pandoc-api-version': [1, 23, 1],
      meta: {},
      blocks: [{ t: 'Para', s: 1, c: [{ t: 'Str', s: 2, c: 'hello' }] }],
    };
    const { container } = render(
      <Ast
        astJson={JSON.stringify(ast)}
        currentFilePath=""
        setAst={noopSetAst}
        registry={q2DebugRegistry}
      />,
    );
    expect(container.querySelector('.q2-attr-wrap')).toBeNull();
    expect(container.querySelector('.q2-attr-badge')).toBeNull();
    // Existing Para label still renders.
    expect(container.textContent).toMatch(/Para/);
    expect(container.textContent).toMatch(/hello/);
  });

  it('on path: each annotated node gets a wrap with actor + sid', () => {
    const ast = {
      'pandoc-api-version': [1, 23, 1],
      meta: {},
      blocks: [{ t: 'Para', s: 1, c: [{ t: 'Str', s: 2, c: 'hello' }] }],
      astContext: {
        attribution: [
          { s: 1, actor: 'alice', time: Date.now() },
          { s: 2, actor: 'alice', time: Date.now() },
        ],
        attributionActors: {
          alice: { name: 'Alice', color: '#ff0000' },
        },
      },
    };
    const { container } = render(
      <Ast
        astJson={JSON.stringify(ast)}
        currentFilePath=""
        setAst={noopSetAst}
        registry={q2DebugRegistry}
      />,
    );

    const wraps = container.querySelectorAll('.q2-attr-wrap');
    // One block-level Para wrapper, one inline-level Str wrapper.
    expect(wraps.length).toBe(2);

    for (const wrap of Array.from(wraps)) {
      const el = wrap as HTMLElement;
      // Identity is render-time CSS now: the wrap carries the
      // per-actor selector key, not an inline colour. The cascade
      // resolves `color: var(--attr-color)` from the injected rule.
      expect(el.getAttribute('data-attr-actor')).toBe('alice');
      expect(el.getAttribute('data-sid')).toMatch(/^[12]$/);
      expect(el.style.color).toBe('');
    }

    // The framework injects a single per-render <style> carrying the
    // static viewer.css plus the per-actor rule for "alice".
    const styles = Array.from(container.querySelectorAll('style')).map(
      (s) => s.textContent ?? '',
    );
    expect(
      styles.some((s) => s.includes('[data-attr-actor="alice"]') && s.includes('--attr-color: #ff0000') && s.includes('--attr-name: "Alice"')),
    ).toBe(true);

    // No badge yet — hover hasn't fired.
    expect(container.querySelector('.q2-attr-badge')).toBeNull();
  });

  it('hover surfaces a single badge with name + relative time', () => {
    const ast = {
      'pandoc-api-version': [1, 23, 1],
      meta: {},
      blocks: [{ t: 'Para', s: 1, c: [{ t: 'Str', s: 2, c: 'hello' }] }],
      astContext: {
        attribution: [
          // 90 seconds ago → "1m ago".
          { s: 1, actor: 'alice', time: Date.now() - 90_000 },
          { s: 2, actor: 'alice', time: Date.now() - 90_000 },
        ],
        attributionActors: {
          alice: { name: 'Alice', color: '#ff0000' },
        },
      },
    };
    const { container } = render(
      <Ast
        astJson={JSON.stringify(ast)}
        currentFilePath=""
        setAst={noopSetAst}
        registry={q2DebugRegistry}
      />,
    );

    const wrap = container.querySelector('.q2-attr-wrap[data-sid="2"]') as HTMLElement;
    expect(wrap).not.toBeNull();
    fireEvent.mouseOver(wrap);

    const badge = container.querySelector('.q2-attr-badge') as HTMLElement | null;
    expect(badge).not.toBeNull();
    expect(badge!.textContent).toMatch(/Alice/);
    expect(badge!.textContent).toMatch(/m ago/);
  });

  it('on path: actor with no entry in attributionActors falls through', () => {
    const ast = {
      'pandoc-api-version': [1, 23, 1],
      meta: {},
      blocks: [{ t: 'Para', s: 1, c: [{ t: 'Str', s: 2, c: 'world' }] }],
      astContext: {
        attribution: [{ s: 1, actor: 'ghost', time: Date.now() }],
        attributionActors: {}, // no entry for "ghost"
      },
    };
    const { container } = render(
      <Ast
        astJson={JSON.stringify(ast)}
        currentFilePath=""
        setAst={noopSetAst}
        registry={q2DebugRegistry}
      />,
    );
    expect(container.querySelector('.q2-attr-wrap')).toBeNull();
    expect(container.querySelector('.q2-attr-badge')).toBeNull();
  });
});
