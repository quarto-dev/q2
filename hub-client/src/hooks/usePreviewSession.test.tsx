/**
 * Unit tests for usePreviewSession. The hook is a thin boot-time
 * wrapper over fetchPreviewSessionConfig: null until the fetch
 * resolves, and null forever when the serving server is not a
 * `q2 preview` session (callers gate on an explicit value, so null is
 * always the banner-free case).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';

vi.mock('../services/previewConfig', () => ({
  fetchPreviewSessionConfig: vi.fn(),
}));

import { usePreviewSession } from './usePreviewSession';
import { fetchPreviewSessionConfig } from '../services/previewConfig';

const mockFetchConfig = vi.mocked(fetchPreviewSessionConfig);

describe('usePreviewSession', () => {
  beforeEach(() => {
    mockFetchConfig.mockReset();
  });

  it('starts null and resolves to the session config', async () => {
    mockFetchConfig.mockResolvedValue({ allowEdit: false });

    const { result } = renderHook(() => usePreviewSession());
    expect(result.current).toBeNull();

    await waitFor(() => expect(result.current).toEqual({ allowEdit: false }));
  });

  it('stays null when the server is not a preview session', async () => {
    mockFetchConfig.mockResolvedValue(null);

    const { result } = renderHook(() => usePreviewSession());
    await waitFor(() => expect(mockFetchConfig).toHaveBeenCalled());
    // Flush the resolved promise's .then callback.
    await act(async () => {});

    expect(result.current).toBeNull();
  });

  it('fetches once at boot and does not refetch on rerender', async () => {
    mockFetchConfig.mockResolvedValue({ allowEdit: true });

    const { rerender } = renderHook(() => usePreviewSession());
    await waitFor(() => expect(mockFetchConfig).toHaveBeenCalledTimes(1));

    rerender();
    expect(mockFetchConfig).toHaveBeenCalledTimes(1);
  });
});
