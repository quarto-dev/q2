import { sliceUtf8 } from './utf8Slice';
export { sliceEncodedUtf8 } from './utf8Slice';

/**
 * Slice a source string by UTF-8 byte offsets.
 *
 * The q2 parser and tree-sitter both report source positions as byte
 * offsets into the UTF-8 encoding of the source text. `sliceBytes`
 * converts such a [start, end) byte range back to the corresponding
 * JS string.
 */
export function sliceBytes(
  content: string,
  start: number,
  end: number,
): string {
  return sliceUtf8(content, start, end);
}
