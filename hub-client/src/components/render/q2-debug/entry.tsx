/**
 * Entry point for the q2-debug renderer iframe.
 *
 * Loaded by `/q2-debug.html` and handles the postMessage protocol with
 * the parent (Q2DebugIframe). Mounts the framework's <Ast> with
 * q2-debug's registry as the format-side defaults, layered with any
 * user-TSX overrides loaded via LOAD_CUSTOM_COMPONENTS.
 */

import { createRoot } from 'react-dom/client';
import React from 'react';
import { Deck, Slide } from '@revealjs/react';
// Reveal CSS comes from the VENDORED copy — the same files `q2 render`
// embeds and `q2-preview/RevealDeck.tsx` imports — not from npm
// `reveal.js/*.css`, so the debug surface can't drift from render's
// deck styling (bd-4b7f1hr7; same convergence as bd-ibqkf9ry). The
// vendored↔npm byte-identity test in `revealjs/assemble.rs` keeps
// these in sync with the pinned npm package.
// bd-dg8x84bu: scoped reset, never the global Meyer reset — this debug iframe
// renders arbitrary content via <Ast> (not only decks), so a global reset would
// zero font-style on <em>/<i> here too. reset-scoped.css confines it to .reveal.
import '../../../../../resources/revealjs/reset-scoped.css';
import '../../../../../resources/revealjs/reveal.css';
import '../../../../../resources/revealjs/theme/white.css';
import '../../../../../resources/revealjs/quarto-reveal.css';
import katex from 'katex';
import 'katex/dist/katex.min.css';

import {
    Ast,
    Node,
    renderChildren,
    renderNode,
    inlinesToPlainText,
    blocksToPlainText,
} from '@quarto/preview-renderer/framework';
import type { FormatRegistry } from '@quarto/preview-renderer/framework';
import {
    Block,
    Inline,
    Para, Plain, Header, CodeBlock, BulletList, OrderedList,
    BlockQuote, Div, HorizontalRule, RawBlock, Figure,
    Str, Space, SoftBreak, LineBreak,
    Emph, Strong, Code, Link, Image, Span, Quoted,
    // Aliased: a bare `Math` import would shadow the global used by the
    // caret/selection geometry below.
    Math as MathComponent,
    blockStyle, inlineStyle,
    q2DebugRegistry,
} from '.';
import { buildCustomRegistry, type ComponentExports } from '@quarto/preview-renderer/utils/customRegistry';
import {
    makeIframeMessageDispatcher,
    type CursorPayload,
    type IframeMessage,
} from '../iframeMessageDispatch';
import {
    DATA_OFF,
    DATA_STR_TEXT,
    parseDataOff,
    sourceByteForTextIndex,
    textIndexForSourceByte,
    utf8Length,
    utf16IndexForByteOffset,
} from './sourceOffset';

// Set the renderer-surface global at module top so importing this
// module is sufficient to populate `window.__REACT_AST_DEBUG_RENDERER__`
// — the framework-primitive parity test (Plan 2A item 14) relies on
// being able to read both globals via plain module imports without
// firing `LOAD_CUSTOM_COMPONENTS` setup messages. `React`, `katex`,
// and `RevealReact` stay lazy in `loadCustomComponents` since they're
// tied to dynamic user-TSX imports (the right time to set them).
(window as any).__REACT_AST_DEBUG_RENDERER__ = {
    renderChildren, renderNode, Node,
    inlinesToPlainText, blocksToPlainText,
    Block, Inline,
    Para, Plain, Header, CodeBlock, BulletList, OrderedList,
    BlockQuote, Div, HorizontalRule, RawBlock, Figure,
    Str, Space, SoftBreak, LineBreak,
    Emph, Strong, Code, Link, Image, Span, Quoted,
    Math: MathComponent,
    q2DebugRegistry, blockStyle, inlineStyle,
};

let root: ReturnType<typeof createRoot> | null = null;
let customRegistry: Record<string, React.ComponentType<any>> = {};

interface UpdateAstPayload {
  astJson: string;
  currentFilePath: string;
  renderedContent?: string;
}

