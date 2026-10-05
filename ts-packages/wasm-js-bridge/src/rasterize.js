/**
 * WASM-JS Bridge for SVG rasterization
 *
 * pandoc.wasm has no `rsvg-convert`, so its docx and pptx writers drop an SVG
 * image to alt text. `RasterizeSvgImagesStage` (quarto-core) calls this to
 * turn each referenced SVG into a PNG before pandoc runs.
 *
 * Imported by quarto-system-runtime/src/wasm.rs using:
 *
 *   #[wasm_bindgen(raw_module = "/src/wasm-js-bridge/rasterize.js")]
 *
 * Main thread only: it needs `<img>` and `<canvas>` (`OffscreenCanvas` cannot
 * decode SVG in a worker). Where there is no `document` (node tests, workers)
 * `jsCanRasterizeSvg()` is false and `jsRasterizeSvg` rejects with an error
 * named `RasterizerUnavailable`.
 *
 * Sizing. A browser's `naturalWidth x naturalHeight` for a viewBox-only SVG is
 * engine-dependent (300x150 in Chromium and Firefox, 100x50 in WebKit), so the
 * intrinsic size is parsed from the SVG root here. The PNG is rendered at
 * twice that size (longest side capped at `maxSide`) and carries a 192 dpi
 * pHYs chunk, so pandoc displays it at the SVG's intrinsic size. Without pHYs
 * pandoc assumes a different dpi and the image comes out ~2.7x too large.
 */

/** Longest allowed SVG input, in bytes (the import/storage image limit, I16). */
export const MAX_INPUT_BYTES = 10 * 1024 * 1024;
/** Per-image time limit, in milliseconds (the import per-image timeout). */
export const TIMEOUT_MS = 10_000;
/** Render scale relative to the SVG's intrinsic size. */
const SCALE = 2;
/**
 * CSS pixels per inch, and the PNG's pixels per metre at SCALE x 96 dpi. Rounded *up*:
 * pandoc truncates the dpi it derives (7559 px/m is 191.9986, which it reads as 191 and
 * shows the image 0.5% too large), so 7560 is what makes it read 192.
 */
const CSS_DPI = 96;
const PIXELS_PER_METRE = Math.ceil((CSS_DPI * SCALE) / 0.0254);
/** The size a browser gives an SVG with neither a size nor a viewBox. */
const DEFAULT_SIZE = { width: 300, height: 150 };

const UNIT_PX = { "": 1, px: 1, in: CSS_DPI, cm: CSS_DPI / 2.54, mm: CSS_DPI / 25.4, pt: CSS_DPI / 72, pc: CSS_DPI / 6 };

/** @type {((svg: Uint8Array, maxSide: number) => Promise<Uint8Array>) | null} */
let rasterizerOverride = null;

/**
 * Replace the rasterizer, for tests that have no DOM (cf. `setVfsCallbacks` in
 * sass.js). `null` restores the DOM implementation.
 *
 * @param {((svg: Uint8Array, maxSide: number) => Promise<Uint8Array>) | null} fn
 */
export function setRasterizer(fn) {
  rasterizerOverride = fn;
}

function unavailable() {
  const e = new Error("SVG rasterization needs a DOM (main thread only)");
  e.name = "RasterizerUnavailable";
  return e;
}

const hasDom = () => typeof document !== "undefined" && typeof document.createElement === "function";

/**
 * Whether `jsRasterizeSvg` can run here. Synchronous, so the Rust stage can skip
 * its AST walk on native and in node.
 *
 * @returns {boolean}
 */
export function jsCanRasterizeSvg() {
  return rasterizerOverride !== null || hasDom();
}

/**
 * A root `width`/`height` as CSS pixels, or null for a missing, percentage or unparsable value.
 *
 * @param {string | null} value
 * @returns {number | null}
 */
