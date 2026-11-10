/**
 * Cold-build race coverage for the stateful attribution hook. These tests use
 * the real Automerge producer and manually release its idle-callback yields.
 *
 * @vitest-environment jsdom
 */

import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  COLD_BUILD_ACTOR,
  COLD_BUILD_TEXT_LENGTH,
  createColdBuildAttributionFixture,
  installColdBuildIdleGate,
} from '../test-utils/attributionColdBuild';

const getFileHandle = vi.fn();
vi.mock('@quarto/preview-runtime', () => ({
  getFileHandle: (path: string) => getFileHandle(path),
}));

import { useAttribution } from './useAttribution';

interface HookProps {
  enabled: boolean;
  filePath: string;
  sourceText: string;
}

const IDENTITIES = {};

function renderAttribution(initialProps: HookProps) {
  return renderHook(
    (props: HookProps) => useAttribution({ ...props, identities: IDENTITIES }),
    { initialProps },
  );
}

function payloadRuns(payload: string | null) {
  expect(payload).not.toBeNull();
  return (JSON.parse(payload!) as {
    runs: Array<{ start: number; end: number; actor: string; time: number }>;
  }).runs;
}

beforeEach(() => {
  getFileHandle.mockReset();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('useAttribution cold-build races', () => {
  it('includes multiple edits made during the build in the final-text payload', async () => {
    const fixture = createColdBuildAttributionFixture();
    const gate = installColdBuildIdleGate();
    getFileHandle.mockReturnValue(fixture.handle);

    const { result, rerender } = renderAttribution({
      enabled: true,
      filePath: 'doc.qmd',
      sourceText: fixture.text,
    });
    expect(gate.pending).toBe(1);
    expect(result.current).toEqual({ payload: null, generating: true });

    fixture.append('X', 2);
    fixture.append('Y', 3);
    rerender({ enabled: true, filePath: 'doc.qmd', sourceText: fixture.text });

    await act(async () => {
      gate.runNext();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(result.current.generating).toBe(false);
    expect(payloadRuns(result.current.payload)).toEqual([
      { start: 0, end: COLD_BUILD_TEXT_LENGTH, actor: COLD_BUILD_ACTOR, time: 1 },
      { start: COLD_BUILD_TEXT_LENGTH, end: COLD_BUILD_TEXT_LENGTH + 1, actor: COLD_BUILD_ACTOR, time: 2 },
      { start: COLD_BUILD_TEXT_LENGTH + 1, end: COLD_BUILD_TEXT_LENGTH + 2, actor: COLD_BUILD_ACTOR, time: 3 },
    ]);
  });

  it('ignores a late completion after attribution is disabled', async () => {
    const fixture = createColdBuildAttributionFixture();
    const gate = installColdBuildIdleGate();
    getFileHandle.mockReturnValue(fixture.handle);

    const { result, rerender } = renderAttribution({
      enabled: true,
      filePath: 'doc.qmd',
      sourceText: fixture.text,
    });
    fixture.append('X', 2);
    rerender({ enabled: false, filePath: 'doc.qmd', sourceText: fixture.text });

    await act(async () => {
      gate.runNext();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(result.current).toEqual({ payload: null, generating: false });
  });

  it('ignores an old file late completion and catches up the new file build', async () => {
    const first = createColdBuildAttributionFixture();
    const second = createColdBuildAttributionFixture();
    const gate = installColdBuildIdleGate();
    getFileHandle.mockImplementation((path: string) => {
      if (path === 'first.qmd') return first.handle;
      if (path === 'second.qmd') return second.handle;
      return undefined;
    });

    const { result, rerender } = renderAttribution({
      enabled: true,
      filePath: 'first.qmd',
      sourceText: first.text,
    });
    first.append('old', 2);
    rerender({ enabled: true, filePath: 'second.qmd', sourceText: second.text });
    expect(gate.pending).toBe(2);

    // Complete the aborted first build while the second file is still paused.
    await act(async () => {
      gate.runNext();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(result.current).toEqual({ payload: null, generating: true });

    second.append('X', 2);
    second.append('Y', 3);
    rerender({ enabled: true, filePath: 'second.qmd', sourceText: second.text });
    await act(async () => {
      gate.runNext();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(result.current.generating).toBe(false);
    expect(payloadRuns(result.current.payload)).toEqual([
      { start: 0, end: COLD_BUILD_TEXT_LENGTH, actor: COLD_BUILD_ACTOR, time: 1 },
      { start: COLD_BUILD_TEXT_LENGTH, end: COLD_BUILD_TEXT_LENGTH + 1, actor: COLD_BUILD_ACTOR, time: 2 },
      { start: COLD_BUILD_TEXT_LENGTH + 1, end: COLD_BUILD_TEXT_LENGTH + 2, actor: COLD_BUILD_ACTOR, time: 3 },
    ]);
  });
});