// Shared dispatcher gates UPDATE_AST on the in-flight
// LOAD_CUSTOM_COMPONENTS promise so two UPDATE_ASTs queued during
// component load run in arrival order. See
// `../iframeMessageDispatch.ts` for the rationale (the previous
// setInterval-polling pattern was phase-racy).
const dispatch = makeIframeMessageDispatcher({
  loadCustomComponents,
  updateAst: (payload) => updateAst(payload as UpdateAstPayload),
  updateCursor,
});

// ---------------------------------------------------------------------------
// Virtual caret
//
// The editor cursor arrives as a UTF-8 byte offset (`CURSOR` message).
// Every q2-debug element is stamped `data-off="start-end"` from its
// node's `l` field (see `sourceOffset.ts`), so the caret's home is the
// smallest stamped range containing the offset. Inside a `Str`, the
// payload text is wrapped in `[data-str-text]`, and a DOM `Range` on
// that text node gives the exact inter-character pixel position. The
// caret itself is a plain absolutely-positioned <div> inside `#root`
// (which is `position: relative` and the scroll container), so it
// scrolls with the content and never touches the browser selection.
// ---------------------------------------------------------------------------

let lastCursor: CursorPayload | null = null;
let caretEl: HTMLDivElement | null = null;
let caretRaf = 0;
// UTF-8 bytes of the source the current AST was parsed from, so a
// node's `data-off` range can be sliced back to its source text.
let sourceBytes: Uint8Array | null = null;
const utf8Decoder = new TextDecoder();

let selectionEl: SVGSVGElement | null = null;
const SVG_NS = 'http://www.w3.org/2000/svg';

function ensureSelectionLayer(): SVGSVGElement | null {
  const rootElement = document.getElementById('root');
  if (!rootElement) return null;
  if (!selectionEl || !selectionEl.isConnected) {
    selectionEl = document.createElementNS(SVG_NS, 'svg');
    selectionEl.setAttribute('class', 'q2-debug-selection');
    rootElement.appendChild(selectionEl);
  }
  return selectionEl;
}

/** Rect in the root's scroll space (not viewport space). */
interface Box {
  left: number; top: number; right: number; bottom: number;
  /** Source byte where the box's content starts — rows are built in this order. */
  start: number;
  /** Tie-break for several rects of one node (a wrapped Str's lines). */
  seq: number;
}

/**
 * Group boxes into rows by vertical overlap and, per row, build one
 * hull polygon that joins neighbouring boxes with diagonals: along the
 * top, each box's top edge then a line from its top-right corner to
 * the next box's top-left; along the bottom (walking back right to
 * left), each box's bottom edge then a line from its bottom-left to
 * the previous box's bottom-right. Boxes of different heights (a Str
 * text run next to a Space badge) therefore read as one connected
 * highlight instead of a row of separate blocks.
 */
function hullPaths(boxes: Box[]): string[] {
  if (boxes.length === 0) return [];
  // Rows follow *source order*, like wrapped text: walk the boxes in the
  // order their content appears in the document and start a new row only
  // when the next box goes back to the left of where the previous one
  // ended. Everything else — a raised exponent, a matrix's top row sitting
  // beside the `=` that precedes it — stays on the current row and is
  // joined by the diagonals. (Grouping by vertical position instead
  // either merged a matrix's rows with the baseline or split the exponent
  // off into its own hull; neither reads as "the selection".)
  const sorted = [...boxes].sort((a, b) => a.start - b.start || a.seq - b.seq);
  const rows: Box[][] = [];
  let prev: Box | null = null;
  for (const b of sorted) {
    const wraps = prev !== null && b.left < prev.right - 1;
    if (!prev || wraps) rows.push([b]);
    else rows[rows.length - 1].push(b);
    prev = b;
  }
  const f = (n: number) => n.toFixed(1);
  return rows.map(row => {
    row.sort((a, b) => a.left - b.left);
    let d = `M${f(row[0].left)} ${f(row[0].top)}`;
    for (let i = 0; i < row.length; i++) {
      const r = row[i];
      d += ` L${f(r.right)} ${f(r.top)}`;                  // top edge
      if (i + 1 < row.length) d += ` L${f(row[i + 1].left)} ${f(row[i + 1].top)}`; // diagonal to next top-left
    }
    for (let i = row.length - 1; i >= 0; i--) {
      const r = row[i];
      d += ` L${f(r.right)} ${f(r.bottom)}`;               // down right edge / along to bottom-right
      d += ` L${f(r.left)} ${f(r.bottom)}`;                // bottom edge
      if (i > 0) d += ` L${f(row[i - 1].right)} ${f(row[i - 1].bottom)}`; // diagonal to previous bottom-right
    }
    return d + ' Z';
  });
}

