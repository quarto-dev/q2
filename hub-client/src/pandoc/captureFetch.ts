/**
 * Capture bytes for a whole-book download (pandoc-host H5, R9): every chapter's recorded
 * engine capture, fetched at click time from the hub's binary docs.
 */
import { getBinaryDocById, type CaptureRef } from '@quarto/preview-runtime';

/**
 * The chapters that have a capture, as chapter path (a sidecar key) to capture doc id.
 * Chapters absent from the sidecar are simply left out; Rust renders them as source.
 */
export function captureDocIdsFor(chapters: readonly string[], captures: Record<string, CaptureRef> | undefined): Record<string, string> {
  const out: Record<string, string> = {};
  for (const path of chapters) {
    const id = captures?.[path]?.captureDocId;
    if (id) out[path] = id;
  }
  return out;
}

/**
 * Fetch each chapter's capture bytes in parallel. A chapter whose fetch fails (a throw, a
 * missing doc, no content) is reported in `failed` and the rest still arrive. `getBinaryDocById`
 * takes no signal (the sync client bounds each attempt with its own deadline), so an abort
 * cannot cancel the fetches; it settles this call at once with the signal's reason and the
 * late results are dropped.
 */
export async function fetchChapterCaptures(
  docIds: Record<string, string>,
  signal: AbortSignal,
  fetchDoc: typeof getBinaryDocById = getBinaryDocById,
): Promise<{ byPath: Record<string, Uint8Array>; failed: string[] }> {
  signal.throwIfAborted();
  const aborted = new Promise<never>((_, reject) => {
    signal.addEventListener('abort', () => reject(signal.reason), { once: true });
  });
  const settled = Promise.all(
    Object.entries(docIds).map(async ([path, id]) => {
      try {
        const doc = await fetchDoc(id);
        return { path, bytes: doc?.content };
      } catch {
        return { path, bytes: undefined };
      }
    }),
  );
  const results = await Promise.race([settled, aborted]);
  const byPath: Record<string, Uint8Array> = {};
  const failed: string[] = [];
  for (const { path, bytes } of results) {
    if (bytes) byPath[path] = bytes;
    else failed.push(path);
  }
  return { byPath, failed };
}
