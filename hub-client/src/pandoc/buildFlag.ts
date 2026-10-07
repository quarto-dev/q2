// Build-time decision for the pandoc.wasm "Download as" feature (design D6/D7, H4).
// Pure so vite.config.ts and the unit test share it; vite.config.ts does the file-system
// check and passes the answer in.

export interface PandocFlagInput {
  /** `process.env` of the build. */
  env: Record<string, string | undefined>;
  /** Whether `public/pandoc/pandoc.wasm.gz` exists at config time. */
  assetExists: boolean;
}

export interface PandocFlag {
  enabled: boolean;
  /** Why the feature is off, for the build log; absent when on, or when off by choice. */
  warning?: string;
}

/**
 * The feature is on when the asset is present and `VITE_PANDOC_WASM` is not `0`.
 * `VITE_PANDOC_WASM=0` is the kill switch (the embed build sets it: D7, the embed carries
 * no wasm). A missing asset turns the feature off with a warning rather than failing, so a
 * checkout that never ran `scripts/fetch-pandoc-wasm.mjs` still builds; the release entry
 * points run it with `--require` first, so a release build cannot reach here without it.
 */
export function resolvePandocFlag({ env, assetExists }: PandocFlagInput): PandocFlag {
  if (env.VITE_PANDOC_WASM === '0') return { enabled: false };
  if (!assetExists) {
    return {
      enabled: false,
      warning:
        'public/pandoc/pandoc.wasm.gz is missing: "Download as" is disabled in this build. ' +
        'Run `node scripts/fetch-pandoc-wasm.mjs` (see dev-docs/pandoc-wasm-host.md).',
    };
  }
  return { enabled: true };
}
