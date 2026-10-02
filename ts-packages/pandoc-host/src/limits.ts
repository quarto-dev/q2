/**
 * Mount limits, from resources/pandoc-wasm.json (`limits`). The values are
 * duplicated here because the package's `rootDir ./src` rules out importing the
 * repo-root JSON; `constants.test.ts` fails if they drift from the file.
 * Callers (hub-client) may pass their own through `ExecuteOptions.limits`.
 */
export interface Limits {
  /** Each image (a resource_refs entry with an image extension). */
  image_bytes: number;
  /** The `--reference-doc` file. */
  reference_doc_bytes: number;
  /** Every mounted byte: share tree + files + resource_refs. */
  total_bytes: number;
}

export const DEFAULT_LIMITS: Limits = {
  image_bytes: 26214400,
  reference_doc_bytes: 52428800,
  total_bytes: 314572800,
};

/** The reserved share root (resources/pandoc-wasm.json `share_root`). */
export const DEFAULT_SHARE_ROOT = '/__q2_share__';

/** The only request schema version this host understands. */
export const SUPPORTED_SCHEMA_VERSION = 1;
