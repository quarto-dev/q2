import { Fragment, useContext, type ReactNode } from 'react';
import { dataLocProps } from '../../framework';
import type { CodeBlock as CodeBlockType, NodeArgs } from '../../framework';
import { PreviewContext } from '../PreviewContext';
import { useCommentAnchorRef } from '../commentAnchor';
import { sliceEncodedUtf8 } from '../../utils/sliceSource';

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
 * decoded back to a string via `sliceEncodedUtf8`; tree-sitter only
 * emits offsets on valid utf-8 boundaries, so the decode is always
 * clean.
 */
/** Pandoc's two spellings of the line-numbering class. */
function isNumberLinesClass(cls: string): boolean {
    return cls === 'number-lines' || cls === 'numberLines';
}

/**
 * Attributes of the code annotation pass
 * (`crates/quarto-core/src/transforms/code_annotations.rs`). The anchors
 * attribute is `line=annotation` pairs joined by commas; it is an
 * instruction to the renderer and is not forwarded to the DOM. Keep in
 * step with the same constants in `crates/pampa/src/writers/html.rs`.
 */
const ANNOTATION_ANCHORS_KEY = 'code-annotation-anchors';
const ANNOTATION_INTERACTIVE_KEY = 'code-annotation-interactive';
const ANNOTATION_CODE_CLASS = 'code-annotation-code';
/** Read by reveal's line-highlight plugin from `div.sourceCode`. */
const LINE_HIGHLIGHT_KEYS = ['code-line-numbers', 'code-line-fragment-indices'];

/** Per-line markup request; mirrors `NumberedLines` in the native writer. */
interface LineMarkup {
    /** Code block id (`cb3`); empty when the block has none. */
    blockId: string;
    /** Number of the first line (Pandoc's `startFrom`, default 1). */
    start: number;
    /** line number → annotation number. */
    anchors: Map<number, string>;
}

/**
 * Split rendered code into Pandoc's per-line markup:
 * `<span id="cb1-2"><a href="#cb1-2"></a>…</span>` per source line, with a
 * code-annotation anchor before the lines that carry one. A highlight span
 * that crosses a line break is closed at the end of the line and reopened
 * on the next, so line spans are direct children of `<code>`. Mirrors the
 * numbered branch of `write_highlighted_body` in
 * `crates/pampa/src/writers/html.rs`.
 */
function renderLines(text: string, spans: HighlightSpan[], lines: LineMarkup): ReactNode {
    const bytes = new TextEncoder().encode(text);
    type Event =
        | { kind: 'open'; offset: number; capture: string; tie: number }
        | { kind: 'close'; offset: number; tie: number };
    const events: Event[] = [];
    spans.forEach((s, i) => {
        const len = Math.max(0, s.end - s.start);
        events.push({ kind: 'open', offset: s.start, capture: s.capture, tie: -len });
        events.push({ kind: 'close', offset: s.end, tie: len + i });
    });
    events.sort((a, b) => {
        if (a.offset !== b.offset) return a.offset - b.offset;
        if (a.kind !== b.kind) return a.kind === 'close' ? -1 : 1;
        return a.tie - b.tie;
    });

    type Frame = { capture: string; children: ReactNode[] };
    const out: ReactNode[] = [];
    let key = 0;
    let lineNo = lines.start;
    let lineChildren: ReactNode[] = [];
    // Highlight spans open on the current line, outermost first.
    let stack: Frame[] = [];

    const target = () => (stack.length > 0 ? stack[stack.length - 1].children : lineChildren);
    const closeFrame = () => {
        const frame = stack.pop()!;
        target().push(
            <span key={key++} className={`hl-${captureToClass(frame.capture)}`}>{frame.children}</span>,
        );
    };
    const endLine = (reopen: boolean) => {
        const captures = stack.map((f) => f.capture);
        while (stack.length > 0) closeFrame();
        const id = lines.blockId ? `${lines.blockId}-${lineNo}` : undefined;
        const annotation = lines.anchors.get(lineNo);
        if (annotation !== undefined) {
            out.push(
                <a
                    key={key++}
                    className="code-annotation-anchor"
                    data-target-cell={lines.blockId}
                    data-target-annotation={annotation}
                    href=""
                >
                    {annotation}
                </a>,
            );
        }
        out.push(
            <span
                key={key++}
                {...(id ? { id } : {})}
                {...(annotation !== undefined ? { className: 'code-annotation-target' } : {})}
            >
                <a {...(id ? { href: `#${id}` } : {})} />
                {lineChildren}
            </span>,
        );
        lineChildren = [];
        if (reopen) {
            out.push('\n');
            lineNo += 1;
            stack = captures.map((capture) => ({ capture, children: [] }));
        }
    };
    const pushText = (start: number, end: number) => {
        if (start >= end) return;
        const pieces = sliceEncodedUtf8(bytes, start, end).split('\n');
        pieces.forEach((piece, i) => {
            if (i > 0) endLine(true);
            if (piece) target().push(piece);
        });
    };

    let cursor = 0;
    for (const ev of events) {
        const offset = Math.min(ev.offset, bytes.length);
        if (offset > cursor) {
            pushText(cursor, offset);
            cursor = offset;
        }
        if (ev.kind === 'open') stack.push({ capture: ev.capture, children: [] });
        else if (stack.length > 0) closeFrame();
    }
    if (cursor < bytes.length) pushText(cursor, bytes.length);
    // A final newline already ended the last line; do not add an empty one.
    if (text.endsWith('\n')) {
        out.pop();
    } else if (text.length > 0) {
        endLine(false);
    }
    return <Fragment>{out}</Fragment>;
}

