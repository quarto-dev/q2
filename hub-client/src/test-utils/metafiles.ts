/**
 * Test-only: a spec-correct placeable WMF (a window, a solid black brush, one `META_RECTANGLE`, `META_EOF`). P1's `emf-docx` WMF is
 * built with a 24-byte placeable header instead of the spec's 22 (`capture_import_recordings.rs`
 * `wmf_bytes` writes one zero word too many), so it cannot render and is not used for conversion tests.
 * Same drawing as that fixture: a 100 x 50 box at 1440 units per inch, a rectangle from (10, 10) to (90, 40).
 */
export function validWmf(): Uint8Array {
  const words: number[] = [];
  const u16 = (v: number) => words.push(v & 0xffff);
  const out: number[] = [];
  const push16 = (v: number) => out.push(v & 0xff, (v >> 8) & 0xff);
  const push32 = (v: number) => {
    push16(v & 0xffff);
    push16(v >>> 16);
  };
  // placeable header: key(4) hmf(2) bbox(8) inch(2) reserved(4) checksum(2) = 22 bytes
  const header = [0xcdd7, 0x9ac6, 0, 0, 0, 100, 50, 1440];
  header.forEach(u16);
  header.forEach(push16);
  push32(0);
  push16(words.reduce((a, w) => a ^ w, 0));
  // WMF header: type, header size (words), version, size (words), objects, max record (words), members
  push16(1);
  push16(9);
  push16(0x0300);
  push32(9 + 5 + 5 + 7 + 4 + 7 + 3);
  push16(1);
  push32(7);
  push16(0);
  // META_SETWINDOWORG then META_SETWINDOWEXT (y, then x): real files always carry them, and the renderer maps through them
  push32(5);
  push16(0x020b);
  push16(0);
  push16(0);
  push32(5);
  push16(0x020c);
  push16(50);
  push16(100);
  // META_CREATEBRUSHINDIRECT: size, function, style (solid), colour (black), hatch
  push32(7);
  push16(0x02fc);
  push16(0);
  push32(0);
  push16(0);
  // META_SELECTOBJECT: size, function, object index
  push32(4);
  push16(0x012d);
  push16(0);
  // META_RECTANGLE: size, function, bottom, right, top, left
  push32(7);
  push16(0x041b);
  [40, 90, 10, 10].forEach(push16);
  // META_EOF
  push32(3);
  push16(0);
  return Uint8Array.from(out);
}
