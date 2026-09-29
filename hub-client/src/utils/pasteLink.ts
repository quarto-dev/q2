/**
 * Paste-a-link-over-a-selection: with text selected, pasting a bare URL
 * yields `[selection](url)` instead of replacing the selection.
 */

const URL_RE = /^(https?:\/\/|mailto:)\S+$/i;

/** True for a single bare URL (no surrounding text or whitespace). */
export function isBareUrl(text: string): boolean {
  const t = text.trim();
  return t.length > 0 && !/\s/.test(t) && URL_RE.test(t);
}

/**
 * Markdown to insert when `pasted` is pasted over `selected`, or null
 * when normal paste behavior should apply: no selection, a multi-line
 * selection, a selection that already is a link, or a non-URL paste.
 */
export function linkFromPaste(selected: string, pasted: string): string | null {
  if (!selected || selected.includes('\n')) return null;
  if (!isBareUrl(pasted)) return null;
  const label = selected;
  // Already `[text](…)` or a raw URL: leave it to the plain paste.
  if (/^\[.*\]\(.*\)$/.test(label) || isBareUrl(label)) return null;
  return `[${label}](${pasted.trim()})`;
}