/**
 * Paint the editor selection `[start, end)` as highlight rects. Each
 * stamped element contributes:
 *   - a `Str`: the rects of the overlapping text sub-range (both ends
 *     aligned through the escape mapping, like the caret);
 *   - a leaf without text (`Space`, `SoftBreak`, `HorizontalRule`, a
 *     `CodeBlock`…): its whole box when the ranges overlap;
 *   - a container (anything with stamped descendants): nothing — its
 *     children cover it.
 */
function paintSelection(
  rootElement: HTMLElement,
  layer: SVGSVGElement,
  selection: { start: number; end: number } | undefined,
) {
  layer.replaceChildren();
  if (!selection || selection.end <= selection.start) return;
  // Size the layer to the scrollable content so paths in scroll space
  // aren't clipped.
  layer.setAttribute('width', String(rootElement.scrollWidth));
  layer.setAttribute('height', String(rootElement.scrollHeight));
  const rootRect = rootElement.getBoundingClientRect();
  const boxes: Box[] = [];
  let seq = 0;
  const addRect = (r: DOMRect, start: number) => {
    if (r.width <= 0 && r.height <= 0) return;
    const left = r.left - rootRect.left + rootElement.scrollLeft;
    const top = r.top - rootRect.top + rootElement.scrollTop;
    boxes.push({
      left, top, right: left + Math.max(r.width, 1), bottom: top + r.height,
      start, seq: seq++,
    });
  };

  for (const el of rootElement.querySelectorAll<HTMLElement>(`[${DATA_OFF}]`)) {
    const range = parseDataOff(el.getAttribute(DATA_OFF) ?? '');
    if (!range) continue;
    if (range.end <= selection.start || range.start >= selection.end) continue;

    const textHost = el.querySelector<HTMLElement>(`:scope > [${DATA_STR_TEXT}]`);
    const textNode = textHost?.firstChild;
    if (textNode && textNode.nodeType === 3 /* TEXT_NODE */) {
      const text = textNode.textContent ?? '';
      const source = sourceBytes
        ? utf8Decoder.decode(sourceBytes.subarray(range.start, range.end))
        : null;
      const toIdx = (byte: number) => {
        const rel = Math.max(0, Math.min(byte, range.end) - range.start);
        return source
          ? textIndexForSourceByte(source, text, rel)
          : utf16IndexForByteOffset(text, rel);
      };
      const from = toIdx(selection.start);
      const to = toIdx(selection.end);
      if (to <= from) continue;
      const r = document.createRange();
      r.setStart(textNode, from);
      r.setEnd(textNode, to);
      for (const rect of r.getClientRects()) addRect(rect, Math.max(range.start, selection.start));
      continue;
    }
    if (el.querySelector(`[${DATA_OFF}]`)) continue; // container
    addRect(el.getBoundingClientRect(), range.start);
  }

  for (const d of hullPaths(boxes)) {
    const path = document.createElementNS(SVG_NS, 'path');
    path.setAttribute('class', 'q2-debug-selection-hull');
    path.setAttribute('d', d);
    layer.appendChild(path);
  }
}

function ensureCaret(): HTMLDivElement | null {
  const rootElement = document.getElementById('root');
  if (!rootElement) return null;
  if (!caretEl || !caretEl.isConnected) {
    caretEl = document.createElement('div');
    caretEl.className = 'q2-debug-caret hidden';
    rootElement.appendChild(caretEl);
  }
  return caretEl;
}

function updateCursor(payload: CursorPayload | null) {
  lastCursor = payload;
  scheduleCaretPlacement();
}

/** Coalesce placement onto the next frame (also used after AST re-renders). */
function scheduleCaretPlacement() {
  if (caretRaf) return;
  caretRaf = requestAnimationFrame(() => {
    caretRaf = 0;
    placeCaret();
  });
}

