// A caret as a stable position in a parse tree. No DOM, no layout.
//
// Indices sit *between* things in the AST (see IDEA.md):
//
//   0
//   para{
//     1
//     text{2h3e4l5l6o7}
//     8
//   }
//   9
//
// A position is a node plus a place relative to it: before it, after it,
// or (for a leaf) an offset between two of its characters. Some places
// coincide: after one sibling is before the next, and a container's
// first inner index is before its first child. A position is kept in
// *canonical* form so that every index has exactly one representation:
//
//   - `before` any node;
//   - an offset 0..text.length into a leaf;
//   - `after` a node only if it is the last child (or the root).
//
// Left and right movement are walks over the tree. After a reparse the
// position re-resolves by node identity: inc_parsers.ts hands back the
// same node objects for everything an edit did not touch, so a position
// whose node survived is found again directly. If the node is gone (the
// edit was inside it), the position falls back to the same child indices
// from the root.

import { ParseTree } from "./inc_parsers";

export type At = "before" | "after" | number;

export type Position = {
  // The node and its ancestors, root first.
  path: ParseTree[];
  at: At;
};

const nodeOf = (p: Position): ParseTree => p.path[p.path.length - 1];
const parentOf = (p: Position): ParseTree | null => (p.path.length > 1 ? p.path[p.path.length - 2] : null);
const siblingIndex = (p: Position): number => parentOf(p)!.children.indexOf(nodeOf(p));

const withNode = (p: Position, n: ParseTree, at: At): Position => ({ path: [...p.path.slice(0, -1), n], at });
const withChild = (p: Position, i: number, at: At): Position => ({ path: [...p.path, nodeOf(p).children[i]], at });
const withParent = (p: Position, at: At): Position => ({ path: p.path.slice(0, -1), at });
const withAt = (p: Position, at: At): Position => ({ path: p.path, at });

// `after` a node that has a next sibling is the same place as `before`
// that sibling; prefer the latter.
const canonical = (p: Position): Position => {
  if (p.at !== "after") return p;
  const parent = parentOf(p);
  if (parent === null) return p;
  const i = siblingIndex(p);
  const siblings = parent.children;
  return i === siblings.length - 1 ? p : withNode(p, siblings[i + 1], "before");
};

export const start = (root: ParseTree): Position => ({ path: [root], at: "before" });
export const end = (root: ParseTree): Position => ({ path: [root], at: "after" });

export const same = (a: Position, b: Position): boolean =>
  nodeOf(a) === nodeOf(b) && a.at === b.at;

// The first inner position of a node.
const innerStart = (p: Position): Position =>
  nodeOf(p).children.length === 0 ? withAt(p, 0) : withChild(p, 0, "before");

// The last inner position of a node.
const innerEnd = (p: Position): Position => {
  const n = nodeOf(p);
  return n.children.length === 0 ? withAt(p, n.text.length) : withChild(p, n.children.length - 1, "after");
};

export const right = (p: Position): Position => {
  const n = nodeOf(p);
  if (p.at === "before") return innerStart(p);
  if (typeof p.at === "number") {
    return p.at < n.text.length ? withAt(p, p.at + 1) : canonical(withAt(p, "after"));
  }
  // `after` the last child: leave the parent too.
  return parentOf(p) === null ? p : canonical(withParent(p, "after"));
};

export const left = (p: Position): Position => {
  if (p.at === "after") return innerEnd(p);
  if (typeof p.at === "number") return p.at > 0 ? withAt(p, p.at - 1) : withAt(p, "before");
  // `before`: step into the previous sibling's end, or out to the parent.
  const parent = parentOf(p);
  if (parent === null) return p;
  const i = siblingIndex(p);
  return i === 0 ? withParent(p, "before") : innerEnd(withNode(p, parent.children[i - 1], "before"));
};

// The number of indices a node spans: after(node) - before(node).
const size = (n: ParseTree): number =>
  n.children.length === 0 ? n.text.length + 2 : n.children.reduce((s, c) => s + size(c), 2);

// The index of a position, as in the diagram above.
export const indexOf = (p: Position): number => {
  let idx = 0;
  for (let k = 0; k + 1 < p.path.length; k++) {
    idx += 1;
    for (const sibling of p.path[k].children) {
      if (sibling === p.path[k + 1]) break;
      idx += size(sibling);
    }
  }
  const n = nodeOf(p);
  if (p.at === "before") return idx;
  if (p.at === "after") return idx + size(n);
  return idx + 1 + p.at;
};

