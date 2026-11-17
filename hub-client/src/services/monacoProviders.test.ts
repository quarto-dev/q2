/**
 * Phase 6 unit tests: the semantic-tokens delta-encoder and the provider's
 * cancel / stale / empty return-shape contract. Pure — no Monaco runtime, no
 * real WASM (the intelligence service is mocked).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type * as Monaco from 'monaco-editor';

// Mock the intelligence service so the provider's WASM call is controllable.
const getSemanticTokensForContentMock = vi.fn();
vi.mock('./intelligenceService', () => ({
  getSemanticTokensForContent: (...args: unknown[]) =>
    getSemanticTokensForContentMock(...args),
  getSymbols: vi.fn(async () => []),
  getFoldingRanges: vi.fn(async () => []),
  QMD_TOKEN_LEGEND: ['qmd.markup.heading', 'qmd.code.keyword'],
}));

import {
  encodeSemanticTokens,
  registerIntelligenceProviders,
  disposeIntelligenceProviders,
  refreshSemanticTokens,
} from './monacoProviders';

describe('encodeSemanticTokens', () => {
  it('delta-encodes tokens into Monaco 5-tuples', () => {
    const data = encodeSemanticTokens([
      { line: 0, character: 0, length: 1, tokenType: 3, modifiers: 0 },
      { line: 0, character: 5, length: 4, tokenType: 7, modifiers: 0 },
      { line: 2, character: 2, length: 3, tokenType: 1, modifiers: 0 },
    ]);
    expect(Array.from(data)).toEqual([
      // deltaLine, deltaStartChar, length, type, mods
      0, 0, 1, 3, 0, // first token: absolute
      0, 5, 4, 7, 0, // same line: deltaChar = 5 - 0
      2, 2, 3, 1, 0, // line +2: deltaChar absolute again
    ]);
  });

  it('encodes an empty token list to an empty array', () => {
    expect(encodeSemanticTokens([]).length).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// Provider return-shape contract
// ---------------------------------------------------------------------------

type SemanticProvider = Monaco.languages.DocumentSemanticTokensProvider;

/** Minimal stand-in for Monaco's `Emitter`, recording subscribers. */
class StubEmitter<T> {
  private listeners: Array<(e: T) => void> = [];
  readonly event = (listener: (e: T) => void) => {
    this.listeners.push(listener);
    return {
      dispose: () => {
        this.listeners = this.listeners.filter((l) => l !== listener);
      },
    };
  };
  fire = (e: T) => {
    for (const l of this.listeners) l(e);
  };
  dispose = () => {
    this.listeners = [];
  };
}

/** Build a stub Monaco, optionally observing semantic-tokens registrations. */
function makeStubMonaco(
  onSemanticRegister?: (provider: SemanticProvider) => void
): typeof Monaco {
  const disposable = { dispose: () => {} };
  return {
    Emitter: StubEmitter,
    languages: {
      registerDocumentSymbolProvider: () => disposable,
      registerFoldingRangeProvider: () => disposable,
      registerDocumentSemanticTokensProvider: (
        _lang: string,
        provider: SemanticProvider
      ) => {
        onSemanticRegister?.(provider);
        return disposable;
      },
    },
  } as unknown as typeof Monaco;
}

/** Capture the semantic-tokens provider registered with a stub Monaco. */
function captureProvider(currentPath: string | null): SemanticProvider {
  let captured: SemanticProvider | null = null;
  const monaco = makeStubMonaco((provider) => {
    captured = provider;
  });

  registerIntelligenceProviders(monaco, () => currentPath);
  if (!captured) throw new Error('semantic provider was not registered');
  return captured;
}

function stubToken(cancelled: boolean): Monaco.CancellationToken {
  return {
    isCancellationRequested: cancelled,
    onCancellationRequested: () => ({ dispose: () => {} }),
  } as unknown as Monaco.CancellationToken;
}

function stubModel(versionIds: number[], value = 'model text'): Monaco.editor.ITextModel {
  let i = 0;
  return {
    getVersionId: () => versionIds[Math.min(i++, versionIds.length - 1)],
    getValue: () => value,
    onDidChangeContent: () => ({ dispose: () => {} }),
  } as unknown as Monaco.editor.ITextModel;
}

function stubQuietModel(initialValue = 'initial') {
  let value = initialValue;
  let version = 1;
  const listeners = new Set<() => void>();
  const model = {
    getVersionId: () => version,
    getValue: () => value,
    onDidChangeContent: (listener: () => void) => {
      listeners.add(listener);
      return { dispose: () => listeners.delete(listener) };
    },
  } as unknown as Monaco.editor.ITextModel;
  return {
    model,
    change(nextValue: string) {
      value = nextValue;
      version += 1;
      for (const listener of listeners) listener();
    },
  };
}

