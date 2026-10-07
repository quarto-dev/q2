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
import type { PandocAST } from '@quarto/preview-renderer/framework';
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

// ---------------------------------------------------------------------------
// Input sink
//
// Typing in the preview edits the *editor*, not the DOM here: keystrokes
// are translated into Monaco commands (`EDIT` messages) so Monaco keeps
// ownership of the text, the undo stack, selection replacement and IME
// semantics, and its cursor event echoes the result back as `CURSOR`.
// The sink is a hidden <textarea> focused on click. Using `beforeinput`
// (not `keydown`) for text gives us dead keys, IME composition and
// paste for free; navigation keys that never produce input go through
// `keydown`. The sink follows the caret so an IME candidate window
// opens next to it.
// ---------------------------------------------------------------------------

let inputEl: HTMLTextAreaElement | null = null;

function ensureInput(): HTMLTextAreaElement | null {
  const rootElement = document.getElementById('root');
  if (!rootElement) return null;
  if (!inputEl || !inputEl.isConnected) {
    inputEl = document.createElement('textarea');
    inputEl.className = 'q2-debug-input';
    inputEl.setAttribute('aria-label', 'Preview text input');
    inputEl.autocapitalize = 'off';
    inputEl.autocomplete = 'off';
    inputEl.spellcheck = false;
    inputEl.tabIndex = -1;
    inputEl.addEventListener('beforeinput', onBeforeInput);
    inputEl.addEventListener('compositionend', onCompositionEnd);
    inputEl.addEventListener('keydown', onInputKeyDown);
    inputEl.addEventListener('focus', scheduleCaretPlacement);
    inputEl.addEventListener('blur', scheduleCaretPlacement);
    rootElement.appendChild(inputEl);
  }
  return inputEl;
}

function previewHasFocus(): boolean {
  return !!inputEl && document.activeElement === inputEl && document.hasFocus();
}

function sendEdit(edit: { kind: 'type'; text: string } | { kind: 'command'; command: string }) {
  window.parent.postMessage({ type: 'EDIT', ...edit }, '*');
}

function onBeforeInput(event: InputEvent) {
  switch (event.inputType) {
    // Text mutations are AST edits (`applyLocalEdit`) or nothing. They are
    // never forwarded to Monaco as raw source edits: a source change with
    // no corresponding AST edit (a selection into math deleting `$$`)
    // is exactly what this surface must not produce.
    case 'insertText':
      event.preventDefault();
      if (event.data) applyLocalEdit('type', event.data);
      break;
    case 'insertLineBreak':
    case 'insertParagraph':
      event.preventDefault();                          // paragraph split: not expressible yet
      break;
    case 'insertFromPaste': {
      event.preventDefault();
      const text = event.dataTransfer?.getData('text/plain') ?? event.data ?? '';
      if (text) applyLocalEdit('type', text);         // multi-line paste is a no-op
      break;
    }
    case 'deleteContentBackward':
      event.preventDefault(); applyLocalEdit('deleteLeft'); break;
    case 'deleteContentForward':
      event.preventDefault(); applyLocalEdit('deleteRight'); break;
    case 'deleteWordBackward':
    case 'deleteWordForward':
    case 'deleteSoftLineBackward':
    case 'deleteHardLineBackward':
      event.preventDefault();                          // not expressible as a node edit yet
      break;
    case 'historyUndo':
      event.preventDefault(); sendEdit({ kind: 'command', command: 'undo' }); break;
    case 'historyRedo':
      event.preventDefault(); sendEdit({ kind: 'command', command: 'redo' }); break;
    case 'insertCompositionText':
      // IME in progress: let the textarea compose; `compositionend` commits.
      break;
    default:
      event.preventDefault();
  }
}

function onCompositionEnd(event: CompositionEvent) {
  if (event.data) applyLocalEdit('type', event.data);
  if (inputEl) inputEl.value = '';
}

// Keys that never produce `beforeinput`. Left/right are handled locally
// (see `moveCaret`: they walk AST caret stops, not source bytes); the
// vertical/line keys still go to Monaco until a geometric model exists.
const NAV_COMMANDS: Record<string, [plain: string, select: string]> = {
  ArrowUp: ['cursorUp', 'cursorUpSelect'],
  ArrowDown: ['cursorDown', 'cursorDownSelect'],
  Home: ['cursorHome', 'cursorHomeSelect'],
  End: ['cursorEnd', 'cursorEndSelect'],
};

