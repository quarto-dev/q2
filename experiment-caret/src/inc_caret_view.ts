// Rendering for the caret in inc_caret.ts. Everything here depends on
// layout: where each position is drawn, and which position is nearest
// to a click. The caret itself knows nothing of this.
//
// The rendering technique follows UI_CARET.md: a leaf's text is a single
// text node, a collapsed DOM Range at a character offset gives the exact
// inter-character x position, and a click never resolves to a container,
// only to the nearest drawn position.

import { ParseTree } from "./inc_parsers";
import { Caret, Position, between, left, right, start, end } from "./inc_caret";
import { Box, outline, roundedPath } from "./inc_hull";

// Where a position is drawn: the gap before character `offset` of a text
// node, or the left/right edge of an element's box.
type Locator =
  | { kind: "text"; node: Text; offset: number }
  | { kind: "edge"; el: HTMLElement; side: "left" | "right" };

type NodeLocators = { el: HTMLElement; before: Locator; after: Locator; offsets: Locator[] };

export type Layout = {
  root: HTMLElement;
  tree: ParseTree;
  // Every canonical position in the tree, with where it is drawn.
  points: { pos: Position; loc: Locator }[];
  locators: Map<ParseTree, NodeLocators>;
};

// Show whitespace control characters as single visible glyphs. Each
// replacement is one UTF-16 unit, so text-node offsets still equal
// source string indices.
const glyphs: Record<string, string> = { "\n": "⏎", "\r": "␍", "\t": "⇥" };
const display = (s: string): string => s.replace(/[\n\r\t]/g, (c) => glyphs[c]);

// Render `t` into a nested-box DOM.
export const layout = (t: ParseTree): Layout => {
  const out: Layout = { root: document.createElement("div"), tree: t, points: [], locators: new Map() };
  out.root = render(t, [], out);
  return out;
};

const render = (t: ParseTree, ancestors: ParseTree[], out: Layout): HTMLDivElement => {
  const el = document.createElement("div");
  el.className = "node";
  const label = document.createElement("span");
  label.className = "label";
  label.textContent = t.type;
  el.append(label);

  const path = [...ancestors, t];
  const locs: NodeLocators = {
    el,
    before: { kind: "edge", el, side: "left" },
    after: { kind: "edge", el, side: "right" },
    offsets: [],
  };
  out.locators.set(t, locs);
  out.points.push({ pos: { path, at: "before" }, loc: locs.before });

  if (t.children.length === 0) {
    const text = document.createElement("span");
    text.className = "text";
    el.append(text);
    const str = t.text;
    if (str.length === 0) {
      locs.offsets.push({ kind: "edge", el: text, side: "left" });
    } else {
      const node = document.createTextNode(display(str));
      text.append(node);
      for (let i = 0; i <= str.length; i++) locs.offsets.push({ kind: "text", node, offset: i });
    }
    locs.offsets.forEach((loc, i) => out.points.push({ pos: { path, at: i }, loc }));
  } else {
    const box = document.createElement("div");
    box.className = "kids";
    el.append(box);
    for (const child of t.children) box.append(render(child, path, out));
  }

  // `after` is canonical only for a last child or the root.
  const parent = ancestors[ancestors.length - 1];
  if (parent === undefined || parent.children[parent.children.length - 1] === t) {
    out.points.push({ pos: { path, at: "after" }, loc: locs.after });
  }
  return el;
};

// Undefined when the position's node is not in this layout, which happens
// briefly between installing a new layout and re-resolving the caret.
const locatorOf = (lay: Layout, p: Position): Locator | undefined => {
  const locs = lay.locators.get(p.path[p.path.length - 1]);
  if (locs === undefined) return undefined;
  if (p.at === "before") return locs.before;
  if (p.at === "after") return locs.after;
  return locs.offsets[p.at];
};

// A rect in the scroll container's content coordinates.
type Rect = { x: number; top: number; bottom: number };

// How far the selection hull extends beyond the boxes it covers. Boxes
// have 1px margins, so this also closes the gaps between neighbours.
const HULL_PAD = 2;
const HULL_RADIUS = 3;

const SVG = "http://www.w3.org/2000/svg";

export class CaretView {
  private lay: Layout | null = null;
  private rects: Rect[] | null = null;
  private readonly el: HTMLDivElement;
  private readonly svg: SVGSVGElement;
  private readonly hull: SVGPathElement;

  constructor(private readonly container: HTMLElement, private readonly caret: Caret) {
    this.el = document.createElement("div");
    this.el.className = "caret";
    this.svg = document.createElementNS(SVG, "svg");
    this.svg.setAttribute("class", "selection");
    this.hull = document.createElementNS(SVG, "path");
    this.svg.append(this.hull);
    container.tabIndex = 0;
    container.addEventListener("mousedown", this.onMouseDown);
    container.addEventListener("keydown", this.onKeyDown);
    window.addEventListener("resize", () => {
      this.rects = null;
      this.draw();
    });
  }

  // Install a freshly rendered layout.
  set(lay: Layout | null): void {
    this.lay = lay;
    this.rects = null;
    if (lay === null) {
      this.el.remove();
      this.svg.remove();
    } else {
      this.draw();
    }
  }

