/**
 * Loading the typst compiler's two assets (host phase H7): the typst.ts wasm and the default
 * fonts. Both are served from `public/typst/` as opaque gzip (like pandoc's, design D6) and go
 * through the same verified, Cache-API-backed loader as pandoc.wasm; only the label, the cache
 * names and the exnref requirement differ (typst.ts needs no exception-handling proposal).
 */
import { PandocLoader, type LoaderConfig, type LoadOptions, type LoadProgress } from '../pandoc/pandocLoader';
import { unpackFonts } from '@quarto/typst-host/fontBundle';
import constants from '../../../resources/typst-wasm.json';

export const TYPST_WASM_SHA256: string = constants.wasm_sha256;
export const TYPST_FONTS_SHA256: string = constants.fonts_bundle_sha256;
export const TYPST_VERSION: string = constants.typst_version;

export const TYPST_WASM_PATH = 'typst/typst.wasm.gz';
export const TYPST_FONTS_PATH = 'typst/fonts.bin.gz';

export function createTypstLoader(config: LoaderConfig = {}): PandocLoader {
  return new PandocLoader({
    assetPath: TYPST_WASM_PATH,
    cacheName: 'q2-typst-wasm-v1',
    label: 'the Typst compiler',
    requireExnref: false,
    ...config,
  });
}

/** A font bundle starts with a plausible font count and a first length that fits (not HTML, not gzip). */
export function looksLikeFontBundle(b: Uint8Array): boolean {
  if (b.length < 8) return false;
  const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
  const count = v.getUint32(0, true);
  return count > 0 && count < 1024 && v.getUint32(4, true) <= b.length - 8;
}

/**
 * The default fonts, as raw bytes. The loader's compile step is the identity here: the
 * "module" it keeps resident is the verified, decompressed font bundle.
 */
export class TypstFontsLoader {
  private readonly loader: PandocLoader;

  constructor(config: LoaderConfig = {}) {
    this.loader = new PandocLoader({
      assetPath: TYPST_FONTS_PATH,
      cacheName: 'q2-typst-fonts-v1',
      label: 'the Typst default fonts',
      requireExnref: false,
      isRaw: looksLikeFontBundle,
      ...config,
      env: { ...config.env, compile: async (bytes) => bytes as unknown as WebAssembly.Module },
    });
  }

  async load(options?: LoadOptions): Promise<{ fonts: Uint8Array[]; notices: string[] }> {
    const r = await this.loader.load(TYPST_FONTS_SHA256, options);
    return { fonts: unpackFonts(r.module as unknown as Uint8Array), notices: r.notices };
  }

  hold(): () => void {
    return this.loader.hold();
  }

  dropResident(): void {
    this.loader.dropResident();
  }

  get hasResident(): boolean {
    return this.loader.hasResidentModule;
  }
}

export type { LoadProgress };