interface CaretTarget {
  leaf: Leaf;
  /** Where on the leaf the caret sits: inside its text, or at an edge. */
  at: 'inside' | 'start' | 'end';
}

/**
 * Pick the leaf that hosts the caret. Only leaves qualify — never a
 * container — so an offset inside syntax with no glyph of its own
 * (`\begin{bmatrix}`, the `*` of an `Emph`, a blank line) resolves to
 * the nearest leaf *in source order*: the preceding leaf's end or the
 * following leaf's start, whichever is closer in bytes. A leaf whose
 * range contains the offset wins outright (smallest range on ties).
 */
function findCaretTarget(rootElement: HTMLElement, offset: number): CaretTarget | null {
  let inside: Leaf | null = null;
  let before: Leaf | null = null;
  let after: Leaf | null = null;
  for (const leaf of leaves(rootElement)) {
    if (offset >= leaf.start && offset <= leaf.end) {
      if (!inside || leaf.end - leaf.start < inside.end - inside.start) inside = leaf;
    } else if (leaf.end < offset) {
      if (!before || leaf.end > before.end) before = leaf;
    } else if (leaf.start > offset) {
      if (!after || leaf.start < after.start) after = leaf;
    }
  }
  if (inside) return { leaf: inside, at: 'inside' };
  if (before && after) {
    return offset - before.end <= after.start - offset
      ? { leaf: before, at: 'end' }
      : { leaf: after, at: 'start' };
  }
  if (before) return { leaf: before, at: 'end' };
  if (after) return { leaf: after, at: 'start' };
  return null;
}

function placeCaret() {
  const caret = ensureCaret();
  const rootElement = document.getElementById('root');
  if (!caret || !rootElement) return;

  const cursor = lastCursor;
  const layer = ensureSelectionLayer();
  if (layer) paintSelection(rootElement, layer, cursor?.selection);
  if (!cursor) {
    caret.classList.add('hidden');
    return;
  }

  const target = findCaretTarget(rootElement, cursor.offset);
  if (!target) {
    caret.classList.add('hidden');
    return;
  }

  let rect: DOMRect | null = null;
  const { leaf } = target;
  const textNode = leaf.textHost?.firstChild;
  if (target.at === 'inside' && leaf.textHost && textNode) {
    // Inside a Str: land between characters.
    const text = textNode.textContent ?? '';
    const rel = cursor.offset - leaf.start;
    // With the source at hand, align it against the rendered text so
    // escapes/entities don't skew the index; otherwise assume 1:1 bytes.
    const idx = sourceBytes
      ? textIndexForSourceByte(
          utf8Decoder.decode(sourceBytes.subarray(leaf.start, leaf.end)),
          text,
          rel,
        )
      : utf16IndexForByteOffset(text, rel);
    const range = document.createRange();
    range.setStart(textNode, idx);
    range.setEnd(textNode, idx);
    const rects = range.getClientRects();
    rect = rects.length > 0 ? rects[0] : range.getBoundingClientRect();
    if (!rect || (rect.width === 0 && rect.height === 0)) {
      // Collapsed ranges at the very start/end of a text node can report
      // an empty rect in some engines; fall back to the host's edge.
      const host = leaf.textHost.getBoundingClientRect();
      rect = new DOMRect(idx === 0 ? host.left : host.right, host.top, 0, host.height);
    }
  } else {
    // A textless leaf (Space, SoftBreak, a KaTeX glyph) or a source-order
    // neighbour: sit on the edge. 'inside' a multi-byte glyph such as
    // `\alpha` picks the nearer edge in bytes.
    const box = leaf.el.getBoundingClientRect();
    const atStart =
      target.at === 'start' ||
      (target.at === 'inside' && cursor.offset - leaf.start < leaf.end - cursor.offset);
    rect = new DOMRect(atStart ? box.left : box.right, box.top, 0, box.height);
  }

  const rootRect = rootElement.getBoundingClientRect();
  caret.style.left = `${rect.left - rootRect.left + rootElement.scrollLeft - 1}px`;
  caret.style.top = `${rect.top - rootRect.top + rootElement.scrollTop}px`;
  caret.style.height = `${Math.max(rect.height, 12)}px`;
  caret.classList.remove('hidden');
  caret.classList.toggle('unfocused', !cursor.focused);

  // Keep the caret in view while the user is typing in the editor.
  if (cursor.focused) {
    caret.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }
}

