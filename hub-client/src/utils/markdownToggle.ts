/**
 * Toggle an inline markdown wrapper (e.g. `**` for bold) around a
 * selection. Pure: takes the selected text and the text immediately
 * around it, returns what to write and how far to extend the replaced
 * range.
 */

export interface ToggleResult {
  /** Replacement text for the (possibly extended) range. */
  text: string;
  /** Characters to extend the range by on each side (to eat existing markers). */
  extendBefore: number;
  extendAfter: number;
  /** Where the cursor/selection should land, relative to the start of `text`. */
  selectStart: number;
  selectEnd: number;
}

export function toggleWrap(
  selected: string,
  marker: string,
  before: string,
  after: string
): ToggleResult {
  const m = marker.length;
  // Case 1: the selection itself carries the markers → strip them.
  if (selected.length >= 2 * m && selected.startsWith(marker) && selected.endsWith(marker)) {
    const inner = selected.slice(m, selected.length - m);
    return { text: inner, extendBefore: 0, extendAfter: 0, selectStart: 0, selectEnd: inner.length };
  }
  // Case 2: the markers sit just outside the selection → strip them.
  if (before.endsWith(marker) && after.startsWith(marker)) {
    return {
      text: selected,
      extendBefore: m,
      extendAfter: m,
      selectStart: 0,
      selectEnd: selected.length,
    };
  }
  // Case 3: wrap. With nothing selected, leave the cursor between the markers.
  return {
    text: `${marker}${selected}${marker}`,
    extendBefore: 0,
    extendAfter: 0,
    selectStart: m,
    selectEnd: m + selected.length,
  };
}
