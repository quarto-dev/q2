/**
 * Vitest coverage for `UserGrammarCache` — Phase 4.5.
 *
 * Tests use an in-memory loader so they do not depend on
 * web-tree-sitter. The cache contract is: given a list of
 * descriptors + content resolvers, produce a set of registered
 * grammars whose highlighters are loaded once per unique (path,
 * content-hash) and disposed when their descriptor goes away.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { UserGrammarCache } from './userGrammarCache';
import type { GrammarDescriptor } from './userGrammarDiscovery';
import type {
  UserGrammarHighlighter,
  LoadUserGrammarArgs,
} from './userGrammarHighlight';

interface FakeBinary {
  bytes: Uint8Array;
  revision: number;
}

/** Build a minimal faux loader that tracks call counts + disposals. */
function makeFakeLoader() {
  const loaded: string[] = [];
  const disposed: string[] = [];
  const loader = vi.fn(
    async (args: LoadUserGrammarArgs): Promise<UserGrammarHighlighter> => {
      loaded.push(args.name);
      return {
        name: args.name,
        highlight: (src) => `highlighted:${args.name}:${src}`,
        dispose: () => {
          disposed.push(args.name);
        },
      };
    },
  );
  return { loader, loaded, disposed };
}

function bytes(...values: number[]): Uint8Array {
  return new Uint8Array(values);
}

