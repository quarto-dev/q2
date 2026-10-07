/**
 * Generators for the document-import memory measurement (`pandoc-import-measure.harness.spec.ts`,
 * plan P1 T8): two docx files, built in memory (a 25 MB file is never checked in).
 *
 * - `imageHeavyDocx`: incompressible images (valid noise PNGs and JPEG-framed noise), so the docx is about as
 *   big as the media it carries.
 * - `compressibleDocx`: all-zero BMPs, so a docx of a few hundred KB extracts to about
 *   `collected_total_bytes` (300 MB).
 *
 * pandoc's docx reader never decodes an image: it copies the bytes out of the archive.
 */
import { zipSync, type Zippable } from 'fflate';

const enc = new TextEncoder();
const MIB = 1024 * 1024;

const crcTable = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (b: Uint8Array) => {
  let c = 0xffffffff;
  for (let i = 0; i < b.length; i++) c = crcTable[(c ^ b[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const u32be = (n: number) => new Uint8Array([(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]);
const concat = (parts: Uint8Array[]) => {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
};
const noise = (n: number, seed: number) => {
  const out = new Uint8Array(n);
  let x = seed | 1;
  for (let i = 0; i < n; i++) {
    x ^= x << 13;
    x ^= x >>> 17;
    x ^= x << 5;
    out[i] = x & 255;
  }
  return out;
};

/** A valid PNG of about `bytes` bytes: stored deflate blocks of noise, so it does not compress. */
export function noisePng(bytes: number, seed: number): Uint8Array {
  const w = 2000;
  const h = Math.max(1, Math.round(bytes / (w * 3 + 1)));
  const raw = noise(h * (w * 3 + 1), seed);
  for (let r = 0; r < h; r++) raw[r * (w * 3 + 1)] = 0; // filter: none
  let a = 1;
  let b = 0;
  for (let i = 0; i < raw.length; i++) {
    a = (a + raw[i]) % 65521;
    b = (b + a) % 65521;
  }
  const blocks: Uint8Array[] = [new Uint8Array([0x78, 0x01])];
  for (let off = 0; off < raw.length; off += 65535) {
    const len = Math.min(65535, raw.length - off);
    const last = off + len >= raw.length ? 1 : 0;
    blocks.push(new Uint8Array([last, len & 255, len >>> 8, ~len & 255, (~len >>> 8) & 255]), raw.subarray(off, off + len));
  }
  blocks.push(u32be(((b << 16) | a) >>> 0));
  const chunk = (type: string, data: Uint8Array) => {
    const body = concat([enc.encode(type), data]);
    return [u32be(data.length), body, u32be(crc32(body))];
  };
  const ihdr = concat([u32be(w), u32be(h), new Uint8Array([8, 2, 0, 0, 0])]);
  return concat([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]), ...chunk('IHDR', ihdr), ...chunk('IDAT', concat(blocks)), ...chunk('IEND', new Uint8Array(0))]);
}

/** Noise between a JPEG's SOI and EOI markers: incompressible, and never decoded by the docx reader. */
export const noiseJpeg = (bytes: number, seed: number): Uint8Array => concat([new Uint8Array([0xff, 0xd8]), noise(bytes - 4, seed), new Uint8Array([0xff, 0xd9])]);

/** A 24-bit BMP of `width` x `height` black pixels (width * 3 must be a multiple of 4). */
export function blackBmp(width: number, height: number): Uint8Array {
  const pixels = width * 3 * height;
  const out = new Uint8Array(54 + pixels);
  const dv = new DataView(out.buffer);
  out.set([0x42, 0x4d]);
  dv.setUint32(2, out.length, true);
  dv.setUint32(10, 54, true);
  dv.setUint32(14, 40, true);
  dv.setInt32(18, width, true);
  dv.setInt32(22, height, true);
  dv.setUint16(26, 1, true);
  dv.setUint16(28, 24, true);
  dv.setUint32(34, pixels, true);
  return out;
}

interface Media {
  name: string;
  ext: string;
  mime: string;
  bytes: Uint8Array;
}

/** A minimal docx: one paragraph per image. `level` is the deflate level for the media (0 stores them). */
function docxWith(media: Media[], level: 0 | 6): Uint8Array {
  const exts = [...new Set(media.map((m) => m.ext))];
  const emu = 914400;
  const body = media
    .map(
      (m, i) =>
        `<w:p><w:r><w:drawing><wp:inline><wp:extent cx="${emu}" cy="${emu}"/><wp:docPr id="${i + 1}" name="${m.name}" descr="${m.name}"/>` +
        `<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic>` +
        `<pic:nvPicPr><pic:cNvPr id="${i + 1}" name="${m.name}"/><pic:cNvPicPr/></pic:nvPicPr>` +
        `<pic:blipFill><a:blip r:embed="rId${i + 10}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>` +
        `<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="${emu}" cy="${emu}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>` +
        `</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>`,
    )
    .join('');
  const ns =
    'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" ' +
    'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" ' +
    'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"';
  const files: Zippable = {
    '[Content_Types].xml': enc.encode(
      '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">' +
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>' +
        exts.map((e) => `<Default Extension="${e}" ContentType="${media.find((m) => m.ext === e)!.mime}"/>`).join('') +
        '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
    ),
    '_rels/.rels': enc.encode(
      '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    ),
    'word/document.xml': enc.encode(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document ${ns}><w:body>${body}</w:body></w:document>`),
    'word/_rels/document.xml.rels': enc.encode(
      '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
        media.map((m, i) => `<Relationship Id="rId${i + 10}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/${m.name}.${m.ext}"/>`).join('') +
        '</Relationships>',
    ),
  };
  for (const m of media) files[`word/media/${m.name}.${m.ext}`] = [m.bytes, { level }];
  return zipSync(files);
}

/** About 25 MB of incompressible images in the docx (three noise PNGs, two JPEG-framed noise images). */
export function imageHeavyDocx(): Uint8Array {
  const each = Math.floor(4.9 * MIB);
  const media: Media[] = [];
  for (let i = 0; i < 3; i++) media.push({ name: `photo${i}`, ext: 'png', mime: 'image/png', bytes: noisePng(each, 1000 + i) });
  for (let i = 0; i < 2; i++) media.push({ name: `scan${i}`, ext: 'jpg', mime: 'image/jpeg', bytes: noiseJpeg(each, 2000 + i) });
  return docxWith(media, 0);
}

/** `count` (default twelve) 24 MiB black BMPs: about 288 MiB extracted from a docx of a few hundred KB. */
export function compressibleDocx(count = 12): Uint8Array {
  const media: Media[] = [];
  for (let i = 0; i < count; i++) media.push({ name: `bitmap${i}`, ext: 'bmp', mime: 'image/bmp', bytes: blackBmp(4096, 2048) });
  return docxWith(media, 6);
}

/** A one-image docx, to load the wasm before the measured runs. */
export const tinyDocx = (): Uint8Array => docxWith([{ name: 'tiny', ext: 'png', mime: 'image/png', bytes: noisePng(2000, 7) }], 0);
