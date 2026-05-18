import { Fragment, type ReactNode } from 'react';
import type { CodeBlock as CodeBlockType, NodeArgs } from '../../framework';

/**
 * Attribute key the Rust `CodeHighlightStage` writes into a code
 * node's kvs map. Wire format: JSON `[[start_byte, end_byte,
 * capture_name], ...]` — the same triple-array shape decoded by the
 * native HTML writer in `crates/pampa/src/writers/html.rs`. Keep this
 * literal aligned with `crates/quarto-highlight-encoding`'s
 * `SPANS_ATTR_KEY`. (bd-nxslt)
 */
const HL_SPANS_KEY = 'data-hl-spans';

/** One decoded highlight span — a half-open `[start, end)` byte range
 *  in the code text plus the tree-sitter capture name. */
interface HighlightSpan {
  start: number;
  end: number;
  capture: string;
}

/**
 * Parse the JSON triple-array stored in `data-hl-spans`. Tolerates
 * malformed input — any decode error returns `null`, which the
 * caller treats the same as a missing attribute (renders plain
 * `<code>`). Trailing positional elements in each triple are ignored
 * (forward-compat with future extras per the encoding spec).
 */
function decodeHighlightSpans(raw: string | undefined): HighlightSpan[] | null {
  if (!raw) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!Array.isArray(parsed) || parsed.length === 0) return null;
  const spans: HighlightSpan[] = [];
  for (const entry of parsed) {
    if (!Array.isArray(entry) || entry.length < 3) continue;
    const start = typeof entry[0] === 'number' ? entry[0] : -1;
    const end = typeof entry[1] === 'number' ? entry[1] : -1;
    const capture = typeof entry[2] === 'string' ? entry[2] : '';
    if (start < 0 || end < start) continue;
    spans.push({ start, end, capture });
  }
  return spans.length > 0 ? spans : null;
}

/**
 * Map a tree-sitter capture name (e.g. `function.builtin`,
 * `string.escape`) to the CSS class suffix the HTML writer uses on
 * `<span>` elements (`function-builtin`, `string-escape`). Done at
 * render time, not encode time, so the wire form keeps the grammar-
 * native capture names. Mirrors
 * `crates/pampa/src/writers/html.rs::capture_to_class`.
 */
function captureToClass(capture: string): string {
  return capture.replace(/\./g, '-');
}

const utf8Encoder = new TextEncoder();
const utf8Decoder = new TextDecoder();

/**
 * Render the body of a code block as nested `<span class="hl-...">`
 * elements driven by the highlight spans. Walks the byte stream of
 * `text` (utf-8) in start-order, emitting plain text up to each span
 * boundary and wrapping the in-range bytes in a `<span>`.
 *
 * The walk mirrors `write_highlighted_body` in
 * `crates/pampa/src/writers/html.rs`, with the same simultaneous-
 * open / simultaneous-close tie-breakers — outer spans open first,
 * inner spans close first — so nested grammar captures (e.g. a
 * `function` enclosing a `function.builtin`) render as
 * `<span class="hl-function"><span class="hl-function-builtin">…
 * </span></span>`.
 *
 * Byte offsets index into the utf-8 representation. Sliced bytes are
 * decoded back to a string via `TextDecoder`; tree-sitter only emits
 * offsets on valid utf-8 boundaries, so the decode is always clean.
 */
function renderHighlighted(text: string, spans: HighlightSpan[]): ReactNode {
  const bytes = utf8Encoder.encode(text);

  type Event =
    | { kind: 'open'; offset: number; capture: string; tie: number }
    | { kind: 'close'; offset: number; tie: number };

  const events: Event[] = [];
  for (let i = 0; i < spans.length; i++) {
    const s = spans[i];
    const span_len = Math.max(0, s.end - s.start);
    // Outer-first opens (larger ranges sort earlier): negative len.
    events.push({ kind: 'open', offset: s.start, capture: s.capture, tie: -span_len });
    // Inner-first closes (smaller ranges sort earlier): positive
    // len + i so close-end-of-A vs close-end-of-B-at-same-offset is
    // stable.
    events.push({ kind: 'close', offset: s.end, tie: span_len + i });
  }
  events.sort((a, b) => {
    if (a.offset !== b.offset) return a.offset - b.offset;
    // Closes come before opens at the same offset so an adjacent
    // close-then-open renders close first, then open (natural
    // reading order).
    if (a.kind !== b.kind) return a.kind === 'close' ? -1 : 1;
    return a.tie - b.tie;
  });

  const children: ReactNode[] = [];
  let cursor = 0;
  // The output is a flat stream of React nodes wrapped at each open
  // event into the corresponding close. We track an open-spans stack
  // with their accumulated children buffers, and on close pop the
  // stack and emit a `<span>` carrying those children.
  type Frame = { capture: string; children: ReactNode[] };
  const stack: Frame[] = [];
  let key = 0;

  function pushText(slice: Uint8Array) {
    if (slice.length === 0) return;
    const piece = utf8Decoder.decode(slice);
    const target = stack.length > 0 ? stack[stack.length - 1].children : children;
    target.push(piece);
  }

  for (const ev of events) {
    const offset = Math.min(ev.offset, bytes.length);
    if (offset > cursor) {
      pushText(bytes.subarray(cursor, offset));
      cursor = offset;
    }
    if (ev.kind === 'open') {
      stack.push({ capture: ev.capture, children: [] });
    } else {
      // close: pop and emit the span
      const frame = stack.pop();
      if (!frame) continue; // mismatched close — tolerated
      const className = `hl-${captureToClass(frame.capture)}`;
      const target = stack.length > 0 ? stack[stack.length - 1].children : children;
      target.push(
        <span key={key++} className={className}>{frame.children}</span>,
      );
    }
  }
  // Any trailing bytes after the last event.
  if (cursor < bytes.length) {
    pushText(bytes.subarray(cursor));
  }
  // Any spans still open at end-of-text — emit them as if closed at
  // the boundary so we don't lose the text or the class. (Should not
  // happen for well-formed inputs.)
  while (stack.length > 0) {
    const frame = stack.pop()!;
    const className = `hl-${captureToClass(frame.capture)}`;
    const target = stack.length > 0 ? stack[stack.length - 1].children : children;
    target.push(
      <span key={key++} className={className}>{frame.children}</span>,
    );
  }
  return <Fragment>{children}</Fragment>;
}

export const CodeBlock = ({ node }: NodeArgs<CodeBlockType>) => {
    const [[id, classes, kvs], code] = node.c;
    const codeProps: Record<string, string> = {};
    const preProps: Record<string, string> = {};
    if (id) preProps.id = id;
    if (classes.length) {
        // Pandoc writer puts language classes on <code>; Bootstrap
        // and pampa's HTML writer follow the same convention.
        codeProps.className = classes.join(' ');
    }
    // bd-nxslt: `data-hl-spans` is consumed by the renderer (we emit
    // its content as nested `<span>` markup), so it must NOT be
    // forwarded to the DOM as a raw `data-*` attribute. Other
    // `data-*` keys (e.g. `data-loc`) still pass through for
    // downstream consumers (source-mapping, plugin attribute echo).
    let hlSpansRaw: string | undefined;
    for (const [k, v] of kvs) {
        if (k === HL_SPANS_KEY) {
            hlSpansRaw = v;
            continue;
        }
        if (k.startsWith('data-')) preProps[k] = v;
    }
    const spans = decodeHighlightSpans(hlSpansRaw);
    return (
        <pre {...preProps}>
            <code {...codeProps}>
                {spans === null ? code : renderHighlighted(code, spans)}
            </code>
        </pre>
    );
};