// ---------------------------------------------------------------------------
// Math glyph stamps
//
// `Math` renders KaTeX whose glyph spans carry `data-s`/`data-e`
// character ranges into the LaTeX (see `mathInstrument.ts`). Those are
// relative; here they become absolute `data-off` byte stamps so the
// glyphs participate in the caret, selection hull, and click resolver
// as ordinary leaves — and the Math badge, now having stamped
// descendants, is treated as a container. Byte base = the badge's own
// start + the bytes before the LaTeX inside its source slice (which is
// how the `$` / `$$` / `\(` delimiter width is discovered without the
// component having to know the syntax).
//
// Driven by a MutationObserver on `#root` rather than only a frame
// after `root.render`: React commits on its own scheduler, which can
// land *after* the next animation frame, and KaTeX output arrives via
// innerHTML which React may re-set later. Observing the DOM catches
// both. Already-stamped glyphs are skipped, so the pass is idempotent.
// ---------------------------------------------------------------------------

function stampMathLeaves(): boolean {
  const rootElement = document.getElementById('root');
  if (!rootElement || !sourceBytes) return false;
  let changed = false;
  for (const badge of rootElement.querySelectorAll<HTMLElement>('[data-math]')) {
    const range = parseDataOff(badge.getAttribute(DATA_OFF) ?? '');
    const latex = badge.getAttribute('data-math-latex');
    const host = badge.querySelector<HTMLElement>(':scope > [data-math-render]');
    if (!range || latex == null || !host) continue;
    if (!host.querySelector(`[data-s][data-e]:not([${DATA_OFF}])`)) continue; // already stamped
    const slice = utf8Decoder.decode(sourceBytes.subarray(range.start, range.end));
    const at = slice.indexOf(latex);
    if (at < 0) {
      // The AST's math text does not appear verbatim in its source span
      // (normalised whitespace?) — glyphs stay unmapped; the badge is the leaf.
      console.warn('[q2-debug] math text not found in its source span', { slice, latex });
      continue;
    }
    const base = range.start + utf8Length(slice.slice(0, at));
    for (const glyph of host.querySelectorAll<HTMLElement>('[data-s][data-e]')) {
      const s = Number(glyph.getAttribute('data-s'));
      const e = Number(glyph.getAttribute('data-e'));
      if (!Number.isFinite(s) || !Number.isFinite(e) || e <= s) continue;
      const start = base + utf8Length(latex.slice(0, s));
      const end = base + utf8Length(latex.slice(0, e));
      if (end > range.end) continue;
      glyph.setAttribute(DATA_OFF, `${start}-${end}`);
      changed = true;
    }
  }
  return changed;
}

// Any DOM change under #root (React commit, innerHTML reset) may have
// moved leaves or introduced unstamped math glyphs. Our own caret and
// selection layers live under #root too; ignore mutations inside them
// so painting doesn't re-trigger the observer in a loop.
let domSyncRaf = 0;
const rootObserver = new MutationObserver((records) => {
  const ours = records.every(r =>
    (r.target instanceof Element) &&
    (r.target.closest('.q2-debug-caret, .q2-debug-selection') !== null),
  );
  if (ours) return;
  if (domSyncRaf) return;
  domSyncRaf = requestAnimationFrame(() => {
    domSyncRaf = 0;
    stampMathLeaves();
    invalidateLeaves();
    scheduleCaretPlacement();
  });
});
{
  const rootElement = document.getElementById('root');
  if (rootElement) rootObserver.observe(rootElement, { childList: true, subtree: true });
}

window.addEventListener('resize', () => { invalidateLeaves(); scheduleCaretPlacement(); });
// Rects are viewport-relative, so any scroll moves them.
document.addEventListener('scroll', invalidateLeaves, true);

