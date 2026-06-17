/**
 * Regression test for the disposed-provider bug: opening/switching a document
 * could tear down the Monaco intelligence providers (semantic tokens included)
 * with no re-registration, so link brackets `[`/`]` rendered via the Monarch
 * base only — mismatched — until a full page reload. The lifecycle contract is:
 * register on editor mount, dispose ONLY on unmount, never on re-render.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import type * as Monaco from 'monaco-editor';

const registerSpy = vi.fn();
const disposeSpy = vi.fn();
const refreshSpy = vi.fn();
vi.mock('../services/monacoProviders', () => ({
  registerIntelligenceProviders: (...args: unknown[]) => registerSpy(...args),
  disposeIntelligenceProviders: (...args: unknown[]) => disposeSpy(...args),
  refreshSemanticTokens: (...args: unknown[]) => refreshSpy(...args),
}));

import { useIntelligenceProviders } from './useIntelligenceProviders';

const fakeMonaco = {} as unknown as typeof Monaco;

describe('useIntelligenceProviders', () => {
  beforeEach(() => {
    registerSpy.mockReset();
    disposeSpy.mockReset();
    refreshSpy.mockReset();
  });

  it('registers on editor mount and disposes only on unmount', () => {
    // Each render passes a fresh getter identity, mimicking how the Editor's
    // path getter changes when `currentFile` changes identity.
    const { result, rerender, unmount } = renderHook(
      ({ path }) => useIntelligenceProviders(() => path),
      { initialProps: { path: 'index.qmd' } }
    );

    // Editor mounts → register once, and force an immediate re-tokenise so the
    // correct colours appear without waiting out Monaco's adaptive debounce.
    result.current(fakeMonaco);
    expect(registerSpy).toHaveBeenCalledTimes(1);
    expect(refreshSpy).toHaveBeenCalledTimes(1);
    expect(disposeSpy).not.toHaveBeenCalled();

    // currentFile changes identity, same path (no Monaco remount) — must NOT
    // dispose the providers.
    rerender({ path: 'index.qmd' });
    // currentFile changes to a different path.
    rerender({ path: 'other.qmd' });
    expect(disposeSpy).not.toHaveBeenCalled();

    // Only a true unmount disposes, exactly once.
    unmount();
    expect(disposeSpy).toHaveBeenCalledTimes(1);
  });
});