  draw(): void {
    if (this.lay === null) return;
    const loc = locatorOf(this.lay, this.caret.position);
    if (loc === undefined) return;
    const r = this.rectOf(loc);
    this.el.style.left = `${r.x - 1}px`;
    this.el.style.top = `${r.top}px`;
    this.el.style.height = `${r.bottom - r.top}px`;
    this.container.prepend(this.svg);
    this.container.append(this.el);
    this.drawSelection();
  }

  // The hull: the union of the boxes of whole selected nodes and the
  // line rects of partly selected leaf text, padded and outlined.
  private drawSelection(): void {
    const lay = this.lay!;
    const sel = this.caret.selection();
    if (sel === null) {
      this.hull.setAttribute("d", "");
      return;
    }
    const boxes: Box[] = [];
    for (const item of between(lay.tree, sel[0], sel[1])) {
      const locs = lay.locators.get(item.node);
      if (locs === undefined) continue;
      if (item.from === undefined || item.to === undefined) {
        boxes.push(this.boxOf(locs.el.getBoundingClientRect()));
      } else if (locs.offsets[0].kind === "text") {
        const range = document.createRange();
        range.setStart(locs.offsets[0].node, item.from);
        range.setEnd(locs.offsets[0].node, item.to);
        for (const r of range.getClientRects()) boxes.push(this.boxOf(r));
      }
    }
    this.svg.setAttribute("width", `${this.container.scrollWidth}`);
    this.svg.setAttribute("height", `${this.container.scrollHeight}`);
    this.hull.setAttribute("d", roundedPath(outline(boxes), HULL_RADIUS));
  }

  // A viewport-space DOMRect as a padded content-space box.
  private boxOf(rect: DOMRect): Box {
    const c = this.container.getBoundingClientRect();
    const dx = this.container.scrollLeft - c.left;
    const dy = this.container.scrollTop - c.top;
    return {
      x0: rect.left + dx - HULL_PAD,
      y0: rect.top + dy - HULL_PAD,
      x1: rect.right + dx + HULL_PAD,
      y1: rect.bottom + dy + HULL_PAD,
    };
  }

  // Convert a viewport-space DOMRect edge into content coordinates.
  private toContent(rect: DOMRect, side: "left" | "right"): Rect {
    const c = this.container.getBoundingClientRect();
    const x = (side === "left" ? rect.left : rect.right) - c.left + this.container.scrollLeft;
    const dy = this.container.scrollTop - c.top;
    return { x, top: rect.top + dy, bottom: rect.bottom + dy };
  }

  private rectOf(loc: Locator): Rect {
    if (loc.kind === "edge") return this.toContent(loc.el.getBoundingClientRect(), loc.side);
    const range = document.createRange();
    range.setStart(loc.node, loc.offset);
    range.setEnd(loc.node, loc.offset);
    const rect = range.getClientRects()[0];
    if (rect !== undefined && rect.height > 0) return this.toContent(rect, "left");
    // Some engines give no rect for a collapsed range at a text node's
    // very start or end; use the edge of the enclosing span instead.
    const span = loc.node.parentElement as HTMLElement;
    return this.toContent(span.getBoundingClientRect(), loc.offset === 0 ? "left" : "right");
  }

  private allRects(): Rect[] {
    if (this.rects === null) {
      this.rects = this.lay === null ? [] : this.lay.points.map((pt) => this.rectOf(pt.loc));
    }
    return this.rects;
  }

  // The position drawn nearest to a content-space point: nearest row first
  // (vertical distance to the caret's extent), then nearest horizontally.
  private nearest(x: number, y: number): Position {
    let best = 0;
    let bestDy = Infinity;
    let bestDx = Infinity;
    this.allRects().forEach((r, i) => {
      const dy = y < r.top ? r.top - y : y > r.bottom ? y - r.bottom : 0;
      const dx = Math.abs(r.x - x);
      if (dy < bestDy || (dy === bestDy && dx < bestDx)) {
        best = i;
        bestDy = dy;
        bestDx = dx;
      }
    });
    return this.lay!.points[best].pos;
  }

  private positionAt(e: MouseEvent): Position {
    const c = this.container.getBoundingClientRect();
    const x = e.clientX - c.left + this.container.scrollLeft;
    const y = e.clientY - c.top + this.container.scrollTop;
    return this.nearest(x, y);
  }

  // Click places the caret (Shift extends); dragging extends from there.
  private onMouseDown = (e: MouseEvent): void => {
    if (this.lay === null || e.button !== 0) return;
    e.preventDefault();
    this.container.focus();
    this.caret.moveTo(this.positionAt(e), e.shiftKey);
    const onMove = (ev: MouseEvent): void => {
      if (this.lay === null) return;
      this.caret.moveTo(this.positionAt(ev), true);
    };
    const onUp = (): void => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  private onKeyDown = (e: KeyboardEvent): void => {
    if (this.lay === null) return;
    const p = this.caret.position;
    const extend = e.shiftKey;
    switch (e.key) {
      case "ArrowLeft":
        this.caret.moveTo(left(p), extend);
        break;
      case "ArrowRight":
        this.caret.moveTo(right(p), extend);
        break;
      case "Home":
        this.caret.moveTo(start(p.path[0]), extend);
        break;
      case "End":
        this.caret.moveTo(end(p.path[0]), extend);
        break;
      default:
        return;
    }
    e.preventDefault();
  };
}
