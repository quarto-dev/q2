import { describe, it, expect, afterEach, vi } from "vitest";
import {
  MAX_INPUT_BYTES,
  intrinsicSize,
  jsCanRasterizeSvg,
  jsRasterizeSvg,
  setRasterizer,
  targetSize,
  withPhys,
} from "./rasterize.js";

/** The only part of an `Element` that `intrinsicSize` reads. */
const root = (attrs: Record<string, string>) =>
  ({ getAttribute: (n: string) => attrs[n] ?? null }) as unknown as Element;

describe("intrinsicSize", () => {
  it("reads width and height with units as CSS pixels", () => {
    expect(intrinsicSize(root({ width: "2in", height: "1in" }))).toEqual({ width: 192, height: 96, viewBox: false });
    expect(intrinsicSize(root({ width: "72pt", height: "10" }))).toEqual({ width: 96, height: 10, viewBox: false });
  });

  it("falls back to the viewBox, not to the browser's engine-specific default", () => {
    expect(intrinsicSize(root({ viewBox: "0 0 100 50" }))).toEqual({ width: 100, height: 50, viewBox: true });
  });

  it("derives the missing side from the viewBox's aspect ratio", () => {
    expect(intrinsicSize(root({ width: "200", viewBox: "0 0 100 50" }))).toMatchObject({ width: 200, height: 100 });
    expect(intrinsicSize(root({ height: "20", viewBox: "0 0 100 50" }))).toMatchObject({ width: 40, height: 20 });
  });

  it("ignores percentages and garbage, and defaults to 300x150 with nothing", () => {
    expect(intrinsicSize(root({ width: "100%", height: "100%", viewBox: "0,0,10,10" }))).toMatchObject({ width: 10, height: 10 });
    expect(intrinsicSize(root({ width: "abc" }))).toEqual({ width: 300, height: 150, viewBox: false });
  });
});

describe("targetSize", () => {
  it("renders at twice the intrinsic size", () => {
    expect(targetSize({ width: 100, height: 50 }, 2048)).toEqual({ width: 200, height: 100 });
  });

  it("caps the longest side, keeping the aspect ratio", () => {
    expect(targetSize({ width: 4000, height: 2000 }, 2048)).toEqual({ width: 2048, height: 1024 });
  });

  it("never returns a zero side", () => {
    expect(targetSize({ width: 1000, height: 0.01 }, 100)).toEqual({ width: 100, height: 1 });
  });
});

/** A minimal valid PNG: signature + IHDR(1x1 RGBA) + IDAT + IEND (CRCs are not checked by `withPhys`). */
function tinyPng(extraChunkBeforeIdat?: Uint8Array): Uint8Array {
  const chunk = (type: string, data: number[]) => {
    const out = new Uint8Array(12 + data.length);
    new DataView(out.buffer).setUint32(0, data.length);
    out.set([...type].map((c) => c.charCodeAt(0)), 4);
    out.set(data, 8);
    return out;
  };
  const parts = [
    new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", [0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]),
    ...(extraChunkBeforeIdat ? [extraChunkBeforeIdat] : []),
    chunk("IDAT", [1, 2, 3]),
    chunk("IEND", []),
  ];
  const png = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) (png.set(p, o), (o += p.length));
  return png;
}

describe("withPhys", () => {
  const chunks = (png: Uint8Array) => {
    const v = new DataView(png.buffer, png.byteOffset, png.byteLength);
    const out: { type: string; data: Uint8Array }[] = [];
    for (let o = 8; o < png.length; ) {
      const len = v.getUint32(o);
      out.push({ type: String.fromCharCode(...png.subarray(o + 4, o + 8)), data: png.subarray(o + 8, o + 8 + len) });
      o += 12 + len;
    }
    return out;
  };

  it("inserts a pHYs chunk that pandoc reads as 192 dpi right after IHDR", () => {
    const out = withPhys(tinyPng());
    expect(chunks(out).map((c) => c.type)).toEqual(["IHDR", "pHYs", "IDAT", "IEND"]);
    const phys = chunks(out)[1].data;
    const v = new DataView(phys.buffer, phys.byteOffset);
    expect([v.getUint32(0), v.getUint32(4), phys[8]]).toEqual([7560, 7560, 1]);
  });

  it("writes a correct CRC (PNG chunk CRC-32 over type + data)", () => {
    const out = withPhys(tinyPng());
    // 8 (sig) + 25 (IHDR) = 33: length(4) type(4) data(9) crc(4)
    const crc = new DataView(out.buffer, out.byteOffset).getUint32(33 + 17);
    // Python zlib.crc32(b"pHYs" + 00 00 1D 88 00 00 1D 88 01); 7560 = 0x1D88.
    expect(crc).toBe(0xf92b5f7f);
  });

  it("leaves a PNG that already has pHYs alone", () => {
    const existing = withPhys(tinyPng());
    expect(withPhys(existing)).toEqual(existing);
  });

  it("refuses non-PNG bytes", () => {
    expect(() => withPhys(new Uint8Array(10))).toThrow(/no PNG/);
  });
});

describe("jsRasterizeSvg without a DOM (node)", () => {
  afterEach(() => setRasterizer(null));

  it("is unavailable, which the Rust side treats as a silent no-op", async () => {
    expect(jsCanRasterizeSvg()).toBe(false);
    await expect(jsRasterizeSvg(new Uint8Array([1]), 2048)).rejects.toMatchObject({ name: "RasterizerUnavailable" });
  });

  it("runs the override installed by setRasterizer, and reports available", async () => {
    const fn = vi.fn(async () => new Uint8Array([9]));
    setRasterizer(fn);
    expect(jsCanRasterizeSvg()).toBe(true);
    await expect(jsRasterizeSvg(new Uint8Array([1, 2]), 512)).resolves.toEqual(new Uint8Array([9]));
    expect(fn).toHaveBeenCalledWith(new Uint8Array([1, 2]), 512);
  });

  it("refuses an input over the size cap before rasterizing", async () => {
    const fn = vi.fn(async () => new Uint8Array());
    setRasterizer(fn);
    await expect(jsRasterizeSvg(new Uint8Array(MAX_INPUT_BYTES + 1), 2048)).rejects.toThrow(/limit/);
    expect(fn).not.toHaveBeenCalled();
  });

  it("rejects when the signal is already aborted, or aborts mid-flight", async () => {
    setRasterizer(() => new Promise(() => {}));
    const done = new AbortController();
    done.abort(new Error("click cancelled"));
    await expect(jsRasterizeSvg(new Uint8Array([1]), 2048, done.signal)).rejects.toThrow("click cancelled");
    const mid = new AbortController();
    const p = jsRasterizeSvg(new Uint8Array([1]), 2048, mid.signal);
    mid.abort(new Error("later"));
    await expect(p).rejects.toThrow("later");
  });

  it("times out a rasterizer that never settles", async () => {
    vi.useFakeTimers();
    try {
      setRasterizer(() => new Promise(() => {}));
      const p = jsRasterizeSvg(new Uint8Array([1]), 2048);
      const assertion = expect(p).rejects.toThrow(/longer than 10 s/);
      await vi.advanceTimersByTimeAsync(10_001);
      await assertion;
    } finally {
      vi.useRealTimers();
    }
  });
});