function renderHighlighted(text: string, spans: HighlightSpan[]): ReactNode {
  const bytes = new TextEncoder().encode(text);

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

  function pushText(start: number, end: number) {
    if (start >= end) return;
    const piece = sliceEncodedUtf8(bytes, start, end);
    const target = stack.length > 0 ? stack[stack.length - 1].children : children;
    target.push(piece);
  }

  for (const ev of events) {
    const offset = Math.min(ev.offset, bytes.length);
    if (offset > cursor) {
      pushText(cursor, offset);
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
    pushText(cursor, bytes.length);
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
    const ctx = useContext(PreviewContext);
    const anchorRef = useCommentAnchorRef(node);
    const poolId = (node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;
    const affordanceAttr = isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {};

    const [[id, classes, kvs], code] = node.c;

    // bd-nxslt: `data-hl-spans` is consumed by the renderer (we emit
    // its content as nested `<span>` markup), so it must NOT be
    // forwarded as a raw DOM attribute. Other `data-*` keys pass
    // through to the `<pre>` for downstream consumers (source-mapping,
    // plugin attribute echo).
    let hlSpansRaw: string | undefined;
    let anchorsRaw: string | undefined;
    let startFrom = 1;
    const preDataAttrs: Record<string, string> = {};
    const divDataAttrs: Record<string, string> = {};
    for (const [k, v] of kvs) {
        if (k === HL_SPANS_KEY) {
            hlSpansRaw = v;
            continue;
        }
        if (k === ANNOTATION_ANCHORS_KEY) {
            anchorsRaw = v;
            continue;
        }
        if (k === ANNOTATION_INTERACTIVE_KEY) continue;
        if (LINE_HIGHLIGHT_KEYS.includes(k)) {
            divDataAttrs[`data-${k}`] = v;
            continue;
        }
        if (k.toLowerCase() === 'startfrom') {
            const n = parseInt(v.trim(), 10);
            if (Number.isFinite(n) && n >= 0) startFrom = n;
        }
        if (k.startsWith('data-')) preDataAttrs[k] = v;
    }
    const spans = decodeHighlightSpans(hlSpansRaw);

    // Line numbers (`.number-lines`) and code annotations both need Pandoc's
    // per-line markup; see `renderLines`.
    const numbered = classes.some(isNumberLinesClass);
    const anchors = new Map<number, string>();
    for (const pair of (anchorsRaw ?? '').split(',')) {
        const [line, annotation] = pair.split('=');
        const n = parseInt((line ?? '').trim(), 10);
        if (Number.isFinite(n) && annotation) anchors.set(n, annotation.trim());
    }
    const annotated = anchors.size > 0;

    // bd-s3z1g: highlighted code blocks emit Pandoc's nested
    // structure to match the native writer in
    // `crates/pampa/src/writers/html.rs::write_highlighted_codeblock`.
    // The affordance attribute goes on the outer div (the root element).
    // The attribute's presence — even an empty list — marks a
    // source-code block (GH #1079: languages Pandoc knows but q2 has no
    // grammar for get `[]`); `spans` decides only whether there is
    // markup to nest inside `<code>`.
    if (hlSpansRaw !== undefined || numbered || annotated) {
        // A numbering class in first position is not a language
        // (`{.number-lines}`).
        const hasLanguage = classes.length > 0 && !isNumberLinesClass(classes[0]);
        const language = hasLanguage ? classes[0] : undefined;
        const extras = (hasLanguage ? classes.slice(1) : classes).filter(
            (c) => c !== ANNOTATION_CODE_CLASS,
        );
        const divClassName = ['sourceCode', ...extras].join(' ');
        const codeContainerClass = language ? `sourceCode ${language}` : 'sourceCode';
        // `numberSource` is what Pandoc's line-number CSS keys off;
        // `code-annotated` what Quarto's annotation CSS does.
        const preClassName = [
            'sourceCode',
            ...(numbered ? ['numberSource'] : []),
            ...(language ? [language] : []),
            ...(annotated ? [ANNOTATION_CODE_CLASS, 'code-annotated'] : []),
        ].join(' ');
        const perLine = numbered || annotated;
        const codeStyle =
            numbered && startFrom !== 1
                ? { counterReset: `source-line ${Math.max(0, startFrom - 1)}` }
                : undefined;
        return (
            <div ref={anchorRef} className={divClassName} {...(id ? { id } : {})} {...divDataAttrs} {...affordanceAttr} {...dataLocProps(node)}>
                <pre className={preClassName} {...preDataAttrs}>
                    <code className={codeContainerClass} {...(codeStyle ? { style: codeStyle } : {})}>
                        {perLine
                            ? renderLines(code, spans ?? [], { blockId: id, start: startFrom, anchors })
                            : spans !== null
                              ? renderHighlighted(code, spans)
                              : code}
                        {annotated ? (
                            <>
                                <div className="code-annotation-gutter-bg" />
                                <div className="code-annotation-gutter" />
                            </>
                        ) : null}
                    </code>
                </pre>
            </div>
        );
    }

    // bd-y1fs3: un-highlighted path — `<pre>` carries the `Attr` (id,
    // classes, non-hl-spans kvs); `<code>` is bare. The affordance
    // attribute (+ roving tabindex) is spread separately so its
    // number-valued `tabIndex` keeps its precise type.
    const preProps: Record<string, string> = { ...preDataAttrs };
    if (id) preProps.id = id;
    if (classes.length) preProps.className = classes.join(' ');
    return (
        <pre ref={anchorRef} {...preProps} {...affordanceAttr} {...dataLocProps(node)}>
            <code>{code}</code>
        </pre>
    );
};