// ---------------------------------------------------------------------------
// Click → editor cursor (the reverse mapping)
//
// Native selection is disabled on `#root` (q2-debug.html), so a click
// in the debug view places the *editor* cursor instead. The browser's
// hit-testing (`caretPositionFromPoint`, `caretRangeFromPoint` on
// WebKit) gives the text node and character index under the pointer;
// inside a `Str` that index is aligned back to a byte offset within the
// node's source slice, then shifted by the node's `data-off` start.
// Clicks on anything else resolve to the nearer edge of the nearest
// stamped ancestor. The parent moves Monaco to the offset
// (`CURSOR_CLICK`) and the editor's own cursor event echoes the
// position back as a `CURSOR` message, which draws the caret.
// ---------------------------------------------------------------------------

interface CaretHit {
  node: globalThis.Node;
  offset: number;
}

function caretHitFromPoint(x: number, y: number): CaretHit | null {
  const doc = document as Document & {
    caretPositionFromPoint?: (x: number, y: number) => { offsetNode: globalThis.Node; offset: number } | null;
    caretRangeFromPoint?: (x: number, y: number) => Range | null;
  };
  if (doc.caretPositionFromPoint) {
    const pos = doc.caretPositionFromPoint(x, y);
    return pos ? { node: pos.offsetNode, offset: pos.offset } : null;
  }
  if (doc.caretRangeFromPoint) {
    const range = doc.caretRangeFromPoint(x, y);
    return range ? { node: range.startContainer, offset: range.startOffset } : null;
  }
  return null;
}

// ---------------------------------------------------------------------------
// Pointer → source offset: nearest leaf position
//
// Resolving a pointer through `elementFromPoint` alone snaps to whatever
// box is under it; over padding, margins, badge labels, or the gap
// between two badges that is a *container*, whose edges can be a whole
// paragraph apart in source. Editors avoid this by never resolving to
// containers: a pointer resolves to the nearest *leaf* position, chosen
// by vertical distance first (the row) and horizontal distance second.
//
// Leaves are stamped elements with no stamped descendants — every byte
// reachable in the document is on a leaf edge or inside a `Str` text,
// so container edges add nothing. The leaf list and its rects are
// cached per render and invalidated on scroll/resize, so a pointer
// move costs one scan plus at most one `caretPositionFromPoint`.
// ---------------------------------------------------------------------------

interface Leaf {
  el: HTMLElement;
  start: number;
  end: number;
  rect: DOMRect;
  /** The `[data-str-text]` host when this leaf is a `Str`. */
  textHost: HTMLElement | null;
}

let leafCache: Leaf[] | null = null;

function invalidateLeaves() {
  leafCache = null;
}

function leaves(rootElement: HTMLElement): Leaf[] {
  if (leafCache) return leafCache;
  const out: Leaf[] = [];
  for (const el of rootElement.querySelectorAll<HTMLElement>(`[${DATA_OFF}]`)) {
    if (el.querySelector(`[${DATA_OFF}]`)) continue; // container
    const range = parseDataOff(el.getAttribute(DATA_OFF) ?? '');
    if (!range) continue;
    const textHost = el.querySelector<HTMLElement>(`:scope > [${DATA_STR_TEXT}]`);
    const hasText = textHost?.firstChild?.nodeType === 3 /* TEXT_NODE */;
    out.push({
      el,
      start: range.start,
      end: range.end,
      rect: el.getBoundingClientRect(),
      textHost: hasText ? textHost : null,
    });
  }
  leafCache = out;
  return out;
}

const vdist = (r: DOMRect, y: number) => (y < r.top ? r.top - y : y > r.bottom ? y - r.bottom : 0);
const hdist = (r: DOMRect, x: number) => (x < r.left ? r.left - x : x > r.right ? x - r.right : 0);

/**
 * The leaf nearest the point: smallest vertical distance wins (that
 * picks the row), then smallest horizontal distance within it.
 *
 * When the pointer is in the margin *between* rows (every leaf has a
 * positive vertical distance) and two rows tie, prefer the row that
 * contains `preferOffset` (the drag anchor), so a wobbling horizontal
 * drag doesn't flicker across rows. "Row" is approximated as vertical
 * overlap with the anchor's leaf. The preference never applies while
 * the pointer is inside a row (distance 0) — there, horizontal
 * distance alone must decide, or the head could never leave the
 * anchor's node.
 */