function lengthToPx(value) {
  if (value == null) return null;
  const m = /^\s*([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)\s*([a-zA-Z]*)\s*$/.exec(value);
  if (!m) return null;
  const factor = UNIT_PX[m[2].toLowerCase()];
  const n = Number(m[1]) * (factor ?? NaN);
  return Number.isFinite(n) && n > 0 ? n : null;
}

/**
 * The intrinsic size of an SVG root: `width`/`height` with units, else the
 * viewBox, else 300x150. When only one of width/height is given the other
 * follows the viewBox's aspect ratio (or is the default's).
 *
 * @param {Element} root
 * @returns {{ width: number, height: number, viewBox: boolean }}
 */
export function intrinsicSize(root) {
  const vb = (root.getAttribute("viewBox") ?? "").trim().split(/[\s,]+/).map(Number);
  const hasViewBox = vb.length === 4 && vb.every(Number.isFinite) && vb[2] > 0 && vb[3] > 0;
  const aspect = hasViewBox ? vb[2] / vb[3] : DEFAULT_SIZE.width / DEFAULT_SIZE.height;
  const w = lengthToPx(root.getAttribute("width"));
  const h = lengthToPx(root.getAttribute("height"));
  let width;
  let height;
  if (w && h) [width, height] = [w, h];
  else if (w) [width, height] = [w, w / aspect];
  else if (h) [width, height] = [h * aspect, h];
  else if (hasViewBox) [width, height] = [vb[2], vb[3]];
  else [width, height] = [DEFAULT_SIZE.width, DEFAULT_SIZE.height];
  return { width, height, viewBox: hasViewBox };
}

/**
 * The PNG's pixel size: SCALE x intrinsic, longest side capped at `maxSide`, aspect kept.
 *
 * @param {{ width: number, height: number }} size
 * @param {number} maxSide
 * @returns {{ width: number, height: number }}
 */
export function targetSize(size, maxSide) {
  let width = size.width * SCALE;
  let height = size.height * SCALE;
  const longest = Math.max(width, height);
  if (longest > maxSide) {
    width = (width * maxSide) / longest;
    height = (height * maxSide) / longest;
  }
  return { width: Math.max(1, Math.round(width)), height: Math.max(1, Math.round(height)) };
}

let crcTable = null;
function crc32(bytes) {
  if (!crcTable) {
    crcTable = new Uint32Array(256);
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      crcTable[n] = c >>> 0;
    }
  }
  let crc = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) crc = crcTable[(crc ^ bytes[i]) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

/**
 * Insert a pHYs chunk (192 dpi) right after IHDR, unless the PNG already has one.
 *
 * @param {Uint8Array} png
 * @returns {Uint8Array}
 */
export function withPhys(png) {
  const IHDR_END = 8 + 25; // signature + (length, type, 13 data bytes, crc)
  const v = new DataView(png.buffer, png.byteOffset, png.byteLength);
  if (png.byteLength < IHDR_END || v.getUint32(0) !== 0x89504e47) throw new Error("the canvas produced no PNG");
  for (let o = 8; o + 8 <= png.byteLength; ) {
    const len = v.getUint32(o);
    const type = String.fromCharCode(png[o + 4], png[o + 5], png[o + 6], png[o + 7]);
    if (type === "pHYs") return png;
    if (type === "IDAT") break;
    o += 12 + len;
  }
  const chunk = new Uint8Array(21);
  const cv = new DataView(chunk.buffer);
  cv.setUint32(0, 9);
  chunk.set([0x70, 0x48, 0x59, 0x73], 4); // "pHYs"
  cv.setUint32(8, PIXELS_PER_METRE);
  cv.setUint32(12, PIXELS_PER_METRE);
  chunk[16] = 1; // unit: metre
  cv.setUint32(17, crc32(chunk.subarray(4, 17)));
  const out = new Uint8Array(png.byteLength + chunk.byteLength);
  out.set(png.subarray(0, IHDR_END), 0);
  out.set(chunk, IHDR_END);
  out.set(png.subarray(IHDR_END), IHDR_END + chunk.byteLength);
  return out;
}

const loadImage = (src) =>
  new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("the browser could not decode the SVG"));
    img.src = src;
  });

async function rasterizeWithDom(svg, maxSide) {
  const doc = new DOMParser().parseFromString(new TextDecoder().decode(svg), "image/svg+xml");
  const root = doc.documentElement;
  if (!root || root.localName !== "svg" || doc.getElementsByTagName("parsererror").length > 0) {
    throw new Error("not a well-formed SVG document");
  }
  const intrinsic = intrinsicSize(root);
  const { width, height } = targetSize(intrinsic, maxSide);
  // Make the root's size explicit (the browser then agrees with us whatever its
  // default for a viewBox-only SVG), and give a viewBox-less SVG one so it scales.
  if (!intrinsic.viewBox) root.setAttribute("viewBox", `0 0 ${intrinsic.width} ${intrinsic.height}`);
  root.setAttribute("width", String(width));
  root.setAttribute("height", String(height));
  const text = new XMLSerializer().serializeToString(root);

  const url = URL.createObjectURL(new Blob([text], { type: "image/svg+xml" }));
  try {
    const img = await loadImage(url);
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("no 2d canvas context");
    ctx.drawImage(img, 0, 0, width, height);
    // A tainted canvas (e.g. an SVG with <foreignObject>) throws a SecurityError here.
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
    if (!blob) throw new Error("the canvas produced no PNG");
    return withPhys(new Uint8Array(await blob.arrayBuffer()));
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * Rasterize an SVG to a PNG.
 *
 * `<img>` decoding cannot be cancelled, so a timeout or abort rejects the
 * caller but leaves the decode running; `MAX_INPUT_BYTES` bounds how long that is.
 *
 * @param {Uint8Array} svg
 * @param {number} maxSide - cap on the PNG's longest side, in pixels
 * @param {AbortSignal | undefined} [signal] - the click's abort signal
 * @returns {Promise<Uint8Array>}
 * @throws {Error} named `RasterizerUnavailable` with no DOM; otherwise on an input over
 *   `MAX_INPUT_BYTES`, an undecodable SVG, a tainted canvas, a timeout or an abort
 */
export async function jsRasterizeSvg(svg, maxSide, signal) {
  if (rasterizerOverride === null && !hasDom()) throw unavailable();
  if (svg.byteLength > MAX_INPUT_BYTES) {
    throw new Error(`the SVG is ${svg.byteLength} bytes; the limit is ${MAX_INPUT_BYTES}`);
  }
  if (signal?.aborted) throw signal.reason ?? new Error("rasterization aborted");

  let timer;
  let onAbort;
  const guards = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`rasterizing took longer than ${TIMEOUT_MS / 1000} s`)), TIMEOUT_MS);
    if (signal) {
      onAbort = () => reject(signal.reason ?? new Error("rasterization aborted"));
      signal.addEventListener("abort", onAbort, { once: true });
    }
  });
  try {
    const work = rasterizerOverride ? rasterizerOverride(svg, maxSide) : rasterizeWithDom(svg, maxSide);
    return await Promise.race([work, guards]);
  } finally {
    clearTimeout(timer);
    if (signal && onAbort) signal.removeEventListener("abort", onAbort);
  }
}