describe('UserGrammarCache', () => {
  let cache: UserGrammarCache;
  let binaries: Map<string, Uint8Array>;
  let texts: Map<string, string>;

  beforeEach(() => {
    binaries = new Map();
    texts = new Map();
  });

  afterEach(() => {
    cache?.disposeAll();
  });

  const makeCache = () => {
    const { loader, loaded, disposed } = makeFakeLoader();
    cache = new UserGrammarCache({
      loadUserGrammar: loader,
      getBinaryContent: async (p) => binaries.get(p) ?? null,
      getTextContent: async (p) => texts.get(p) ?? null,
    });
    return { loader, loaded, disposed };
  };

  it('loads each discovered grammar on the first sync', async () => {
    const { loader } = makeCache();
    binaries.set('_quarto/grammars/toml/toml.wasm', bytes(1, 2, 3));
    texts.set('_quarto/grammars/toml/highlights.scm', '(a) @b');

    const descriptors: GrammarDescriptor[] = [
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/toml/toml.wasm',
        highlightsPath: '_quarto/grammars/toml/highlights.scm',
      },
    ];

    const result = await cache.sync(descriptors);
    expect(result.classes).toEqual(['toml']);
    expect(result.failures).toEqual([]);
    expect(loader).toHaveBeenCalledTimes(1);
  });

  it('reuses the highlighter when bytes + scm are unchanged', async () => {
    const { loader } = makeCache();
    binaries.set('_quarto/grammars/toml/toml.wasm', bytes(1, 2, 3));
    texts.set('_quarto/grammars/toml/highlights.scm', '(a) @b');
    const d: GrammarDescriptor[] = [
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/toml/toml.wasm',
        highlightsPath: '_quarto/grammars/toml/highlights.scm',
      },
    ];

    await cache.sync(d);
    await cache.sync(d);
    expect(loader).toHaveBeenCalledTimes(1);
  });

  it('reloads and disposes the old highlighter when bytes change', async () => {
    const { loader, disposed } = makeCache();
    binaries.set('_quarto/grammars/toml/toml.wasm', bytes(1, 2, 3));
    texts.set('_quarto/grammars/toml/highlights.scm', '(a) @b');
    const d: GrammarDescriptor[] = [
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/toml/toml.wasm',
        highlightsPath: '_quarto/grammars/toml/highlights.scm',
      },
    ];

    await cache.sync(d);
    // Simulate user editing the grammar.
    binaries.set('_quarto/grammars/toml/toml.wasm', bytes(9, 9, 9));
    await cache.sync(d);

    expect(loader).toHaveBeenCalledTimes(2);
    expect(disposed).toEqual(['toml']);
  });

  it('reloads when only the highlights.scm changes', async () => {
    const { loader } = makeCache();
    binaries.set('_quarto/grammars/toml/toml.wasm', bytes(1, 2, 3));
    texts.set('_quarto/grammars/toml/highlights.scm', '(a) @b');
    const d: GrammarDescriptor[] = [
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/toml/toml.wasm',
        highlightsPath: '_quarto/grammars/toml/highlights.scm',
      },
    ];

    await cache.sync(d);
    texts.set('_quarto/grammars/toml/highlights.scm', '(a) @different');
    await cache.sync(d);
    expect(loader).toHaveBeenCalledTimes(2);
  });

  it('drops grammars removed from the descriptor set', async () => {
    const { disposed } = makeCache();
    binaries.set('_quarto/grammars/toml/toml.wasm', bytes(1, 2, 3));
    texts.set('_quarto/grammars/toml/highlights.scm', '(a) @b');
    binaries.set('_quarto/grammars/zig/zig.wasm', bytes(4, 5, 6));
    texts.set('_quarto/grammars/zig/highlights.scm', '(z) @keyword');

    const both: GrammarDescriptor[] = [
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/toml/toml.wasm',
        highlightsPath: '_quarto/grammars/toml/highlights.scm',
      },
      {
        class: 'zig',
        wasmPath: '_quarto/grammars/zig/zig.wasm',
        highlightsPath: '_quarto/grammars/zig/highlights.scm',
      },
    ];
    await cache.sync(both);

    const onlyToml = both.slice(0, 1);
    const result = await cache.sync(onlyToml);
    expect(result.classes).toEqual(['toml']);
    expect(disposed).toContain('zig');
  });

  it('reports a failure when binary content is missing', async () => {
    makeCache();
    texts.set('_quarto/grammars/toml/highlights.scm', '(a) @b');
    // intentionally no binary
    const d: GrammarDescriptor[] = [
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/toml/toml.wasm',
        highlightsPath: '_quarto/grammars/toml/highlights.scm',
      },
    ];
    const result = await cache.sync(d);
    expect(result.classes).toEqual([]);
    expect(result.failures).toHaveLength(1);
    expect(result.failures[0].class).toBe('toml');
  });

  it('reports a failure when the loader throws', async () => {
    binaries.set('_quarto/grammars/toml/toml.wasm', bytes(1, 2, 3));
    texts.set('_quarto/grammars/toml/highlights.scm', '(a) @b');
    cache = new UserGrammarCache({
      loadUserGrammar: async () => {
        throw new Error('boom');
      },
      getBinaryContent: async (p) => binaries.get(p) ?? null,
      getTextContent: async (p) => texts.get(p) ?? null,
    });

    const d: GrammarDescriptor[] = [
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/toml/toml.wasm',
        highlightsPath: '_quarto/grammars/toml/highlights.scm',
      },
    ];
    const result = await cache.sync(d);
    expect(result.classes).toEqual([]);
    expect(result.failures).toHaveLength(1);
    expect(result.failures[0].reason).toContain('boom');
  });

  it('registerInto wires every cached highlighter into the handle', async () => {
    makeCache();
    binaries.set('_quarto/grammars/toml/toml.wasm', bytes(1, 2, 3));
    texts.set('_quarto/grammars/toml/highlights.scm', '(a) @b');
    const d: GrammarDescriptor[] = [
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/toml/toml.wasm',
        highlightsPath: '_quarto/grammars/toml/highlights.scm',
      },
    ];
    await cache.sync(d);

    const registered: Array<[string, string]> = [];
    const handle = {
      register(cls: string, fn: (c: string, s: string) => string | null | undefined) {
        const res = fn(cls, 'test-source');
        registered.push([cls, String(res)]);
      },
    };
    cache.registerInto(handle);
    expect(registered).toEqual([['toml', 'highlighted:toml:test-source']]);
  });

  it('disposeAll disposes every cached highlighter', async () => {
    const { disposed } = makeCache();
    binaries.set('_quarto/grammars/toml/toml.wasm', bytes(1, 2, 3));
    texts.set('_quarto/grammars/toml/highlights.scm', '(a) @b');
    const d: GrammarDescriptor[] = [
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/toml/toml.wasm',
        highlightsPath: '_quarto/grammars/toml/highlights.scm',
      },
    ];
    await cache.sync(d);
    cache.disposeAll();
    expect(disposed).toEqual(['toml']);
  });
});
