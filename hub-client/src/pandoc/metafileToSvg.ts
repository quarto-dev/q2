/**
 * EMF/WMF to SVG for document import (epic I8 as revised by the metafile-svg plan; I15), with rtf.js's
 * EMFJS and WMFJS renderers. The SVG is stored in the project as `<hash>.svg`; a docx/pptx export
 * rasterizes it then (`RasterizeSvgImagesStage`), because pandoc.wasm has no `rsvg-convert`.
 *
 * Main thread only. `Renderer.render()` builds an `SVGElement` with `document.createElementNS`, so
 * neither a worker nor the node test suites can run the conversion (`metafileSize` and
 * `normalizeSvgIds` are pure and tested in node).
 *
 * `render()` is synchronous and cannot be interrupted: the service's per-image timeout bounds
 * nothing here, so a render that hangs on a malformed file blocks the tab.
 */

export type MetafileFormat = 'emf' | 'wmf';

/** CSS pixels per inch: the SVG's `width`/`height` are the metafile's size at this. */
const DPI = 96;

/** Everything the two renderers need, parsed from the metafile header. */
export interface MetafileSize {
  /** The SVG's `width`/`height` in CSS pixels (the metafile's size at 96 dpi). */
  width: number;
  height: number;
  /** The renderer's logical extent (`xExt`/`yExt`; the EMF window extent is the same). */
  extX: number;
  extY: number;
}

/**
 * `Renderer` takes `{ width, height, wExt, hExt, xExt, yExt, mapMode }` and exposes no frame size
 * (`src/emfjs/Renderer.ts:34-42`), so the size comes from the header: the EMF's `rclBounds` (device
 * units) and `rclFrame` (0.01 mm), or the WMF's placeable header (bounding box and units per inch).
 * Throws when there is no usable size (a WMF without a placeable header, a degenerate frame).
 */
export function metafileSize(bytes: Uint8Array, format: MetafileFormat): MetafileSize {
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let extX: number;
  let extY: number;
  let inchesX: number;
  let inchesY: number;
  if (format === 'emf') {
    if (bytes.byteLength < 40 || v.getUint32(0, true) !== 1) throw new Error('not an EMF file');
    const [bl, bt, br, bb] = [8, 12, 16, 20].map((o) => v.getInt32(o, true));
    const [fl, ft, fr, fb] = [24, 28, 32, 36].map((o) => v.getInt32(o, true));
    const frameX = fr - fl;
    const frameY = fb - ft;
    if (frameX <= 0 || frameY <= 0) throw new Error('EMF frame is empty');
    inchesX = frameX / 2540;
    inchesY = frameY / 2540;
    // rclBounds is inclusive; fall back to the frame's own units when the file leaves it empty.
    extX = br - bl + 1;
    extY = bb - bt + 1;
    if (extX <= 0 || extY <= 0) [extX, extY] = [frameX, frameY];
  } else {
    if (bytes.byteLength < 22 || v.getUint32(0, true) !== 0x9ac6cdd7) throw new Error('WMF has no placeable header, so its size is unknown');
    const [l, t, r, b] = [6, 8, 10, 12].map((o) => v.getInt16(o, true));
    const perInch = v.getUint16(14, true);
    extX = r - l;
    extY = b - t;
    if (perInch === 0 || extX <= 0 || extY <= 0) throw new Error('WMF header is empty');
    inchesX = extX / perInch;
    inchesY = extY / perInch;
  }
  return { width: Math.max(1, Math.round(inchesX * DPI)), height: Math.max(1, Math.round(inchesY * DPI)), extX, extY };
}

interface MetafileRenderer {
  render(info: { width: string; height: string; wExt?: number; hExt?: number; xExt: number; yExt: number; mapMode: number }): SVGElement;
}
interface MetafileModule {
  Renderer: new (blob: ArrayBuffer) => MetafileRenderer;
}

/** The bundles are UMD wrappers; Vite's interop gives either the namespace or its `default`, so accept both. */
const unwrap = (m: unknown): MetafileModule => ((m as { default?: MetafileModule }).default ?? (m as MetafileModule));

/**
 * Lazy: each bundle (about 56 KB) is its own chunk, and importing the package root would pull in the
 * 2.1 MB RTF bundle.
 */
async function loadRenderer(format: MetafileFormat): Promise<MetafileModule> {
  return unwrap(format === 'emf' ? await import('rtf.js/dist/EMFJS.bundle.min.js') : await import('rtf.js/dist/WMFJS.bundle.min.js'));
}

/** MM_ISOTROPIC, which is what rtf.js itself uses for pictures. */
const MM_ISOTROPIC = 8;

/**
 * rtf.js names clip paths and patterns from a counter that lives as long as the page
 * (`EMFJS_c0`, `wmfjs_p3`: `src/emfjs/Helper.ts:324,333`, `src/wmfjs/Helper.ts:279-280`), so the same file
 * converted twice differs. Renumber by order of first appearance, so equal drawings give equal bytes and
 * hash to one stored name (I12, I20).
 */
export function normalizeSvgIds(svg: string): string {
  const seen = new Map<string, string>();
  return svg.replace(/\b((?:EMFJS|wmfjs)_[a-z])(\d+)\b/g, (whole, prefix: string) => {
    let renamed = seen.get(whole);
    if (renamed === undefined) {
      renamed = `${prefix}${seen.size}`;
      seen.set(whole, renamed);
    }
    return renamed;
  });
}

export interface MetafileSvg {
  /** The serialized SVG, UTF-8. */
  svg: Uint8Array;
  /** Its `width` x `height` in CSS pixels (the metafile's size at 96 dpi). */
  width: number;
  height: number;
}

/** Convert an EMF or WMF to SVG at 1x. Rejects when the file cannot be rendered. */
export async function convertMetafileToSvg(bytes: Uint8Array, format: MetafileFormat): Promise<MetafileSvg> {
  const size = metafileSize(bytes, format);
  const { Renderer } = await loadRenderer(format);
  // `Renderer` wants an ArrayBuffer of exactly the file (the bytes may be a view into a larger one).
  const svg = new Renderer(bytes.slice().buffer).render({
    width: `${size.width}px`,
    height: `${size.height}px`,
    ...(format === 'emf' ? { wExt: size.extX, hExt: size.extY } : {}),
    xExt: size.extX,
    yExt: size.extY,
    mapMode: MM_ISOTROPIC,
  });
  const text = normalizeSvgIds(new XMLSerializer().serializeToString(svg));
  return { svg: new TextEncoder().encode(text), width: size.width, height: size.height };
}