function nearestLeaf(all: Leaf[], x: number, y: number, preferOffset: number | null): Leaf | null {
  const anchorLeaf =
    preferOffset == null
      ? null
      : all.find(l => preferOffset >= l.start && preferOffset <= l.end) ?? null;
  const inAnchorRow = (leaf: Leaf) =>
    !!anchorLeaf &&
    leaf.rect.top < anchorLeaf.rect.bottom &&
    leaf.rect.bottom > anchorLeaf.rect.top;

  let best: Leaf | null = null;
  let bestDy = Infinity;
  let bestDx = Infinity;
  let bestPreferred = false;
  for (const leaf of all) {
    const dy = vdist(leaf.rect, y);
    const dx = hdist(leaf.rect, x);
    const preferred = dy > 0 && inAnchorRow(leaf);
    const better =
      dy < bestDy ||
      (dy === bestDy && (
        (preferred && !bestPreferred) ||
        (preferred === bestPreferred && dx < bestDx)
      ));
    if (better) {
      best = leaf; bestDy = dy; bestDx = dx; bestPreferred = preferred;
    }
  }
  return best;
}

/** Byte offset within a leaf for a point, clamping into its text when it has any. */
function offsetInLeaf(leaf: Leaf, x: number, y: number): number {
  const textNode = leaf.textHost?.firstChild;
  if (leaf.textHost && textNode) {
    // A wrapped Str has one rect per line; use the line nearest the
    // pointer and clamp x into it so hits over the badge label or
    // padding slide along the text instead of jumping to an edge.
    const lines = Array.from(leaf.textHost.getClientRects());
    if (lines.length > 0) {
      const line = lines.reduce((a, b) => (vdist(b, y) < vdist(a, y) ? b : a));
      const cx = Math.min(Math.max(x, line.left + 0.5), line.right - 0.5);
      const hit = caretHitFromPoint(cx, (line.top + line.bottom) / 2);
      if (hit && hit.node === textNode) {
        const text = textNode.textContent ?? '';
        const idx = Math.min(hit.offset, text.length);
        const rel = sourceBytes
          ? sourceByteForTextIndex(
              utf8Decoder.decode(sourceBytes.subarray(leaf.start, leaf.end)),
              text,
              idx,
            )
          : utf8Length(text.slice(0, idx));
        return leaf.start + Math.min(rel, leaf.end - leaf.start);
      }
      // Hit-test missed the text (e.g. pointer-events quirk): nearer edge of the text.
      return x < (line.left + line.right) / 2 ? leaf.start : leaf.end;
    }
  }
  return x < leaf.rect.left + leaf.rect.width / 2 ? leaf.start : leaf.end;
}

/**
 * Source byte offset for a viewport point, or `null` when the document
 * has no stamped leaves at all. `preferOffset` biases row ties (see
 * `nearestLeaf`); pass the drag anchor while selecting.
 */
function sourceOffsetAtPoint(
  clientX: number,
  clientY: number,
  preferOffset: number | null = null,
): number | null {
  const rootElement = document.getElementById('root');
  if (!rootElement) return null;
  const leaf = nearestLeaf(leaves(rootElement), clientX, clientY, preferOffset);
  if (!leaf) return null;
  return offsetInLeaf(leaf, clientX, clientY);
}

// Drag state: the anchor is where the pointer went down; while the
// button is held every move re-sends `{ anchor, head }`. Pointer capture
// keeps moves flowing when the pointer leaves the iframe viewport (the
// head then just stops updating, since `elementFromPoint` finds nothing).
let dragAnchor: number | null = null;
let dragHead: number | null = null;

document.addEventListener('pointerdown', (event) => {
  if (event.button !== 0) return;
  const rootElement = document.getElementById('root');
  if (!rootElement || !rootElement.contains(event.target as globalThis.Node)) return;
  const offset = sourceOffsetAtPoint(event.clientX, event.clientY);
  if (offset == null) return;
  // Keep focus (and the native caret / selection) out of the iframe.
  event.preventDefault();
  if (event.shiftKey) {
    window.parent.postMessage({ type: 'CURSOR_CLICK', offset, extend: true }, '*');
    return;
  }
  dragAnchor = offset;
  dragHead = offset;
  rootElement.setPointerCapture(event.pointerId);
  window.parent.postMessage({ type: 'CURSOR_CLICK', offset }, '*');
});

