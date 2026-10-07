/**
 * Source byte-offset stamps for the q2-debug virtual caret.
 *
 * The q2-debug AST is written with `include_inline_locations: true`
 * (`parse_qmd_to_ast` in `wasm-quarto-hub-client/src/lib.rs`), so every
 * node carries an `l` field shaped like `resolve_location` in
 * `crates/pampa/src/writers/json.rs`:
 *
 *   l: { f: fileId, b: { o, l, c }, e: { o, l, c } }
 *
 * where `o` is a UTF-8 byte offset into the source. `dataOffProps`
 * turns that into a `data-off="start-end"` attribute. The iframe entry
 * (`entry.tsx`) queries these to find the node under the editor caret
 * and positions its own CSS caret there — the DOM is never made
 * contenteditable and the browser selection is never used.
 *
 * `DATA_STR_TEXT` marks the element whose only child is a node's own
 * source text (the `Str` payload), so the caret can be placed between
 * characters rather than only at node edges.
 */

export const DATA_OFF = 'data-off';
export const DATA_STR_TEXT = 'data-str-text';

interface Loc {
    f: number;
    b: { o: number; l: number; c: number };
    e: { o: number; l: number; c: number };
}

export function dataOffProps(node: unknown): { [DATA_OFF]?: string } {
    const loc = (node as { l?: Loc } | null | undefined)?.l;
    // Only the primary file (id 0) is open in the editor.
    if (!loc || !loc.b || !loc.e || loc.f !== 0) return {};
    return { [DATA_OFF]: `${loc.b.o}-${loc.e.o}` };
}

export function parseDataOff(value: string): { start: number; end: number } | null {
    const m = /^(\d+)-(\d+)$/.exec(value);
    if (!m) return null;
    return { start: parseInt(m[1], 10), end: parseInt(m[2], 10) };
}

/** UTF-8 byte length of a JS string. */
export function utf8Length(text: string): number {
    // Fast path: pure ASCII has one byte per UTF-16 unit.
    // eslint-disable-next-line no-control-regex
    if (!/[^\u0000-\u007f]/.test(text)) return text.length;
    return new TextEncoder().encode(text).length;
}

/**
 * Index (in UTF-16 code units, as DOM `Range` wants) of the character
 * boundary in `text` that sits `byteOffset` UTF-8 bytes in. Clamped to
 * `[0, text.length]`; the clamp also covers `Str` payloads whose text
 * is shorter than their source span (escapes, entities, smart quotes).
 */
export function utf16IndexForByteOffset(text: string, byteOffset: number): number {
    if (byteOffset <= 0) return 0;
    // eslint-disable-next-line no-control-regex
    if (!/[^\u0000-\u007f]/.test(text)) return Math.min(byteOffset, text.length);
    let bytes = 0;
    let i = 0;
    while (i < text.length) {
        const cp = text.codePointAt(i)!;
        const cpBytes = cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
        if (bytes + cpBytes > byteOffset) break;
        bytes += cpBytes;
        i += cp >= 0x10000 ? 2 : 1;
    }
    return i;
}

/**
 * Walk a node's source slice and its rendered text in lockstep until
 * `stop(bytesConsumed, textIndex)` says so, returning where each side
 * got to. Shared by the two directions of the caret mapping:
 *
 *   - `textIndexForSourceByte`  (editor → preview: stop at a byte count)
 *   - `sourceByteForTextIndex`  (preview → editor: stop at a text index)
 *
 * The `Str` payload is not always byte-for-byte the source it spans:
 * `a+\=\=o` renders as `a+==o`, `&amp;` as `&`, `"` as `“`. A linear
 * mapping therefore drifts right by one per escape and clamps at the
 * end. Here equal characters advance together; a backslash followed by
 * the next text character is consumed as an escape (the position just
 * after the backslash stays *before* the escaped character, matching
 * Monaco's view of the source); any other mismatch looks ahead a few
 * source characters for the text character (entities) and otherwise
 * treats it as a one-for-one substitution (smart punctuation).
 */
function alignSourceAndText(
    source: string,
    text: string,
    stop: (bytes: number, textIndex: number) => boolean,
): { bytes: number; textIndex: number } {
    let si = 0; // index into source (UTF-16 units)
    let sb = 0; // bytes consumed from source
    let ti = 0; // index into text (UTF-16 units)
    const cpLen = (cp: number) => (cp >= 0x10000 ? 2 : 1);
    const cpBytes = (cp: number) => (cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4);

    while (!stop(sb, ti) && si < source.length && ti < text.length) {
        const sc = source.codePointAt(si)!;
        const tc = text.codePointAt(ti)!;
        if (sc === tc) {
            sb += cpBytes(sc); si += cpLen(sc); ti += cpLen(tc);
            continue;
        }
        if (sc === 0x5c /* \ */ && si + 1 < source.length && source.codePointAt(si + 1) === tc) {
            // Escape: consume only the backslash; the next loop matches.
            sb += 1; si += 1;
            continue;
        }
        // Look ahead for the text character (e.g. `&amp;` → `&`).
        let found = -1;
        for (let k = si, n = 0; k < source.length && n < 12; k += cpLen(source.codePointAt(k)!), n++) {
            if (source.codePointAt(k) === tc) { found = k; break; }
        }
        if (found > si) {
            while (si < found && !stop(sb, ti)) {
                const c = source.codePointAt(si)!;
                sb += cpBytes(c); si += cpLen(c);
            }
            continue;
        }
        // Substitution (smart quotes etc.): advance both.
        sb += cpBytes(sc); si += cpLen(sc); ti += cpLen(tc);
    }
    return { bytes: sb, textIndex: ti };
}

/**
 * Editor → preview: the UTF-16 index into `text` that corresponds to
 * `byteOffset` bytes into the node's `source` slice. Clamped to
 * `[0, text.length]`.
 */
export function textIndexForSourceByte(source: string, text: string, byteOffset: number): number {
    if (byteOffset <= 0) return 0;
    const { textIndex } = alignSourceAndText(source, text, (sb) => sb >= byteOffset);
    return Math.min(textIndex, text.length);
}

/**
 * Preview → editor: the byte offset into the node's `source` slice
 * that corresponds to UTF-16 index `textIndex` into `text`. A click
 * past the end of the text maps to the end of the source span, so the
 * closing bytes of an escaped run are reachable too.
 */
export function sourceByteForTextIndex(source: string, text: string, textIndex: number): number {
    if (textIndex <= 0) return 0;
    if (textIndex >= text.length) return utf8Length(source);
    const { bytes } = alignSourceAndText(source, text, (_sb, ti) => ti >= textIndex);
    return bytes;
}
