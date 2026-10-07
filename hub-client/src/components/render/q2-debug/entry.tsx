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

let selectionEl: HTMLDivElement | null = null;

function ensureSelectionLayer(): HTMLDivElement | null {
  const rootElement = document.getElementById('root');
  if (!rootElement) return null;
  if (!selectionEl || !selectionEl.isConnected) {
    selectionEl = document.createElement('div');
    selectionEl.className = 'q2-debug-selection';
    rootElement.appendChild(selectionEl);
  }
  return selectionEl;
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
  layer: HTMLDivElement,
  selection: { start: number; end: number } | undefined,
) {
  layer.replaceChildren();
  if (!selection || selection.end <= selection.start) return;
  const rootRect = rootElement.getBoundingClientRect();
  const addRect = (r: DOMRect) => {
    if (r.width <= 0 && r.height <= 0) return;
    const d = document.createElement('div');
    d.className = 'q2-debug-selection-rect';
    d.style.left = `${r.left - rootRect.left + rootElement.scrollLeft}px`;
    d.style.top = `${r.top - rootRect.top + rootElement.scrollTop}px`;
    d.style.width = `${Math.max(r.width, 1)}px`;
    d.style.height = `${r.height}px`;
    layer.appendChild(d);
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
      for (const rect of r.getClientRects()) addRect(rect);
      continue;
    }
    if (el.querySelector(`[${DATA_OFF}]`)) continue; // container
    addRect(el.getBoundingClientRect());
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
  el: HTMLElement;
  start: number;
  end: number;
}

/**
 * Pick the element whose stamped range should host the caret.
 *
 * Preference order mirrors `findElementForLine` in scrollSyncDom.ts,
 * but at byte granularity: the smallest containing range wins (ties go
 * to the later element in document order, i.e. the deeper one); when
 * nothing contains the offset — blank lines, front matter, the gap
 * after the last block — fall back to the nearest preceding node,
 * then the first following one.
 */
function findCaretTarget(rootElement: HTMLElement, offset: number): CaretTarget | null {
  let best: CaretTarget | null = null;
  let preceding: CaretTarget | null = null;
  let following: CaretTarget | null = null;
  for (const el of rootElement.querySelectorAll<HTMLElement>(`[${DATA_OFF}]`)) {
    const range = parseDataOff(el.getAttribute(DATA_OFF) ?? '');
    if (!range) continue;
    const { start, end } = range;
    if (offset >= start && offset <= end) {
      if (!best || end - start <= best.end - best.start) best = { el, start, end };
    } else if (end < offset) {
      if (!preceding || end >= preceding.end) preceding = { el, start, end };
    } else if (start > offset) {
      if (!following || start < following.start) following = { el, start, end };
    }
  }
  return best ?? preceding ?? following;
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
  const textHost = target.el.querySelector<HTMLElement>(`:scope > [${DATA_STR_TEXT}]`);
  const textNode = textHost?.firstChild;
  if (
    textNode &&
    // `Node` here is the framework component import, so use the DOM constant.
    textNode.nodeType === 3 /* TEXT_NODE */ &&
    cursor.offset >= target.start &&
    cursor.offset <= target.end
  ) {
    // Inside a Str: land between characters.
    const text = textNode.textContent ?? '';
    const rel = cursor.offset - target.start;
    // With the source at hand, align it against the rendered text so
    // escapes/entities don't skew the index; otherwise assume 1:1 bytes.
    const idx = sourceBytes
      ? textIndexForSourceByte(
          utf8Decoder.decode(sourceBytes.subarray(target.start, target.end)),
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
      const host = textHost!.getBoundingClientRect();
      rect = new DOMRect(idx === 0 ? host.left : host.right, host.top, 0, host.height);
    }
  } else {
    // Not a Str (Space, SoftBreak, a container whose syntax is under the
    // caret, or a fallback neighbour): snap to the nearer edge of its badge.
    const box = target.el.getBoundingClientRect();
    const distStart = Math.abs(cursor.offset - target.start);
    const distEnd = Math.abs(target.end - cursor.offset);
    const atStart = cursor.offset < target.start || distStart < distEnd;
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
    requestAnimationFrame(() => { invalidateLeaves(); scheduleCaretPlacement(); });
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