document.addEventListener('pointermove', (event) => {
  if (dragAnchor == null || (event.buttons & 1) === 0) return;
  const head = sourceOffsetAtPoint(event.clientX, event.clientY, dragAnchor);
  if (head == null || head === dragHead) return;
  dragHead = head;
  window.parent.postMessage({ type: 'CURSOR_SELECT', anchor: dragAnchor, head }, '*');
});

const endDrag = (event: PointerEvent) => {
  if (dragAnchor == null) return;
  const rootElement = document.getElementById('root');
  if (rootElement?.hasPointerCapture(event.pointerId)) {
    rootElement.releasePointerCapture(event.pointerId);
  }
  dragAnchor = null;
  dragHead = null;
};
document.addEventListener('pointerup', endDrag);
document.addEventListener('pointercancel', endDrag);

window.addEventListener('message', (event) => {
  // In production, verify event.origin for security
  dispatch(event.data as IframeMessage);
});

/**
 * Load custom components from transpiled JS code using dynamic imports.
 */
async function loadCustomComponents(componentsCode: Record<string, string>) {
  // Make React, RevealReact, and katex available as globals for user
  // TSX modules. The renderer-surface global is set at module top
  // (see above); these three remain lazy because they're tied to
  // dynamic user-TSX imports — `loadCustomComponents` is the right
  // moment to materialize them.
  (window as any).React = React;
  (window as any).RevealReact = { Deck, Slide };
  (window as any).katex = katex;

  const loadedModules: ComponentExports[] = [];
  for (const [componentName, code] of Object.entries(componentsCode)) {
    try {
      const blob = new Blob([code], { type: 'application/javascript' });
      const url = URL.createObjectURL(blob);
      try {
        const module = await import(/* @vite-ignore */ url);
        loadedModules.push(module as ComponentExports);
        console.log(`[Q2DebugIframe] Loaded custom component: ${componentName}`);
      } finally {
        URL.revokeObjectURL(url);
      }
    } catch (err) {
      console.error(`[Q2DebugIframe] Failed to load custom component ${componentName}:`, err);
    }
  }

  customRegistry = buildCustomRegistry(loadedModules);
}

function updateAst(payload: UpdateAstPayload) {
  const {
    astJson,
    currentFilePath,
    renderedContent,
  } = payload;
  sourceBytes = renderedContent != null ? new TextEncoder().encode(renderedContent) : null;

  // Merge q2-debug defaults with any user-TSX overrides. The cast asserts
  // the merged result satisfies the FormatRegistry contract; the override
  // side is babel-transpiled user code and runtime-trusted.
  const mergedRegistry: FormatRegistry = {
    ...q2DebugRegistry,
    ...customRegistry,
  } as FormatRegistry;

  const rootElement = document.getElementById('root');
  if (!rootElement) {
    console.error('Root element not found');
    return;
  }

  try {
    // Create root only once
    if (!root) {
      root = createRoot(rootElement);
    }

    // Render the Ast component
    root.render(
      <Ast
        astJson={astJson}
        currentFilePath={currentFilePath}
        onNavigateToDocument={(path, anchor) => {
          window.parent.postMessage({
            type: 'NAVIGATE_TO_DOCUMENT',
            path,
            anchor
          }, '*');
        }}
        setAst={(newAst) => {
          window.parent.postMessage({
            type: 'SET_AST',
            ast: newAst
          }, '*');
        }}
        registry={mergedRegistry}
      />
    );
    // The new tree commits asynchronously; re-anchor the caret once it
    // has painted (a second frame covers React's own scheduling).
    requestAnimationFrame(() => { stampMathLeaves(); invalidateLeaves(); scheduleCaretPlacement(); });
  } catch (err) {
    console.error('Failed to render AST:', err);
    rootElement.innerHTML = `
      <div style="padding: 20px; color: red;">
        <strong>Render Error:</strong>
        <pre>${err instanceof Error ? err.message : String(err)}</pre>
      </div>
    `;
  }
}

// Signal that the iframe is ready to receive messages
window.parent.postMessage({ type: 'IFRAME_READY' }, '*');
