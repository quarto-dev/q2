/** A row or a pane, in screen pixels (what Playwright's `boundingBox()` returns). */
export interface Box {
  y: number;
  height: number;
}

/**
 * Rows that must fit below a row for it to count as in view: the text it leads into. A narrow editor soft-wraps every
 * source line into several rows, so a paragraph a few lines long can take eight or nine; asking for only a row or two
 * of room cut the end of it off.
 */
const ROOM_ROWS = 12;
/** Where in the pane a row is aimed when it has to move: a fifth of the way down. */
const TARGET = 0.2;
/** The largest wheel event sent at once. Monaco moves the view by about this much per event however large the delta. */
const WHEEL_STEP = 40;

/**
 * How far to wheel an editor to bring `row` into view; 0 when it is already there, and also when it is not rendered
 * (Monaco draws only the rows in view, so there is nothing to measure). The demo scrolls only when it must: an editor that
 * jumps back and forth is distracting, and every jump is time on camera.
 */
export function scrollDelta(row: Box | null, pane: Box): number {
  if (!row) return 0;
  const room = row.height * ROOM_ROWS;
  if (row.y >= pane.y && row.y + room <= pane.y + pane.height) return 0;
  return Math.round(row.y - (pane.y + pane.height * TARGET));
}

/**
 * A distance as a burst of small wheel events. One large event moves the view by only a little (about 50 px), so a long
 * scroll took dozens of calls with a pause after each; a burst sent in quick succession is one gesture, as a hand on a
 * wheel makes it.
 */
export function wheelBursts(distance: number): number[] {
  const out: number[] = [];
  let left = Math.round(distance);
  while (left !== 0) {
    const step = Math.sign(left) * Math.min(WHEEL_STEP, Math.abs(left));
    out.push(step);
    left -= step;
  }
  return out;
}
