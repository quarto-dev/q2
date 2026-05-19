import type React from 'react';

export type ComponentExports = Record<string, React.ComponentType<any>>;

/**
 * Reduce a sequence of dynamically-imported render-component modules into
 * a single registry. Each module's exports layer on top of the previous
 * accumulator, so later modules win on name collisions and earlier modules
 * are never discarded.
 *
 * Lifted out of the iframe entry point so the merge step is unit-testable
 * without the blob-URL dynamic-import dance.
 */
export function buildCustomRegistry(modules: ComponentExports[]): ComponentExports {
  let registry: ComponentExports = {};
  for (const module of modules) {
    registry = { ...registry, ...module };
  }
  return registry;
}
