/**
 * The media half of an import (document import P4 T3; epic I8, I16): what to do with each file pandoc
 * extracted, and the `media_manifest_json` that `finish_import` plans the stored names from (interface 2).
 *
 * Image bytes stay here, in TS (I9). Rust sees only each file's hash, extension, and whether it was
 * converted or dropped.
 */
import type { HostDiagnostic, RequestFile } from '@quarto/pandoc-host';

export type ManifestEntry =
  | { pandoc_path: string; status: 'stored'; sha256: string; ext: string; converted_from?: 'emf' | 'wmf'; conversion_failed?: true }
  | { pandoc_path: string; status: 'skipped'; reason: 'too-large'; size: number };

/** The bytes to store for one `stored` manifest entry. */
export interface StoredMedia {
  bytes: Uint8Array;
  ext: string;
}

export interface MediaDeps {
  /** EMF/WMF bytes to SVG bytes; rejects on failure. */
  convertImage: (bytes: Uint8Array, format: 'emf' | 'wmf') => Promise<Uint8Array>;
  sha256: (bytes: Uint8Array) => Promise<string>;
  now: () => number;
  /** The size limit of a stored image (`FILE_SIZE_LIMITS.MAX_FILE_SIZE`). */
  maxImageBytes: number;
  /** Per-image conversion timeout. */
  convertTimeoutMs: number;
  /** Total conversion time for one import; images after it are not converted. */
  convertBudgetMs: number;
}

export const DEFAULT_CONVERT_TIMEOUT_MS = 10_000;
export const DEFAULT_CONVERT_BUDGET_MS = 60_000;

export type MediaResult =
  | { cancelled: false; manifest: ManifestEntry[]; stored: Map<string, StoredMedia> }
  | { cancelled: true };

const extOf = (path: string): string => {
  const dot = path.lastIndexOf('.');
  return dot < 0 || path.lastIndexOf('/') > dot ? '' : path.slice(dot + 1).toLowerCase();
};

class ConvertTimeout extends Error {}

/** Race `p` against a timer. A converter that never settles is left dangling: wasm and the DOM cannot be interrupted. */
function withTimeout<T>(p: Promise<T>, ms: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new ConvertTimeout(`conversion took longer than ${Math.round(ms / 1000)} s`)), ms);
  });
  p.catch(() => undefined);
  return Promise.race([p, timeout]).finally(() => clearTimeout(timer));
}

/**
 * For each collected file, in order:
 * 1. an EMF or WMF is converted to SVG (10 s per image, 60 s in all). On error, timeout, an exhausted
 *    budget, or an SVG over the size limit, the original is kept and flagged `conversion_failed` (I8);
 * 2. final bytes over the size limit make the entry `skipped` (`too-large`), and the bytes are dropped (I16);
 * 3. otherwise the entry is `stored`, with the SHA-256 of the final bytes.
 *
 * Each host `collect-limit` warning then adds a `skipped` entry for a file the host dropped (no bytes).
 * Checks `signal` before each image; if it is aborted, stops with `{ cancelled: true }`.
 */
export async function buildMedia(collected: RequestFile[], hostWarnings: HostDiagnostic[], deps: MediaDeps, signal?: AbortSignal): Promise<MediaResult> {
  const manifest: ManifestEntry[] = [];
  const stored = new Map<string, StoredMedia>();
  const startedAt = deps.now();

  for (const file of collected) {
    if (signal?.aborted) return { cancelled: true };
    const original = extOf(file.path);
    let bytes = file.bytes;
    let ext = original;
    let convertedFrom: 'emf' | 'wmf' | undefined;
    let conversionFailed = false;

    if (original === 'emf' || original === 'wmf') {
      if (deps.now() - startedAt >= deps.convertBudgetMs) {
        conversionFailed = true;
      } else {
        try {
          const svg = await withTimeout(deps.convertImage(file.bytes, original), deps.convertTimeoutMs);
          // An SVG over the limit counts as a failed conversion (a bitmap-wrapping EMF becomes ~1.33x base64): the
          // original is kept, and skipped below if it too is over.
          if (svg.byteLength > deps.maxImageBytes) conversionFailed = true;
          else {
            bytes = svg;
            ext = 'svg';
            convertedFrom = original;
          }
        } catch {
          conversionFailed = true;
        }
        if (signal?.aborted) return { cancelled: true };
      }
    }

    if (bytes.byteLength > deps.maxImageBytes) {
      manifest.push({ pandoc_path: file.path, status: 'skipped', reason: 'too-large', size: bytes.byteLength });
      continue;
    }
    manifest.push({
      pandoc_path: file.path,
      status: 'stored',
      sha256: await deps.sha256(bytes),
      ext,
      ...(convertedFrom ? { converted_from: convertedFrom } : {}),
      ...(conversionFailed ? { conversion_failed: true as const } : {}),
    });
    stored.set(file.path, { bytes, ext });
  }

  // Files the host dropped for its own size limits (I16): no bytes, but the link must still point where the image would be.
  for (const w of hostWarnings) {
    if (w.code === 'collect-limit' && w.path !== undefined) manifest.push({ pandoc_path: w.path, status: 'skipped', reason: 'too-large', size: w.size ?? 0 });
  }
  return { cancelled: false, manifest, stored };
}