// The path from `root` to `target`, or null if `target` is not in the tree.
const findPath = (root: ParseTree, target: ParseTree): ParseTree[] | null => {
  if (root === target) return [root];
  for (const c of root.children) {
    const p = findPath(c, target);
    if (p !== null) return [root, ...p];
  }
  return null;
};

// Keep `at` meaningful for the node at the end of `path`.
const clampAt = (path: ParseTree[], at: At): At => {
  if (typeof at !== "number") return at;
  const n = path[path.length - 1];
  return n.children.length === 0 ? Math.min(at, n.text.length) : "before";
};

// Find `p` again in `root` after a reparse.
export const resolve = (root: ParseTree, p: Position): Position => {
  // The node survived: same place relative to it.
  const path = findPath(root, nodeOf(p));
  if (path !== null) return canonical({ path, at: clampAt(path, p.at) });

  // `before` a replaced node is also `after` its previous sibling.
  const parent = parentOf(p);
  if (p.at === "before" && parent !== null) {
    const i = siblingIndex(p);
    if (i > 0) {
      const prev = findPath(root, parent.children[i - 1]);
      if (prev !== null) return canonical({ path: prev, at: "after" });
    }
  }

  // Follow the same child indices from the root as far as they go.
  const steps = p.path.slice(1).map((n, k) => p.path[k].children.indexOf(n));
  const fallback = [root];
  for (const step of steps) {
    const cur = fallback[fallback.length - 1].children;
    if (cur.length === 0) break;
    fallback.push(cur[Math.min(step, cur.length - 1)]);
  }
  return canonical({ path: fallback, at: clampAt(fallback, p.at) });
};

// Tree order of two positions in the same tree.
export const compare = (a: Position, b: Position): number => indexOf(a) - indexOf(b);

// What lies between two positions: whole nodes (maximal ones that fit
// entirely inside the range) and, at the ends, runs of characters of a
// partly covered leaf. This is the "contents" of a selection in IDEA.md's
// sense; `from`/`to` are present only for partial leaves.
export type Selected = { node: ParseTree; from?: number; to?: number };

export const between = (root: ParseTree, a: Position, b: Position): Selected[] => {
  const lo = Math.min(indexOf(a), indexOf(b));
  const hi = Math.max(indexOf(a), indexOf(b));
  const out: Selected[] = [];
  const collect = (n: ParseTree, before: number): void => {
    const after = before + size(n);
    if (after <= lo || before >= hi) return;
    if (before >= lo && after <= hi) {
      out.push({ node: n });
      return;
    }
    if (n.children.length === 0) {
      const from = Math.max(0, lo - before - 1);
      const to = Math.min(n.text.length, hi - before - 1);
      if (from < to) out.push({ node: n, from, to });
      return;
    }
    let at = before + 1;
    for (const c of n.children) {
      collect(c, at);
      at += size(c);
    }
  };
  collect(root, 0);
  return out;
};

// A caret with an anchor: the selection runs from the anchor to the
// position and is empty while they coincide.
export class Caret {
  position: Position;
  anchor: Position;

  constructor(root: ParseTree, private readonly onChange: (c: Caret) => void) {
    this.position = start(root);
    this.anchor = this.position;
  }

  // Move the position; the anchor follows unless `extend` is set.
  moveTo(p: Position, extend = false): void {
    this.position = p;
    if (!extend) this.anchor = p;
    this.onChange(this);
  }

  left(extend = false): void {
    this.moveTo(left(this.position), extend);
  }

  right(extend = false): void {
    this.moveTo(right(this.position), extend);
  }

  // The selection in tree order, or null when it is empty.
  selection(): [Position, Position] | null {
    const c = compare(this.anchor, this.position);
    if (c === 0) return null;
    return c < 0 ? [this.anchor, this.position] : [this.position, this.anchor];
  }

  // The tree was reparsed; find the same places in the new one.
  reparsed(root: ParseTree): void {
    this.anchor = resolve(root, this.anchor);
    this.moveTo(resolve(root, this.position), true);
  }
}
