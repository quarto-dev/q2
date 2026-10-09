// The outline of a union of axis-aligned rectangles, as an SVG path with
// rounded corners. Pure geometry; used to draw the selection hull.
//
// The union is computed on the compressed grid of all rectangle edges:
// each grid cell is either covered or not, every cell edge between a
// covered and an uncovered cell is a boundary segment, and the segments
// are chained into closed loops with the covered side on the right.

export type Box = { x0: number; y0: number; x1: number; y1: number };

type Pt = [number, number];

const uniqueSorted = (vs: number[]): number[] =>
  [...new Set(vs)].sort((a, b) => a - b);

// Index of `v` in the sorted array `vs` (every query value is present).
const indexIn = (vs: number[], v: number): number => {
  let lo = 0;
  let hi = vs.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (vs[mid] < v) lo = mid + 1;
    else hi = mid;
  }
  return lo;
};

// Closed boundary loops of the union, each a list of corner points in
// clockwise order (screen coordinates, y down), collinear points removed.
export const outline = (boxes: Box[]): Pt[][] => {
  const xs = uniqueSorted(boxes.flatMap((b) => [b.x0, b.x1]));
  const ys = uniqueSorted(boxes.flatMap((b) => [b.y0, b.y1]));
  const w = xs.length - 1;
  const h = ys.length - 1;
  if (w <= 0 || h <= 0) return [];

  const covered = new Uint8Array(w * h);
  for (const b of boxes) {
    const i0 = indexIn(xs, b.x0);
    const i1 = indexIn(xs, b.x1);
    const j0 = indexIn(ys, b.y0);
    const j1 = indexIn(ys, b.y1);
    for (let j = j0; j < j1; j++) for (let i = i0; i < i1; i++) covered[j * w + i] = 1;
  }
  const at = (i: number, j: number): boolean =>
    i >= 0 && i < w && j >= 0 && j < h && covered[j * w + i] === 1;

  // Directed segments keyed by their start point.
  const segments = new Map<string, Pt[]>();
  const key = (p: Pt): string => `${p[0]},${p[1]}`;
  const add = (from: Pt, to: Pt): void => {
    const list = segments.get(key(from));
    if (list === undefined) segments.set(key(from), [to]);
    else list.push(to);
  };
  for (let j = 0; j <= h; j++) {
    for (let i = 0; i < w; i++) {
      const below = at(i, j);
      const above = at(i, j - 1);
      if (below && !above) add([xs[i], ys[j]], [xs[i + 1], ys[j]]); // top edge, rightwards
      if (above && !below) add([xs[i + 1], ys[j]], [xs[i], ys[j]]); // bottom edge, leftwards
    }
  }
  for (let i = 0; i <= w; i++) {
    for (let j = 0; j < h; j++) {
      const right = at(i, j);
      const left = at(i - 1, j);
      if (right && !left) add([xs[i], ys[j + 1]], [xs[i], ys[j]]); // left edge, upwards
      if (left && !right) add([xs[i], ys[j]], [xs[i], ys[j + 1]]); // right edge, downwards
    }
  }

  const loops: Pt[][] = [];
  for (const [startKey, tos] of segments) {
    while (tos.length > 0) {
      const loop: Pt[] = [];
      let cur = startKey;
      for (;;) {
        const next = segments.get(cur)?.pop();
        if (next === undefined) break;
        loop.push(next);
        cur = key(next);
        if (cur === startKey) break;
      }
      if (loop.length > 0) loops.push(simplify(loop));
    }
  }
  return loops;
};

// Drop points that lie on a straight line between their neighbours.
const simplify = (loop: Pt[]): Pt[] => {
  const out: Pt[] = [];
  const n = loop.length;
  for (let i = 0; i < n; i++) {
    const prev = loop[(i + n - 1) % n];
    const cur = loop[i];
    const next = loop[(i + 1) % n];
    const straight = (prev[0] === cur[0] && cur[0] === next[0]) || (prev[1] === cur[1] && cur[1] === next[1]);
    if (!straight) out.push(cur);
  }
  return out;
};

// An SVG path for `loops`, with corners rounded by up to `radius`.
export const roundedPath = (loops: Pt[][], radius: number): string => {
  const parts: string[] = [];
  for (const loop of loops) {
    const n = loop.length;
    if (n < 3) continue;
    const cmds: string[] = [];
    for (let i = 0; i <= n; i++) {
      const prev = loop[(i + n - 1) % n];
      const cur = loop[i % n];
      const next = loop[(i + 1) % n];
      const dIn = Math.hypot(cur[0] - prev[0], cur[1] - prev[1]);
      const dOut = Math.hypot(next[0] - cur[0], next[1] - cur[1]);
      const r = Math.min(radius, dIn / 2, dOut / 2);
      const a: Pt = [cur[0] + ((prev[0] - cur[0]) / dIn) * r, cur[1] + ((prev[1] - cur[1]) / dIn) * r];
      const b: Pt = [cur[0] + ((next[0] - cur[0]) / dOut) * r, cur[1] + ((next[1] - cur[1]) / dOut) * r];
      if (i === 0) cmds.push(`M${b[0]},${b[1]}`);
      else cmds.push(`L${a[0]},${a[1]} Q${cur[0]},${cur[1]} ${b[0]},${b[1]}`);
    }
    cmds.push("Z");
    parts.push(cmds.join(" "));
  }
  return parts.join(" ");
};
