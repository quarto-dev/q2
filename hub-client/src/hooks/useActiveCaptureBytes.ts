import { useEffect, useState } from 'react';
import { getBinaryDocById, type CaptureRef } from '@quarto/preview-runtime';

/**
 * Fetch the gzipped `EngineCapture[]` bytes for the active document's recorded
 * capture (bd-sfet3264 / bd-uy4uygha).
 *
 * Given the project's `captures` sidecar and the active file path, resolves the
 * active document's `captureDocId` and fetches the capture binary doc's bytes
 * via `getBinaryDocById`. Returns `undefined` when there's no capture (so the
 * caller renders code cells as source). Keyed on the `captureDocId` (not
 * content), so it only re-fetches when a fresh capture arrives — not on every
 * keystroke — and a dangling/unreachable capture falls back to `undefined`.
 *
 * Shared by the `q2-preview` renderer (`ReactPreview`) and the default
 * `format: html` renderer (`Preview`), which both splice captures into their
 * respective render entries.
 */
export function useActiveCaptureBytes(
  captures: Record<string, CaptureRef> | undefined,
  path: string | undefined,
): Uint8Array | undefined {
  const activeCaptureDocId = path ? captures?.[path]?.captureDocId : undefined;
  const [captureBytes, setCaptureBytes] = useState<Uint8Array | undefined>(undefined);

  useEffect(() => {
    let cancelled = false;
    if (!activeCaptureDocId) {
      setCaptureBytes(undefined);
      return;
    }
    (async () => {
      try {
        const doc = await getBinaryDocById(activeCaptureDocId);
        if (!cancelled) setCaptureBytes(doc?.content);
      } catch {
        if (!cancelled) setCaptureBytes(undefined);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeCaptureDocId]);

  return captureBytes;
}
