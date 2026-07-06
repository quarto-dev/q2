/**
 * Tests for useActiveCaptureBytes (bd-uy4uygha).
 *
 * The shared capture-fetch hook used by both the q2-preview renderer
 * (`ReactPreview`) and the default `format: html` renderer (`Preview`): given
 * the project's capture sidecar + the active path, it resolves the active
 * document's `captureDocId` and fetches the capture binary doc's bytes.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';

const CAPTURE_BYTES = new Uint8Array([9, 8, 7, 6]);

const { getBinaryDocById } = vi.hoisted(() => ({
  getBinaryDocById: vi.fn(async () => ({
    content: new Uint8Array([9, 8, 7, 6]),
    mimeType: 'application/x-engine-capture+gzip',
  })),
}));

vi.mock('@quarto/preview-runtime', () => ({ getBinaryDocById }));

import { useActiveCaptureBytes } from './useActiveCaptureBytes';
import type { CaptureRef } from '@quarto/preview-runtime';

describe('useActiveCaptureBytes (bd-uy4uygha)', () => {
  beforeEach(() => {
    getBinaryDocById.mockClear();
  });

  it('fetches the active document capture bytes by captureDocId', async () => {
    const captures: Record<string, CaptureRef> = {
      'doc.qmd': { captureDocId: 'cap-1', state: 'idle' },
      'other.qmd': { captureDocId: 'cap-other', state: 'idle' },
    };
    const { result } = renderHook(() => useActiveCaptureBytes(captures, 'doc.qmd'));

    await waitFor(() => expect(getBinaryDocById).toHaveBeenCalledWith('cap-1'));
    await waitFor(() => expect(result.current).toEqual(CAPTURE_BYTES));
  });

  it('returns undefined and does not fetch when the active path has no capture', async () => {
    const captures: Record<string, CaptureRef> = { 'other.qmd': { captureDocId: 'cap-x' } };
    const { result } = renderHook(() => useActiveCaptureBytes(captures, 'doc.qmd'));

    expect(result.current).toBeUndefined();
    expect(getBinaryDocById).not.toHaveBeenCalled();
  });

  it('returns undefined when the path is undefined', () => {
    const captures: Record<string, CaptureRef> = { 'doc.qmd': { captureDocId: 'cap-1' } };
    const { result } = renderHook(() => useActiveCaptureBytes(captures, undefined));

    expect(result.current).toBeUndefined();
    expect(getBinaryDocById).not.toHaveBeenCalled();
  });
});