function stubCancellableToken() {
  const listeners = new Set<() => void>();
  let cancelled = false;
  const token = {
    get isCancellationRequested() {
      return cancelled;
    },
    onCancellationRequested: (listener: () => void) => {
      listeners.add(listener);
      return { dispose: () => listeners.delete(listener) };
    },
  } as Monaco.CancellationToken;
  return {
    token,
    cancel() {
      cancelled = true;
      for (const listener of listeners) listener();
    },
  };
}

describe('provideDocumentSemanticTokens return shape', () => {
  beforeEach(() => {
    getSemanticTokensForContentMock.mockReset();
    disposeIntelligenceProviders();
  });

  it('tokenises the model content, not the VFS image by path', async () => {
    // Regression: tokens were computed from the VFS image while Monaco renders
    // the model. When the two drift, every token shifts and smears colours onto
    // adjacent characters (e.g. an image opener token landing on a link's `[`).
    getSemanticTokensForContentMock.mockResolvedValue([]);
    const provider = captureProvider('/project/a.qmd');
    await provider.provideDocumentSemanticTokens(
      stubModel([1], '[label](url)'),
      null,
      stubToken(false),
    );
    expect(getSemanticTokensForContentMock).toHaveBeenCalledWith(
      '/project/a.qmd',
      '[label](url)',
    );
  });

  it('returns null when the request is already cancelled', async () => {
    getSemanticTokensForContentMock.mockResolvedValue([]);
    const provider = captureProvider('/project/a.qmd');
    const result = await provider.provideDocumentSemanticTokens(
      stubModel([1]),
      null,
      stubToken(true),
    );
    expect(result).toBeNull();
  });

  it('returns null when the model version changes across the await', async () => {
    getSemanticTokensForContentMock.mockResolvedValue([
      { line: 0, character: 0, length: 1, tokenType: 0, modifiers: 0 },
    ]);
    const provider = captureProvider('/project/a.qmd');
    // versionId 1 before the await, 2 after → stale.
    const result = await provider.provideDocumentSemanticTokens(
      stubModel([1, 2]),
      null,
      stubToken(false),
    );
    expect(result).toBeNull();
  });

  it('returns empty Uint32Array (not null) for a valid empty result', async () => {
    getSemanticTokensForContentMock.mockResolvedValue([]);
    const provider = captureProvider('/project/a.qmd');
    const result = await provider.provideDocumentSemanticTokens(
      stubModel([1]),
      null,
      stubToken(false),
    );
    expect(result).not.toBeNull();
    const tokens = result as Monaco.languages.SemanticTokens;
    expect(tokens.data).toBeInstanceOf(Uint32Array);
    expect(tokens.data.length).toBe(0);
    expect(tokens.resultId).toBeUndefined();
  });

  it('delta-encodes a non-empty result', async () => {
    getSemanticTokensForContentMock.mockResolvedValue([
      { line: 1, character: 2, length: 3, tokenType: 1, modifiers: 0 },
    ]);
    const provider = captureProvider('/project/a.qmd');
    const result = (await provider.provideDocumentSemanticTokens(
      stubModel([1]),
      null,
      stubToken(false),
    )) as Monaco.languages.SemanticTokens;
    expect(Array.from(result.data)).toEqual([1, 2, 3, 1, 0]);
  });

  it('returns null when there is no current file path', async () => {
    const provider = captureProvider(null);
    const result = await provider.provideDocumentSemanticTokens(
      stubModel([1]),
      null,
      stubToken(false),
    );
    expect(result).toBeNull();
    expect(getSemanticTokensForContentMock).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Identical-request coalescing
// ---------------------------------------------------------------------------

describe('semantic-tokens identical-request coalescing', () => {
  beforeEach(() => {
    getSemanticTokensForContentMock.mockReset();
    disposeIntelligenceProviders();
  });

  it('shares concurrent and completed requests for the same path and content', async () => {
    vi.useFakeTimers();
    try {
      let resolveTokens!: (tokens: SemanticToken[]) => void;
      getSemanticTokensForContentMock.mockImplementation(
        () => new Promise<SemanticToken[]>((resolve) => {
          resolveTokens = resolve;
        }),
      );
      const provider = captureProvider('/project/a.qmd');
      const model = stubModel([1], '[label](url)');

      const first = provider.provideDocumentSemanticTokens(model, null, stubToken(false));
      const second = provider.provideDocumentSemanticTokens(model, null, stubToken(false));
      await vi.advanceTimersByTimeAsync(300);
      expect(getSemanticTokensForContentMock).toHaveBeenCalledTimes(1);

      resolveTokens([{ line: 0, character: 1, length: 5, tokenType: 1, modifiers: 0 }]);
      const [firstResult, secondResult] = await Promise.all([first, second]);
      expect(Array.from((firstResult as Monaco.languages.SemanticTokens).data)).toEqual([
        0, 1, 5, 1, 0,
      ]);
      expect(Array.from((secondResult as Monaco.languages.SemanticTokens).data)).toEqual([
        0, 1, 5, 1, 0,
      ]);

      const third = provider.provideDocumentSemanticTokens(model, null, stubToken(false));
      await vi.advanceTimersByTimeAsync(300);
      await third;
      expect(getSemanticTokensForContentMock).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it('retries an empty result instead of caching a possible transient failure', async () => {
    getSemanticTokensForContentMock.mockResolvedValue([]);
    const provider = captureProvider('/project/a.qmd');
    const model = stubModel([1], 'plain text');

    await provider.provideDocumentSemanticTokens(model, null, stubToken(false));
    await provider.provideDocumentSemanticTokens(model, null, stubToken(false));

    expect(getSemanticTokensForContentMock).toHaveBeenCalledTimes(2);
  });

  it('drops the shared request when providers are disposed', async () => {
    getSemanticTokensForContentMock.mockResolvedValue([
      { line: 0, character: 0, length: 1, tokenType: 0, modifiers: 0 },
    ]);
    const model = stubModel([1], '[label](url)');
    await captureProvider('/project/a.qmd').provideDocumentSemanticTokens(
      model,
      null,
      stubToken(false),
    );

    disposeIntelligenceProviders();
    await captureProvider('/project/a.qmd').provideDocumentSemanticTokens(
      model,
      null,
      stubToken(false),
    );

    expect(getSemanticTokensForContentMock).toHaveBeenCalledTimes(2);
  });
});

// ---------------------------------------------------------------------------
// Quiet-window scheduling
// ---------------------------------------------------------------------------

describe('semantic-tokens quiet-window scheduling', () => {
  beforeEach(() => {
    getSemanticTokensForContentMock.mockReset();
    disposeIntelligenceProviders();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('tokenises once with the latest content after the model becomes quiet', async () => {
    getSemanticTokensForContentMock.mockResolvedValue([
      { line: 0, character: 0, length: 4, tokenType: 1, modifiers: 0 },
    ]);
    const provider = captureProvider('/project/a.qmd');
    const { model, change } = stubQuietModel();

    const pending = provider.provideDocumentSemanticTokens(model, null, stubToken(false));
    expect(getSemanticTokensForContentMock).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(150);
    change('first edit');
    await vi.advanceTimersByTimeAsync(150);
    change('final content');
    await vi.advanceTimersByTimeAsync(299);
    expect(getSemanticTokensForContentMock).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    expect(getSemanticTokensForContentMock).toHaveBeenCalledTimes(1);
    expect(getSemanticTokensForContentMock).toHaveBeenCalledWith(
      '/project/a.qmd',
      'final content',
    );
    const result = await pending;
    expect(Array.from((result as Monaco.languages.SemanticTokens).data)).toEqual([
      0, 0, 4, 1, 0,
    ]);
  });

  it('cancels before tokenization when the quiet-window request is cancelled', async () => {
    getSemanticTokensForContentMock.mockResolvedValue([]);
    const provider = captureProvider('/project/a.qmd');
    const { model } = stubQuietModel();
    const cancellation = stubCancellableToken();

    const pending = provider.provideDocumentSemanticTokens(model, null, cancellation.token);
    cancellation.cancel();
    await vi.advanceTimersByTimeAsync(300);

    expect(await pending).toBeNull();
    expect(getSemanticTokensForContentMock).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Immediate-refresh contract (skip the debounce on file open)
// ---------------------------------------------------------------------------

describe('semantic-tokens immediate refresh', () => {
  beforeEach(() => {
    disposeIntelligenceProviders();
  });

  it('exposes an onDidChange so Monaco can re-tokenise on demand', () => {
    const provider = captureProvider('/project/a.qmd');
    expect(typeof provider.onDidChange).toBe('function');
  });

  it('refreshSemanticTokens fires onDidChange (forces Monaco schedule(0))', () => {
    const provider = captureProvider('/project/a.qmd');
    const listener = vi.fn();
    provider.onDidChange?.(listener);
    refreshSemanticTokens();
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('refreshSemanticTokens is a no-op when no providers are registered', () => {
    disposeIntelligenceProviders();
    expect(() => refreshSemanticTokens()).not.toThrow();
  });

  it('registers the semantic-tokens provider once across repeated mounts', () => {
    // The editor remounts per file open; re-registering would reschedule the
    // fetch with the adaptive debounce instead of firing immediately.
    let registrations = 0;
    const monaco = makeStubMonaco(() => {
      registrations++;
    });
    registerIntelligenceProviders(monaco, () => '/project/a.qmd');
    registerIntelligenceProviders(monaco, () => '/project/a.qmd');
    expect(registrations).toBe(1);
  });
});
