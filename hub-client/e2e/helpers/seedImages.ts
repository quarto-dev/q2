/**
 * Shared by the pandoc memory specs (pandoc-measure, pandoc-memory; host phases H3, H6).
 */
import type { Page } from '@playwright/test';
import type {} from './testHooks';

/**
 * Image-heavy render in two steps, so process memory can be read between them: `seedImages`
 * puts `count` images of about `mb` MB each in the VFS (PNG with stored deflate blocks of
 * xorshift noise: valid, incompressible like a photo) and `renderSeeded` runs the harness.
 * The generator's own buffers are garbage after the first call returns (Chromium is
 * launched with `--expose-gc` and collected between the steps), so the second step's growth
 * is the render chain's.
 */
export async function seedImages(page: Page, count: number, mb: number) {
  return page.evaluate(
    async ({ count, mb }) => {
      const { wasmRenderer } = window.__quartoTest!;
      await wasmRenderer.initWasm();
      wasmRenderer.vfsClear();
      const crcTable = new Uint32Array(256).map((_, n) => {
        let c = n;
        for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
        return c >>> 0;
      });
      const crc = (b: Uint8Array) => {
        let c = 0xffffffff;
        for (let i = 0; i < b.length; i++) c = crcTable[(c ^ b[i]) & 0xff] ^ (c >>> 8);
        return (c ^ 0xffffffff) >>> 0;
      };
      const u32 = (n: number) => new Uint8Array([(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]);
      const chunk = (type: string, data: Uint8Array) => {
        const t = new TextEncoder().encode(type);
        const body = new Uint8Array(4 + data.length);
        body.set(t);
        body.set(data, 4);
        return [u32(data.length), body, u32(crc(body))];
      };
      const png = (bytesTarget: number, seed: number) => {
        const w = 2000;
        const h = Math.max(1, Math.round(bytesTarget / (w * 3 + 1)));
        const raw = new Uint8Array(h * (w * 3 + 1));
        let x = seed | 1;
        for (let i = 0; i < raw.length; i++) {
          x ^= x << 13;
          x ^= x >>> 17;
          x ^= x << 5;
          raw[i] = x & 255;
        }
        for (let r = 0; r < h; r++) raw[r * (w * 3 + 1)] = 0; // filter: none
        const blocks: Uint8Array[] = [new Uint8Array([0x78, 0x01])];
        let a = 1;
        let b = 0;
        for (let i = 0; i < raw.length; i++) {
          a = (a + raw[i]) % 65521;
          b = (b + a) % 65521;
        }
        for (let off = 0; off < raw.length; off += 65535) {
          const len = Math.min(65535, raw.length - off);
          const last = off + len >= raw.length ? 1 : 0;
          blocks.push(new Uint8Array([last, len & 255, len >>> 8, ~len & 255, (~len >>> 8) & 255]), raw.subarray(off, off + len));
        }
        blocks.push(u32(((b << 16) | a) >>> 0));
        const z = new Uint8Array(blocks.reduce((n, p) => n + p.length, 0));
        let o = 0;
        for (const p of blocks) {
          z.set(p, o);
          o += p.length;
        }
        const ihdr = new Uint8Array(13);
        ihdr.set(u32(w), 0);
        ihdr.set(u32(h), 4);
        ihdr.set([8, 2, 0, 0, 0], 8);
        const parts = [new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]), ...chunk('IHDR', ihdr), ...chunk('IDAT', z), ...chunk('IEND', new Uint8Array(0))];
        const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
        o = 0;
        for (const p of parts) {
          out.set(p, o);
          o += p.length;
        }
        return out;
      };
      let md = '---\ntitle: Images\n---\n\n';
      let total = 0;
      for (let i = 0; i < count; i++) {
        const bytes = png(mb * 1024 * 1024, 12345 + i);
        total += bytes.length;
        wasmRenderer.vfsAddBinaryFile(`/project/img${i}.png`, bytes);
        md += `![Figure ${i}](img${i}.png)\n\n`;
      }
      wasmRenderer.vfsAddFile('/project/images.qmd', md);
      (globalThis as { gc?: () => void }).gc?.();
      return total;
    },
    { count, mb },
  );
}

