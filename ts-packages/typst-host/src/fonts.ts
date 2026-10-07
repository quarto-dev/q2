import { createTypstFontBuilder } from '@myriaddreamin/typst.ts/compiler';

export { packFonts, unpackFonts } from './fontBundle.ts';
import { moduleRef } from './moduleRef.ts';

/**
 * Family names of the faces in each font file, in file order, first occurrence wins, from
 * typst.ts's font builder (the same wasm that compiles). This is the source for
 * `typst-available-fonts`; `get_loaded_fonts()` lists only fonts a compile has already used.
 * Names follow typst's own parsing of the font's name table, so they can differ from
 * what native `typst fonts` prints for the same file.
 */
export async function fontFamilies(module: WebAssembly.Module, fonts: Uint8Array[]): Promise<string[]> {
  const builder = createTypstFontBuilder();
  await builder.init({ getModule: () => moduleRef(module) });
  const seen = new Set<string>();
  const families: string[] = [];
  for (const bytes of fonts) {
    const { info } = (await builder.getFontInfo(bytes)) as unknown as { info: { family: string }[] };
    for (const face of info) {
      if (!seen.has(face.family)) {
        seen.add(face.family);
        families.push(face.family);
      }
    }
  }
  return families;
}