function onInputKeyDown(event: KeyboardEvent) {
  if (event.isComposing) return;
  if ((event.key === 'ArrowLeft' || event.key === 'ArrowRight') && !event.metaKey && !event.ctrlKey && !event.altKey) {
    event.preventDefault();
    moveCaret(event.key === 'ArrowLeft' ? -1 : 1, event.shiftKey);
    return;
  }
  const nav = NAV_COMMANDS[event.key];
  if (nav && !event.metaKey && !event.ctrlKey) {
    event.preventDefault();
    sendEdit({ kind: 'command', command: nav[event.shiftKey ? 1 : 0] });
    return;
  }
  const mod = event.metaKey || event.ctrlKey;
  if (mod && event.key.toLowerCase() === 'a') {
    event.preventDefault();
    sendEdit({ kind: 'command', command: 'editor.action.selectAll' });
    return;
  }
  if (mod && event.key.toLowerCase() === 'z') {
    // Some engines deliver undo/redo only as keydown, not `historyUndo`.
    event.preventDefault();
    sendEdit({ kind: 'command', command: event.shiftKey ? 'redo' : 'undo' });
    return;
  }
  if (event.key === 'Escape') {
    inputEl?.blur();
  }
}
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
  // While the preview's input sink has focus, the preview caret is the
  // source of truth: clicks, drags and local edits move it directly, and
  // Monaco is merely kept in step (parked by the parent). Passive echoes
  // from Monaco in that state are the intermediate positions of write
  // round-trips, not user intent — ignore them. The one exception is a
  // payload Monaco produced *for us*: the result of a command we sent
  // (`fromCommand`), read synchronously after it ran.
  if (previewHasFocus() && !payload?.fromCommand) return;
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
  if (target.at === 'inside' && leaf.textHost && textNode && textNode.nodeType === 3) {
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
  const focused = cursor.focused || previewHasFocus();
  caret.classList.toggle('unfocused', !focused);

  // Park the input sink on the caret so IME candidate windows open there.
  const input = ensureInput();
  if (input) {
    input.style.left = caret.style.left;
    input.style.top = caret.style.top;
    input.style.height = caret.style.height;
  }

  // Keep the caret in view while the user is typing in the editor.
  if (focused) {
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
// both. Badges whose range is unchanged are skipped, so the pass is
// idempotent. React updates the *badge's* `data-off` as an attribute
// change, so the observer watches that attribute too.
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
    // Stamps are absolute byte offsets, so they go stale whenever the
    // badge's own range moves (typing anywhere before the math shifts
    // every offset after it) even though the KaTeX HTML — memoized per
    // LaTeX — is untouched. Key the stamping to the badge range and
    // redo it when that key changes; skip only when it matches.
    const key = `${range.start}-${range.end}`;
    if (host.dataset.stampedFor === key) continue;
    host.dataset.stampedFor = key;
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
  if (rootElement) {
    rootObserver.observe(rootElement, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: [DATA_OFF],
    });
  }
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

let stopsCache: number[] | null = null;

function invalidateLeaves() {
  leafCache = null;
  stopsCache = null;
}

/**
 * Every position the caret can occupy, as source byte offsets, in
 * document order: each character boundary of a Str (escape-aligned), and
 * both edges of every textless leaf (Space, SoftBreak, a KaTeX glyph…).
 * This is the AST-position model the arrow keys walk; text positions
 * are derived from it only when an edit is applied. (Monaco's cursor
 * would instead step through source syntax such as `\frac{`.)
 */
function caretStops(rootElement: HTMLElement): number[] {
  if (stopsCache) return stopsCache;
  const stops = new Set<number>();
  for (const leaf of leaves(rootElement)) {
    const text = leaf.textHost?.textContent;
    if (leaf.textHost && text != null && text.length > 0) {
      const slice = sourceBytes ? utf8Decoder.decode(sourceBytes.subarray(leaf.start, leaf.end)) : null;
      for (let i = 0; i <= text.length; i++) {
        const byte = slice ? sourceByteForTextIndex(slice, text, i) : utf8Length(text.slice(0, i));
        stops.add(leaf.start + byte);
        if (i < text.length && text.codePointAt(i)! >= 0x10000) i++;   // skip the low surrogate
      }
    } else {
      stops.add(leaf.start);
      stops.add(leaf.end);
    }
  }
  stopsCache = [...stops].sort((a, b) => a - b);
  return stopsCache;
}

/** Nearest stop strictly before/after `offset`, or `offset` itself at the ends. */
function stepCaret(offset: number, dir: -1 | 1): number {
  const rootElement = document.getElementById('root');
  if (!rootElement) return offset;
  const stops = caretStops(rootElement);
  if (dir > 0) {
    const next = stops.find(x => x > offset);
    return next ?? offset;
  }
  for (let i = stops.length - 1; i >= 0; i--) if (stops[i] < offset) return stops[i];
  return offset;
}

/** Move the local caret (optionally extending the selection) and keep Monaco in step. */
function moveCaret(dir: -1 | 1, extend: boolean) {
  const cur = lastCursor;
  if (!cur) return;
  // With a selection and no shift, left/right collapse to its edge.
  if (cur.selection && !extend) {
    const to = dir < 0 ? cur.selection.start : cur.selection.end;
    lastCursor = { offset: to, focused: true };
    scheduleCaretPlacement();
    window.parent.postMessage({ type: 'CURSOR_CLICK', offset: to }, '*');
    return;
  }
  const head = stepCaret(cur.offset, dir);
  if (extend) {
    const anchor = cur.selection
      ? (cur.offset === cur.selection.start ? cur.selection.end : cur.selection.start)
      : cur.offset;
    setLocalSelection(anchor, head);
    window.parent.postMessage({ type: 'CURSOR_SELECT', anchor, head }, '*');
  } else {
    lastCursor = { offset: head, focused: true };
    scheduleCaretPlacement();
    window.parent.postMessage({ type: 'CURSOR_CLICK', offset: head }, '*');
  }
}

function leaves(rootElement: HTMLElement): Leaf[] {
  if (leafCache) return leafCache;
  const out: Leaf[] = [];
  for (const el of rootElement.querySelectorAll<HTMLElement>(`[${DATA_OFF}]`)) {
    if (el.querySelector(`[${DATA_OFF}]`)) continue; // container
    const range = parseDataOff(el.getAttribute(DATA_OFF) ?? '');
    if (!range) continue;
    // A Str's text host counts even when its text is empty (a locally
    // deleted node awaiting the reparse renders no text node at all).
    const textHost = el.querySelector<HTMLElement>(`:scope > [${DATA_STR_TEXT}]`);
    out.push({
      el,
      start: range.start,
      end: range.end,
      rect: el.getBoundingClientRect(),
      textHost,
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
  // Cancel the native caret / selection, then focus the input sink so
  // typing goes to the editor via EDIT messages (the editor itself stays
  // unfocused; the caret reads as focused while the sink has focus).
  event.preventDefault();
  ensureInput()?.focus({ preventScroll: true });
  if (event.shiftKey) {
    // Extend from the current selection's anchor (or the caret).
    const prev = lastCursor;
    const anchor = prev?.selection
      ? (prev.offset === prev.selection.start ? prev.selection.end : prev.selection.start)
      : prev?.offset ?? offset;
    setLocalSelection(anchor, offset);
    window.parent.postMessage({ type: 'CURSOR_CLICK', offset, extend: true }, '*');
    return;
  }
  dragAnchor = offset;
  dragHead = offset;
  rootElement.setPointerCapture(event.pointerId);
  lastCursor = { offset, focused: true };
  scheduleCaretPlacement();
  window.parent.postMessage({ type: 'CURSOR_CLICK', offset }, '*');
});

/** Local caret + selection from an anchor/head pair (head is the caret). */
function setLocalSelection(anchor: number, head: number) {
  lastCursor = {
    offset: head,
    focused: true,
    ...(anchor !== head
      ? { selection: { start: Math.min(anchor, head), end: Math.max(anchor, head) } }
      : {}),
  };
  scheduleCaretPlacement();
}

document.addEventListener('pointermove', (event) => {
  if (dragAnchor == null || (event.buttons & 1) === 0) return;
  const head = sourceOffsetAtPoint(event.clientX, event.clientY, dragAnchor);
  if (head == null || head === dragHead) return;
  dragHead = head;
  setLocalSelection(dragAnchor, head);
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

// ---------------------------------------------------------------------------
// Local AST
//
// The iframe keeps the parsed AST as its own state. `UPDATE_AST` from the
// parent replaces it; a preview-side edit (see "Local edits" below)
// mutates it through the framework's `setLocalAst` path and re-renders
// *immediately*, without waiting for the source → parse → AST
// round-trip. The parent still gets `SET_AST` and writes the changed
// part back to the source with the incremental writer; when its
// re-parsed AST arrives it simply replaces ours.
// ---------------------------------------------------------------------------

let currentAst: PandocAST | null = null;
let currentFilePath = '';

function updateAst(payload: UpdateAstPayload) {
  const { astJson, renderedContent } = payload;
  currentFilePath = payload.currentFilePath;

  // Stale round-trip guard. Local edits run ahead of the parent's
  // source → parse → AST pipeline, and every content change produces an
  // UPDATE_AST in order. One parsed from a source we have *already moved
  // past* (an earlier link of `expectedChain`) would rewind the display
  // into old coordinates — and an edit typed in that moment would be
  // posted from the rewound tree, losing the later keystrokes in the
  // source. Skip those; a newer UPDATE_AST for the latest state is
  // guaranteed to follow. Anything else (our latest state, a writer
  // normalisation, an external edit) is accepted.
  if (localDirty && renderedContent != null) {
    const i = expectedChain.indexOf(renderedContent);
    if (i >= 0 && i < expectedChain.length - 1) return;
  }
  localDirty = false;
  expectedChain = [];
  clearTimeout(roundTripTimer);
  sourceBytes = renderedContent != null ? new TextEncoder().encode(renderedContent) : null;

  const rootElement = document.getElementById('root');
  if (!rootElement) {
    console.error('Root element not found');
    return;
  }
  try {
    currentAst = JSON.parse(astJson) as PandocAST;
  } catch (err) {
    currentAst = null;
    rootElement.innerHTML = `
      <div style="padding: 20px; color: red;">
        <strong>Failed to parse AST:</strong>
        <pre>${err instanceof Error ? err.message : String(err)}</pre>
      </div>
    `;
    return;
  }
  renderAst();
}

function renderAst() {
  const rootElement = document.getElementById('root');
  if (!rootElement || !currentAst) return;

  // Merge q2-debug defaults with any user-TSX overrides. The cast asserts
  // the merged result satisfies the FormatRegistry contract; the override
  // side is babel-transpiled user code and runtime-trusted.
  const mergedRegistry: FormatRegistry = {
    ...q2DebugRegistry,
    ...customRegistry,
  } as FormatRegistry;

  try {
    // Create root only once
    if (!root) {
      root = createRoot(rootElement);
    }
    root.render(
      <Ast
        ast={currentAst}
        currentFilePath={currentFilePath}
        onNavigateToDocument={(path, anchor) => {
          window.parent.postMessage({
            type: 'NAVIGATE_TO_DOCUMENT',
            path,
            anchor
          }, '*');
        }}
        setAst={onLocalAst}
        registry={mergedRegistry}
      />
    );
    // The new tree commits asynchronously; re-anchor the caret once it
    // has painted (the MutationObserver covers React's own scheduling).
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

// ---------------------------------------------------------------------------
// Local edits — tree operations on the local AST
//
// Typing at the caret edits the AST, not the source, and the display
// updates at once; the parent then writes the changed nodes back with
// the incremental writer, and its re-parsed AST replaces ours when it
// lands. Every operation here:
//   1. locates the leaf under the caret in the local tree (with its
//      parent array and index, so siblings can be inserted or removed —
//      which the framework's one-node `setLocalAst` cannot do);
//   2. rewrites that leaf into zero or more nodes. A typed space becomes
//      a real `Space` node, splitting the Str; backspace over a Space
//      removes it; a Str never holds a space. New nodes inherit the
//      source-info id (`s`) of the node they came from — the writer's
//      JSON reader is strict and rejects nodes without one;
//   3. records byte *splices* (at, removed, inserted) in pre-edit
//      coordinates, then remaps every `l` range in the tree, the
//      iframe's source copy, and the caret through them.
// ---------------------------------------------------------------------------

let localDirty = false;
/** Expected source text after each local edit since the last accepted round-trip. */
let expectedChain: string[] = [];
let roundTripTimer: ReturnType<typeof setTimeout> | undefined;

interface LocRange { f?: number; b: { o: number; l?: number; c?: number }; e: { o: number; l?: number; c?: number } }
type AnyNode = Record<string, unknown> & { t?: string; c?: unknown; s?: unknown; l?: LocRange; __fresh?: boolean };
/** A byte edit in pre-edit coordinates. */
interface Splice { at: number; removed: number; inserted: string }

const EDITABLE_LEAVES = new Set(['Str', 'Space', 'SoftBreak', 'LineBreak', 'Math']);

interface Located { node: AnyNode; parent: unknown[]; index: number; b: number; e: number }

/** All editable leaves (with ranges) in document order, with their array slot. */
function locateLeaves(root: unknown): Located[] {
  const out: Located[] = [];
  const walk = (value: unknown, parent: unknown[] | null, index: number) => {
    if (Array.isArray(value)) { value.forEach((v, i) => walk(v, value, i)); return; }
    if (!value || typeof value !== 'object') return;
    const n = value as AnyNode;
    if (n.t && EDITABLE_LEAVES.has(n.t) && n.l?.b && n.l?.e && parent) {
      out.push({ node: n, parent, index, b: n.l.b.o, e: n.l.e.o });
      return;
    }
    for (const [key, v] of Object.entries(n)) {
      if (key === 'l' || key === 'astContext' || key === 'meta') continue;
      walk(v, null, -1);
    }
  };
  walk(root, null, -1);
  return out;
}

function sliceOf(b: number, e: number): string | null {
  return sourceBytes ? utf8Decoder.decode(sourceBytes.subarray(b, e)) : null;
}

/** Escape-aligned helpers on a Str's source slice (fall back to 1:1 bytes). */
function strIndexAt(slice: string | null, text: string, relByte: number): number {
  return slice ? textIndexForSourceByte(slice, text, relByte) : utf16IndexForByteOffset(text, relByte);
}
function strByteAt(slice: string | null, text: string, idx: number): number {
  return slice ? sourceByteForTextIndex(slice, text, idx) : utf8Length(text.slice(0, idx));
}

/** Range object in the new coordinates; `__fresh` tells the remap to leave it alone. */
function freshLoc(template: LocRange | undefined, b: number, e: number): LocRange {
  return {
    f: template?.f ?? 0,
    b: { ...(template?.b ?? {}), o: b },
    e: { ...(template?.e ?? {}), o: e },
  };
}
function mkStr(text: string, b: number, e: number, from: AnyNode): AnyNode {
  return { t: 'Str', c: text, s: from.s, l: freshLoc(from.l, b, e), __fresh: true };
}
function mkSpace(b: number, e: number, from: AnyNode): AnyNode {
  return { t: 'Space', s: from.s, l: freshLoc(from.l, b, e), __fresh: true };
}

/** New position of pre-edit byte `x` after `splices` (sorted by `at`). */
function mapOffset(x: number, splices: Splice[]): number {
  let shift = 0;
  for (const sp of splices) {
    if (x >= sp.at + sp.removed) shift += utf8Length(sp.inserted) - sp.removed;
    else if (x > sp.at) return sp.at + shift;   // inside a removed span: collapse to its start
    else break;
  }
  return x + shift;
}

function remapTree(node: unknown, splices: Splice[]): void {
  if (Array.isArray(node)) { for (const n of node) remapTree(n, splices); return; }
  if (!node || typeof node !== 'object') return;
  const n = node as AnyNode;
  if (n.__fresh) {
    delete n.__fresh;
  } else if (n.l?.b && n.l?.e) {
    n.l.b.o = mapOffset(n.l.b.o, splices);
    n.l.e.o = mapOffset(n.l.e.o, splices);
  }
  for (const [key, v] of Object.entries(n)) {
    if (key === 'l' || key === 'astContext' || key === 'meta') continue;
    remapTree(v, splices);
  }
}

/**
 * Finish a local edit: remap ranges, update the source copy, move the
 * caret (given in post-edit coordinates), render, and post SET_AST.
 */
function commitEdit(splices: Splice[], caret: number): true {
  splices.sort((a, b) => a.at - b.at);
  remapTree(currentAst, splices);
  for (const sp of [...splices].reverse()) {
    if (!sourceBytes) break;
    const ins = new TextEncoder().encode(sp.inserted);
    const out = new Uint8Array(sourceBytes.length - sp.removed + ins.length);
    out.set(sourceBytes.subarray(0, sp.at), 0);
    out.set(ins, sp.at);
    out.set(sourceBytes.subarray(sp.at + sp.removed), sp.at + ins.length);
    sourceBytes = out;
  }
  if (sourceBytes) expectedChain.push(utf8Decoder.decode(sourceBytes));
  localDirty = true;
  lastCursor = { offset: caret, focused: true };
  renderAst();
  window.parent.postMessage({ type: 'SET_AST', ast: currentAst, caretOffset: caret }, '*');
  clearTimeout(roundTripTimer);
  roundTripTimer = setTimeout(() => {
    console.warn('[q2-debug] no AST round-trip 3s after a local edit — is the incremental write failing? (see "Failed to write AST back to QMD" in the parent console)');
  }, 3000);
  return true;
}

/**
 * Byte bounds of a Math node's LaTeX inside its source range (which also
 * covers the `$`/`$$` delimiters). The LaTeX is verbatim in the source,
 * so edits inside it map 1:1 and the node's text is just spliced.
 */
function mathBounds(x: Located): { lb: number; le: number; latex: string } | null {
  if (x.node.t !== 'Math' || !Array.isArray(x.node.c) || typeof x.node.c[1] !== 'string') return null;
  const latex = x.node.c[1] as string;
  const slice = sliceOf(x.b, x.e);
  if (slice == null) return null;
  const at = slice.indexOf(latex);
  if (at < 0) return null;
  const lb = x.b + utf8Length(slice.slice(0, at));
  return { lb, le: lb + utf8Length(latex), latex };
}

/** Replace the Math node's LaTeX, keeping mode and ids; `delta` is the byte change. */
function replaceMath(x: Located, latex: string, delta: number): void {
  const c = x.node.c as [unknown, string];
  x.parent.splice(x.index, 1, {
    ...x.node,
    c: [c[0], latex],
    l: freshLoc(x.node.l, x.b, x.e + delta),
    __fresh: true,
  });
}

/** Insert into a Math node's LaTeX when `offset` lies within it (delimiters excluded). */
function insertIntoMath(leaves: Located[], offset: number, text: string): boolean | null {
  for (const x of leaves) {
    const m = mathBounds(x);
    if (!m || offset < m.lb || offset > m.le) continue;
    const idx = utf16IndexForByteOffset(m.latex, offset - m.lb);
    replaceMath(x, m.latex.slice(0, idx) + text + m.latex.slice(idx), utf8Length(text));
    return commitEdit([{ at: offset, removed: 0, inserted: text }], offset + utf8Length(text));
  }
  return null;                                           // not in any math
}

/** Delete one character of a Math node's LaTeX at `offset` in direction `dir`. */
function deleteInMath(leaves: Located[], offset: number, dir: -1 | 1): boolean | null {
  for (const x of leaves) {
    const m = mathBounds(x);
    if (!m || offset < m.lb || offset > m.le) continue;
    const idx = utf16IndexForByteOffset(m.latex, offset - m.lb);
    let from: number; let to: number;
    if (dir < 0) {
      if (idx === 0) return false;                        // at the LaTeX start: don't eat the `$`
      from = idx - (idx >= 2 && m.latex.codePointAt(idx - 2)! >= 0x10000 ? 2 : 1); to = idx;
    } else {
      if (idx >= m.latex.length) return false;            // at the LaTeX end
      from = idx; to = idx + (m.latex.codePointAt(idx)! >= 0x10000 ? 2 : 1);
    }
    const removed = utf8Length(m.latex.slice(from, to));
    const at = m.lb + utf8Length(m.latex.slice(0, from));
    replaceMath(x, m.latex.slice(0, from) + m.latex.slice(to), -removed);
    return commitEdit([{ at, removed, inserted: '' }], at);
  }
  return null;
}

/** Split typed text into alternating runs of spaces and non-spaces. */
function runsOf(text: string): string[] {
  return text.match(/ +|[^ ]+/g) ?? [];
}

/**
 * Insert `text` at byte `offset`. Host preference: the Str containing
 * the offset, then the Str ending there, then the one starting there,
 * then a Space/SoftBreak on either side. Spaces in `text` become Space
 * nodes; the Str is split around them.
 */
function insertText(offset: number, text: string): boolean {
  if (!currentAst) return false;
  const leaves = locateLeaves(currentAst);
  const inMath = insertIntoMath(leaves, offset, text);
  if (inMath !== null) return inMath;
  const isStr = (x: Located) => x.node.t === 'Str' && typeof x.node.c === 'string';
  const host =
    leaves.find(x => isStr(x) && x.b < offset && offset < x.e) ??
    leaves.find(x => isStr(x) && x.e === offset) ??
    leaves.find(x => isStr(x) && x.b === offset) ??
    leaves.find(x => !isStr(x) && x.node.t !== 'Math' && (x.e === offset || x.b === offset));
  if (!host) return false;

  const runs = runsOf(text);
  const nodes: AnyNode[] = [];
  let cur = host.b;
  const pushStr = (t: string, bytes: number) => {
    if (t) nodes.push(mkStr(t, cur, cur + bytes, host.node));
    cur += bytes;
  };
  const pushSpace = (bytes: number) => { nodes.push(mkSpace(cur, cur + bytes, host.node)); cur += bytes; };

  if (isStr(host)) {
    const oldText = host.node.c as string;
    const slice = sliceOf(host.b, host.e);
    const idx = strIndexAt(slice, oldText, offset - host.b);
    const beforeBytes = strByteAt(slice, oldText, idx);
    const afterBytes = host.e - host.b - beforeBytes;
    const before = oldText.slice(0, idx);
    const after = oldText.slice(idx);
    // Merge `before` into the first run and `after` into the last when
    // those runs are text; otherwise they stand alone.
    let pendingText = before;
    let pendingBytes = beforeBytes;
    runs.forEach((run, i) => {
      const last = i === runs.length - 1;
      if (run[0] === ' ') {
        pushStr(pendingText, pendingBytes); pendingText = ''; pendingBytes = 0;
        pushSpace(utf8Length(run));
      } else {
        pendingText += run; pendingBytes += utf8Length(run);
      }
      if (last) { pushStr(pendingText + after, pendingBytes + afterBytes); pendingText = ''; pendingBytes = 0; }
    });
    if (runs.length === 0) pushStr(before + after, beforeBytes + afterBytes);
  } else {
    // Typing onto a Space / SoftBreak: a leading (at end) or trailing
    // (at start) run of spaces widens the node; other runs become nodes
    // after (at end) or before (at start) it.
    const atEnd = offset === host.e;
    const width = host.e - host.b;
    const isSpace = host.node.t === 'Space';
    const widen = (run: string) => isSpace && run[0] === ' ';
    const original = () => { nodes.push({ ...host.node, l: freshLoc(host.node.l, cur, cur + width), __fresh: true }); cur += width; };
    if (atEnd) {
      const w = runs.length && widen(runs[0]) ? utf8Length(runs[0]) : 0;
      nodes.push({ ...host.node, l: freshLoc(host.node.l, cur, cur + width + w), __fresh: true }); cur += width + w;
      runs.slice(w ? 1 : 0).forEach(run => run[0] === ' ' ? pushSpace(utf8Length(run)) : pushStr(run, utf8Length(run)));
    } else {
      const lastRun = runs[runs.length - 1];
      const w = runs.length && widen(lastRun) ? utf8Length(lastRun) : 0;
      runs.slice(0, w ? -1 : undefined).forEach(run => run[0] === ' ' ? pushSpace(utf8Length(run)) : pushStr(run, utf8Length(run)));
      if (w) { nodes.push({ ...host.node, l: freshLoc(host.node.l, cur, cur + width + w), __fresh: true }); cur += width + w; }
      else original();
    }
  }

  host.parent.splice(host.index, 1, ...nodes);
  return commitEdit([{ at: offset, removed: 0, inserted: text }], offset + utf8Length(text));
}

/** Backspace (dir −1) or forward delete (dir +1) at byte `offset`. */
function deleteAt(offset: number, dir: -1 | 1): boolean {
  if (!currentAst) return false;
  const leaves = locateLeaves(currentAst);
  const inMath = deleteInMath(leaves, offset, dir);
  if (inMath !== null) return inMath;
  const isStr = (x: Located) => x.node.t === 'Str' && typeof x.node.c === 'string';
  // Prefer the leaf whose edge faces the deletion; then a Str containing
  // the offset. A Math node's outer edge is never a host: backspacing
  // after `$$` must not swallow the whole formula.
  const host =
    leaves.find(x => x.node.t !== 'Math' && (dir < 0 ? x.e === offset : x.b === offset)) ??
    leaves.find(x => isStr(x) && x.b < offset && offset < x.e);
  if (!host) return false;

  if (isStr(host)) {
    const text = host.node.c as string;
    const slice = sliceOf(host.b, host.e);
    const idx = strIndexAt(slice, text, offset - host.b);
    let from: number; let to: number;
    if (dir < 0) {
      if (idx === 0) return false;                       // would cross into the previous node
      const twoUnit = idx >= 2 && text.codePointAt(idx - 2)! >= 0x10000;
      from = idx - (twoUnit ? 2 : 1); to = idx;
    } else {
      if (idx >= text.length) return false;              // would cross into the next node
      from = idx; to = idx + (text.codePointAt(idx)! >= 0x10000 ? 2 : 1);
    }
    const newText = text.slice(0, from) + text.slice(to);
    let at: number; let removed: number;
    if (newText === '') {                                // whole node (incl. any escapes) goes
      at = host.b; removed = host.e - host.b;
      host.parent.splice(host.index, 1);
    } else {
      at = host.b + strByteAt(slice, text, from);
      removed = host.b + strByteAt(slice, text, to) - at;
      host.parent.splice(host.index, 1, mkStr(newText, host.b, host.e - removed, host.node));
    }
    return commitEdit([{ at, removed, inserted: '' }], at);
  }

  // Space / SoftBreak / LineBreak: a multi-byte Space shrinks by one; otherwise the node goes.
  const width = host.e - host.b;
  if (host.node.t === 'Space' && width > 1) {
    const at = dir < 0 ? host.e - 1 : host.b;
    host.parent.splice(host.index, 1, mkSpace(host.b, host.e - 1, host.node));
    return commitEdit([{ at, removed: 1, inserted: '' }], at);
  }
  host.parent.splice(host.index, 1);
  return commitEdit([{ at: host.b, removed: width, inserted: '' }], host.b);
}

// ---------------------------------------------------------------------------
// Cross-node selection deletion
//
// For a selection [S, E): every node whose range lies completely inside
// is removed (with its syntax); a Str or Math straddling an end is
// trimmed to its uncovered text; a straddling container is kept and the
// rule recurses into its children. Removed bytes are recorded per node
// or trimmed slice — not as "E − S" — because straddling containers
// keep their syntax (`*`, `$$`, list markers).
// ---------------------------------------------------------------------------

function rangeOf(node: unknown): { b: number; e: number } | null {
  const l = (node as AnyNode | null)?.l;
  return l && l.b && l.e ? { b: l.b.o, e: l.e.o } : null;
}

/** Node with [S, E) deleted, or `null` when fully covered. */
function deleteFromNode(node: unknown, S: number, E: number, splices: Splice[]): unknown {
  if (Array.isArray(node)) {
    const out: unknown[] = [];
    for (const n of node) {
      const r = deleteFromNode(n, S, E, splices);
      if (r !== null) out.push(r);
    }
    return out;
  }
  if (!node || typeof node !== 'object') return node;
  const n = node as AnyNode;
  const range = rangeOf(n);
  if (range) {
    const { b, e } = range;
    if (e <= S || b >= E) return node;
    if (b >= S && e <= E) {
      if (e > b) splices.push({ at: b, removed: e - b, inserted: '' });
      return null;
    }
    const from = Math.max(S, b);
    const to = Math.min(E, e);
    if (n.t === 'Str' && typeof n.c === 'string') {
      const slice = sliceOf(b, e);
      const i0 = strIndexAt(slice, n.c, from - b);
      const i1 = Math.max(i0, strIndexAt(slice, n.c, to - b));
      if (i1 > i0) splices.push({ at: from, removed: to - from, inserted: '' });
      return { ...n, c: n.c.slice(0, i0) + n.c.slice(i1) };
    }
    if (n.t === 'Math' && Array.isArray(n.c) && typeof n.c[1] === 'string') {
      const latex = n.c[1] as string;
      const slice = sliceOf(b, e);
      const at = slice ? slice.indexOf(latex) : -1;
      if (at < 0) return node;
      const lb = b + utf8Length(slice!.slice(0, at));
      const le = lb + utf8Length(latex);
      const f = Math.max(from, lb);
      const t = Math.min(to, le);
      if (t <= f) return node;                           // only delimiters covered
      splices.push({ at: f, removed: t - f, inserted: '' });
      const j0 = utf16IndexForByteOffset(latex, f - lb);
      const j1 = utf16IndexForByteOffset(latex, t - lb);
      return { ...n, c: [n.c[0], latex.slice(0, j0) + latex.slice(j1)] };
    }
  }
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(n)) {
    out[key] = key === 'l' || key === 'astContext' || key === 'meta' || key === 's'
      ? value
      : deleteFromNode(value, S, E, splices);
  }
  return out;
}

function deleteSelection(S: number, E: number, insert: string): boolean {
  if (!currentAst || E <= S) return false;
  const splices: Splice[] = [];
  const newAst = deleteFromNode(currentAst, S, E, splices) as PandocAST | null;
  if (!newAst) return false;
  if (splices.length === 0) return insert ? insertText(S, insert) : false;
  currentAst = newAst;
  splices.sort((a, b) => a.at - b.at);
  const caret = mapOffset(S, splices);
  commitEdit(splices, caret);
  // A typed replacement goes in as a second edit on the updated tree.
  if (insert) insertText(caret, insert);
  return true;
}

/**
 * Entry point for the input sink. Returns true when applied. Returns
 * false when the edit is not expressible as an AST change — the caller
 * then does nothing; text is never pushed to Monaco as a raw source edit.
 */
function applyLocalEdit(kind: 'type' | 'deleteLeft' | 'deleteRight', text = ''): boolean {
  const cursor = lastCursor;
  if (!cursor || !currentAst) return false;
  if (kind === 'type' && (text === '' || /[\r\n]/.test(text))) return false;
  if (cursor.selection) {
    return deleteSelection(cursor.selection.start, cursor.selection.end, kind === 'type' ? text : '');
  }
  if (kind === 'type') return insertText(cursor.offset, text);
  return deleteAt(cursor.offset, kind === 'deleteLeft' ? -1 : 1);
}

/** Non-edit `setAst` calls from components (user TSX, etc.): render and forward. */
function onLocalAst(newAst: PandocAST) {
  currentAst = newAst;
  renderAst();
  window.parent.postMessage({ type: 'SET_AST', ast: newAst }, '*');
}

// Signal that the iframe is ready to receive messages
window.parent.postMessage({ type: 'IFRAME_READY' }, '*');
