import { describe, it, expect } from 'vitest';
import { buildCustomRegistry } from './customRegistry';

describe('buildCustomRegistry', () => {
  it('returns an empty registry for an empty module list', () => {
    expect(buildCustomRegistry([])).toEqual({});
  });

  it('collects exports from a single module', () => {
    const A = () => null;
    const registry = buildCustomRegistry([{ Para: A }]);
    expect(registry).toEqual({ Para: A });
  });

  it('accumulates exports across multiple modules (regression: bd-3day)', () => {
    // The bug was: customRegistry = { ...componentRegistry, ...module }
    // discarded earlier modules' exports. This test fails if the loop body
    // ever resets to anything other than the prior iteration's accumulator.
    const A = () => null;
    const B = () => null;
    const C = () => null;
    const registry = buildCustomRegistry([
      { Para: A },
      { Header: B },
      { Div: C },
    ]);
    expect(registry).toEqual({ Para: A, Header: B, Div: C });
  });

  it('later modules override earlier modules for the same export name', () => {
    const First = () => null;
    const Second = () => null;
    const registry = buildCustomRegistry([
      { Div: First },
      { Div: Second },
    ]);
    expect(registry).toEqual({ Div: Second });
  });
});
